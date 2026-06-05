use std::collections::VecDeque;

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::WorldGenConfig;
use super::progress::{ProgressHandle, report_stage};
use super::world::{Biome, WorldMap};

const DIRS: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

fn interior_slope_at(
    width: usize,
    height: usize,
    elevation: &[f32],
    water: &[bool],
    x: usize,
    y: usize,
) -> f32 {
    let idx = y * width + x;
    let elev = elevation[idx];
    let mut max_delta = 0.0f32;

    for (dx, dy) in DIRS {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
            continue;
        }
        let nidx = ny as usize * width + nx as usize;
        if water[nidx] {
            continue;
        }
        max_delta = max_delta.max((elev - elevation[nidx]).abs());
    }
    max_delta
}

/// Chebyshev distance from each cell to the nearest water cell.
fn distance_to_water(width: usize, height: usize, water: &[bool]) -> Vec<u32> {
    let len = width * height;
    let mut dist = vec![u32::MAX; len];
    let mut queue = VecDeque::new();

    for idx in 0..len {
        if water[idx] {
            dist[idx] = 0;
            queue.push_back(idx);
        }
    }

    while let Some(idx) = queue.pop_front() {
        let x = idx % width;
        let y = idx / width;
        let next_dist = dist[idx] + 1;
        for (dx, dy) in DIRS {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }
            let nidx = ny as usize * width + nx as usize;
            if next_dist < dist[nidx] {
                dist[nidx] = next_dist;
                queue.push_back(nidx);
            }
        }
    }

    dist
}

fn is_interior_land(dist_to_water: &[u32], idx: usize, buffer: u32) -> bool {
    dist_to_water[idx] >= buffer
}

pub fn compute_mountain_mask(map: &mut WorldMap, config: &WorldGenConfig) {
    let width = map.width;
    let height = map.height;
    let water_mask = map.water_mask.as_slice();
    let elevation = map.elevation.as_slice();
    let elev_threshold = config.mountain_elevation_threshold;
    let slope_threshold = config.mountain_slope_threshold;
    let cluster_threshold = config.mountain_cluster_threshold;
    let coast_buffer = config.mountain_coast_buffer;
    let dist_to_water = distance_to_water(width, height, water_mask);

    let mountain_noise = Fbm::<Perlin>::new(config.seed as u32 + 1)
        .set_octaves(3)
        .set_frequency(config.mountain_noise_frequency)
        .set_lacunarity(2.0)
        .set_persistence(0.5);

    let mut raw = vec![false; width * height];
    raw.par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * width + x;
                *cell = false;
                if water_mask[idx] {
                    continue;
                }
                if !is_interior_land(&dist_to_water, idx, coast_buffer) {
                    continue;
                }
                let high = elevation[idx] > elev_threshold;
                if !high {
                    continue;
                }
                let nx = x as f64 / width as f64;
                let ny = y as f64 / height as f64;
                let m = mountain_noise.get([nx, ny]) as f32;
                let cluster = ((m + 1.0) * 0.5) > cluster_threshold;
                let steep = interior_slope_at(width, height, elevation, water_mask, x, y)
                    > slope_threshold;
                *cell = steep && cluster;
            }
        });

    dilate_mountain_mask(&mut raw, width, height, water_mask, &dist_to_water, coast_buffer);
    map.mountain_mask.clone_from(&raw);
}

fn dilate_mountain_mask(
    mask: &mut [bool],
    width: usize,
    height: usize,
    water: &[bool],
    dist_to_water: &[u32],
    coast_buffer: u32,
) {
    let src = mask.to_vec();
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if water[idx] || src[idx] || !is_interior_land(dist_to_water, idx, coast_buffer) {
                continue;
            }
            for (dx, dy) in DIRS {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    continue;
                }
                let nidx = ny as usize * width + nx as usize;
                if src[nidx] {
                    mask[idx] = true;
                    break;
                }
            }
        }
    }
}

fn assign_land_biome(temp: f32, rain: f32, mountain: bool) -> Biome {
    if mountain {
        return Biome::Mountain;
    }

    if temp < 0.22 {
        return if rain < 0.35 {
            Biome::Tundra
        } else {
            Biome::Taiga
        };
    }
    if temp < 0.42 {
        return if rain < 0.30 {
            Biome::Grassland
        } else {
            Biome::Taiga
        };
    }
    if temp < 0.62 {
        return if rain < 0.3 {
            Biome::Grassland
        } else {
            Biome::TemperateForest
        };
    }
    if rain < 0.25 {
        Biome::Desert
    } else if rain < 0.5 {
        Biome::Savanna
    } else {
        Biome::TropicalForest
    }
}

pub fn generate_biomes(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    compute_mountain_mask(map, config);

    let width = map.width;
    let height = map.height;
    let water_mask = map.water_mask.as_slice();
    let temperature = map.temperature.as_slice();
    let rainfall = map.rainfall.as_slice();
    let mountain_mask = map.mountain_mask.as_slice();

    map.biome
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / height as f32,
                    "Assigning biomes",
                );
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * width + x;
                if water_mask[idx] {
                    continue;
                }
                *cell = assign_land_biome(
                    temperature[idx],
                    rainfall[idx],
                    mountain_mask[idx],
                );
            }
        });
}
