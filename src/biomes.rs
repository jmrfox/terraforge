use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::WorldGenConfig;
use super::progress::{ProgressHandle, report_stage};
use super::world::{Biome, WorldMap};

const DIRS: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
const MOUNTAIN_CLUSTER_THRESHOLD: f32 = 0.62;

fn local_slope_at(width: usize, height: usize, elevation: &[f32], x: usize, y: usize) -> f32 {
    let idx = y * width + x;
    let elev = elevation[idx];
    let mut max_drop = 0.0f32;

    for (dx, dy) in DIRS {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
            continue;
        }
        let nidx = ny as usize * width + nx as usize;
        let drop = (elev - elevation[nidx]).abs();
        max_drop = max_drop.max(drop);
    }
    max_drop
}

pub fn compute_mountain_mask(map: &mut WorldMap, config: &WorldGenConfig) {
    let width = map.width;
    let height = map.height;
    let water_mask = map.water_mask.as_slice();
    let elevation = map.elevation.as_slice();
    let elev_threshold = config.mountain_elevation_threshold;
    let slope_threshold = config.mountain_slope_threshold;

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
                let high = elevation[idx] > elev_threshold;
                if !high {
                    continue;
                }
                let wx = x as f64;
                let hy = y as f64;
                let m = mountain_noise.get([wx, hy]) as f32;
                let cluster = ((m + 1.0) * 0.5) > MOUNTAIN_CLUSTER_THRESHOLD;
                let steep =
                    local_slope_at(width, height, elevation, x, y) > slope_threshold;
                *cell = cluster || steep;
            }
        });

    dilate_mountain_mask(&mut raw, width, height, water_mask);
    map.mountain_mask.clone_from(&raw);
}

fn dilate_mountain_mask(mask: &mut [bool], width: usize, height: usize, water: &[bool]) {
    let src = mask.to_vec();
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if water[idx] || src[idx] {
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
