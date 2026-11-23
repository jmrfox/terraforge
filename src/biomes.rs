//! Biome classification from physical temperature and rainfall.

use rayon::prelude::*;

use crate::config::ResolvedSimParams;
use crate::world::{Biome, WorldMap};

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

    map.biome
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                if water[idx] {
                    continue;
                }
                let is_mountain = elevation[idx] >= params.mountain_elev_norm
                    && ridge[idx] >= params.mountain_min_ridge_influence
                    && interior_slope_at(w, h, elevation, water, x, y)
                        >= params.mountain_slope_norm;
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
