use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
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

fn is_interior_land(dist_to_water: &[u32], idx: usize, buffer: u32) -> bool {
    dist_to_water[idx] >= buffer
}

/// Peak orogeny within a small Chebyshev neighborhood (spreads tectonic belts).
fn local_orogeny_peak(
    orogeny: &[f32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    radius: i32,
) -> f32 {
    let mut peak = 0.0f32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }
            let nidx = ny as usize * width + nx as usize;
            peak = peak.max(orogeny[nidx]);
        }
    }
    peak
}

pub fn compute_mountain_mask(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
) {
    let width = map.width;
    let height = map.height;
    let water_mask = map.water_mask.as_slice();
    let elevation = map.elevation.as_slice();
    let orogeny = map.orogeny.as_slice();
    let elev_threshold = params.mountain_elev_norm;
    let slope_threshold = params.mountain_slope_norm;
    let orogeny_threshold = config.orogeny_mountain_threshold;
    let cluster_threshold = config.mountain_cluster_threshold;
    let coast_buffer = params.mountain_coast_buffer_cells;
    let use_orogeny = config.use_orogeny_mountains;
    let dist_to_water = map.dist_to_water.as_slice();

    let mountain_noise = Fbm::<Perlin>::new(config.seed as u32 + 1)
        .set_octaves(3)
        .set_frequency(params.mountain_noise_frequency)
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
                if use_orogeny {
                    let belt_orogeny = local_orogeny_peak(
                        orogeny,
                        width,
                        height,
                        x,
                        y,
                        params.orogeny_peak_radius_cells,
                    );
                    if belt_orogeny <= orogeny_threshold {
                        continue;
                    }
                    let elev_cutoff =
                        (elev_threshold - belt_orogeny * 0.12).clamp(0.42, elev_threshold);
                    if elevation[idx] <= elev_cutoff {
                        continue;
                    }
                    let steep =
                        interior_slope_at(width, height, elevation, water_mask, x, y)
                            > slope_threshold;
                    let strong_belt = belt_orogeny > orogeny_threshold * 1.8;
                    *cell = steep || strong_belt;
                } else {
                    if elevation[idx] <= elev_threshold {
                        continue;
                    }
                    let steep =
                        interior_slope_at(width, height, elevation, water_mask, x, y)
                            > slope_threshold;
                    if !steep {
                        continue;
                    }
                    let nx = x as f64 / width as f64;
                    let ny = y as f64 / height as f64;
                    let m = mountain_noise.get([nx, ny]) as f32;
                    let cluster = ((m + 1.0) * 0.5) > cluster_threshold;
                    *cell = cluster;
                }
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
                if src[nidx] && is_interior_land(dist_to_water, nidx, coast_buffer) {
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
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    compute_mountain_mask(map, config, params);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_world;

    #[test]
    fn orogeny_mountains_are_inland() {
        let config = WorldGenConfig {
            width: 512,
            height: 512,
            seed: 42,
            use_orogeny_mountains: true,
            ..Default::default()
        };
        let map = generate_world(&config);
        let coast_buffer = config.resolve().mountain_coast_buffer_cells as usize;
        let dist = &map.dist_to_water;

        let mut coastal = 0usize;
        let mut total = 0usize;
        for (idx, &is_mountain) in map.mountain_mask.iter().enumerate() {
            if !is_mountain {
                continue;
            }
            total += 1;
            if (dist[idx] as usize) < coast_buffer {
                coastal += 1;
            }
        }

        assert!(total > 0, "expected orogeny-driven mountain cells");
        let coastal_fraction = coastal as f32 / total as f32;
        assert!(
            coastal_fraction < 0.05,
            "expected mountains inland, but {coastal_fraction:.1}% are within coast buffer"
        );
    }
}
