use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
use super::progress::{report_stage, ProgressHandle};
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

fn classify_land_biome(
    temp: f32,
    rain: f32,
    elev: f32,
    slope: f32,
    ridge_influence: f32,
    params: &ResolvedSimParams,
) -> Biome {
    if elev >= params.mountain_elev_norm
        && slope >= params.mountain_slope_norm
        && ridge_influence >= params.mountain_min_ridge_influence
    {
        return Biome::Mountain;
    }

    if temp < 0.12 {
        return Biome::Ice;
    }
    if temp < 0.28 {
        if rain < 0.35 {
            return Biome::Tundra;
        }
        return Biome::Taiga;
    }
    if temp < 0.55 {
        if rain < 0.24 {
            return Biome::Desert;
        }
        if rain < 0.55 {
            return Biome::Grassland;
        }
        return Biome::TemperateForest;
    }
    if rain < 0.28 {
        return Biome::Desert;
    }
    if rain < 0.50 {
        return Biome::Savanna;
    }
    Biome::TropicalForest
}

/// Assign land biomes from temperature, rainfall, and elevation relief.
pub fn generate_biomes(
    map: &mut WorldMap,
    _config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;
    let elevation = map.elevation.as_slice();
    let temperature = map.temperature.as_slice();
    let rainfall = map.rainfall.as_slice();
    let water = map.water_mask.as_slice();
    let ridge_influence = map.ridge_influence.as_slice();

    map.biome
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / h as f32,
                    "Assigning biomes",
                );
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                if water[idx] {
                    continue;
                }
                let slope = interior_slope_at(w, h, elevation, water, x, y);
                *cell = classify_land_biome(
                    temperature[idx],
                    rainfall[idx],
                    elevation[idx],
                    slope,
                    ridge_influence[idx],
                    params,
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_world;

    #[test]
    fn default_config_produces_mountains() {
        let mut config = WorldGenConfig::default();
        config.seed = 42;
        config.width = 512;
        config.height = 512;
        let map = generate_world(&config);
        let mountains = map
            .biome_histogram()
            .get(&Biome::Mountain)
            .copied()
            .unwrap_or(0);
        assert!(
            mountains > 0,
            "expected Mountain biomes on default config, got {mountains}"
        );
    }

    #[test]
    fn mountains_require_ridge_influence() {
        let mut config = WorldGenConfig::default();
        config.seed = 42;
        config.width = 256;
        config.height = 256;
        config.mountain_min_ridge_influence = 1.0;
        let map = generate_world(&config);
        let mountains = map
            .biome_histogram()
            .get(&Biome::Mountain)
            .copied()
            .unwrap_or(0);
        assert_eq!(
            mountains, 0,
            "ridge threshold 1.0 should suppress all mountains"
        );
    }
}
