//! Biome classification from physical temperature and rainfall.

use rayon::prelude::*;

use crate::config::ResolvedSimParams;
use crate::grid_ops::box_blur;
use crate::world::{Biome, WorldMap};

/// Gradient magnitude per cell. Smoothed terrain keeps single noisy cells from punching
/// holes in mountain ranges.
fn slope_at(smoothed: &[f32], width: usize, height: usize, x: usize, y: usize) -> f32 {
    let (left, right) = (x.saturating_sub(1), (x + 1).min(width - 1));
    let (up, down) = (y.saturating_sub(1), (y + 1).min(height - 1));
    let dx =
        (smoothed[y * width + right] - smoothed[y * width + left]) / (right - left).max(1) as f32;
    let dy = (smoothed[down * width + x] - smoothed[up * width + x]) / (down - up).max(1) as f32;
    (dx * dx + dy * dy).sqrt()
}

/// Classification from mean annual °C and annual rainfall in mm.
///
/// Aridity uses the Köppen desert limit, which rises with temperature because warm air
/// evaporates more water. That keeps the boundary continuous across temperature bands.
fn classify_climate(temperature_c: f32, rainfall_mm: f32) -> Biome {
    let desert_limit_mm = 10.0 * temperature_c.max(0.0) + 140.0;
    if temperature_c < -10.0 {
        Biome::Ice
    } else if temperature_c < -4.0 {
        Biome::Tundra
    } else if temperature_c < 4.0 {
        if rainfall_mm < desert_limit_mm {
            Biome::Tundra
        } else {
            Biome::Taiga
        }
    } else if rainfall_mm < desert_limit_mm {
        Biome::Desert
    } else if temperature_c < 20.0 {
        if rainfall_mm < 2.0 * desert_limit_mm {
            Biome::Grassland
        } else {
            Biome::TemperateForest
        }
    } else if rainfall_mm < 1100.0 {
        Biome::Savanna
    } else {
        Biome::TropicalForest
    }
}

pub fn generate(map: &mut WorldMap, params: &ResolvedSimParams) {
    let (w, h) = (map.width, map.height);
    let elevation = map.elevation.as_slice();
    let water = map.water_mask.as_slice();
    let ridge = map.ridge_influence.as_slice();
    let temperature = map.temperature_c.as_slice();
    let rainfall = map.rainfall_mm.as_slice();
    let smoothed = box_blur(elevation, w, h, params.mountain_smoothing_cells);

    map.biome
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                if water[idx] {
                    continue;
                }
                // Summits are locally flat, so higher ground needs less slope to count.
                let above_minimum = elevation[idx] - params.mountain_elev_norm;
                let slope_needed = params.mountain_slope_norm
                    * (1.0 - (above_minimum / params.mountain_slope_fade_norm).clamp(0.0, 1.0));
                let is_mountain = above_minimum >= 0.0
                    && ridge[idx] >= params.mountain_min_ridge_influence
                    && slope_at(&smoothed, w, h, x, y) >= slope_needed;
                *cell = if is_mountain {
                    Biome::Mountain
                } else {
                    classify_climate(temperature[idx], rainfall[idx])
                };
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generate_world, WorldGenConfig};

    #[test]
    fn classifies_familiar_climates() {
        assert_eq!(classify_climate(-20.0, 200.0), Biome::Ice);
        assert_eq!(classify_climate(-6.0, 300.0), Biome::Tundra);
        assert_eq!(classify_climate(1.0, 500.0), Biome::Taiga);
        assert_eq!(classify_climate(12.0, 150.0), Biome::Desert);
        assert_eq!(classify_climate(12.0, 500.0), Biome::Grassland);
        assert_eq!(classify_climate(12.0, 1200.0), Biome::TemperateForest);
        assert_eq!(classify_climate(27.0, 100.0), Biome::Desert);
        assert_eq!(classify_climate(27.0, 800.0), Biome::Savanna);
        assert_eq!(classify_climate(27.0, 2200.0), Biome::TropicalForest);
    }

    #[test]
    fn default_config_produces_mountains() {
        let map = generate_world(&WorldGenConfig::default());
        let mountains = map
            .biome_histogram()
            .get(&Biome::Mountain)
            .copied()
            .unwrap_or(0);
        assert!(mountains > 0, "expected Mountain biomes on default config");
    }

    fn mountain_count(config: &WorldGenConfig) -> usize {
        generate_world(config)
            .biome
            .iter()
            .filter(|b| **b == Biome::Mountain)
            .count()
    }

    #[test]
    fn slope_and_ridge_controls_affect_default_maps() {
        let base = WorldGenConfig::default();
        let count = mountain_count(&base);
        let no_slope = mountain_count(&WorldGenConfig {
            mountain_min_slope_deg: crate::Degrees(0.0),
            ..base.clone()
        });
        let no_ridge = mountain_count(&WorldGenConfig {
            mountain_min_ridge_influence: 0.0,
            ..base
        });
        assert!(no_slope > count, "slope gate has no effect");
        assert!(no_ridge > count, "ridge gate has no effect");
    }

    #[test]
    fn high_flat_summits_are_mountains_but_low_flats_are_not() {
        let config = WorldGenConfig {
            width: 16,
            height: 16,
            ..WorldGenConfig::default()
        };
        let params = config.resolve();
        let classify = |elevation_m: f64| {
            let mut map = WorldMap::from_config(&config);
            let norm = (elevation_m - params.ocean_floor_m)
                / (params.max_elevation_m - params.ocean_floor_m);
            map.elevation.fill(norm as f32);
            map.ridge_influence.fill(0.8);
            map.temperature_c.fill(5.0);
            map.rainfall_mm.fill(800.0);
            generate(&mut map, &params);
            map.biome[map.index(8, 8)]
        };
        assert_ne!(classify(2500.0), Biome::Mountain, "flat plateau");
        assert_eq!(classify(3500.0), Biome::Mountain, "high summit");
    }

    #[test]
    fn mountains_require_ridge_influence() {
        let config = WorldGenConfig {
            width: 256,
            height: 256,
            mountain_min_ridge_influence: 1.0,
            ..WorldGenConfig::default()
        };
        let map = generate_world(&config);
        assert!(!map.biome.contains(&Biome::Mountain));
    }
}
