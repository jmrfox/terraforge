use std::collections::HashMap;

use crate::config::WorldGenConfig;

/// Terrain / climate classification for a map cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Biome {
    Ocean,
    Lake,
    Ice,
    Tundra,
    Taiga,
    Grassland,
    TemperateForest,
    Desert,
    Savanna,
    TropicalForest,
    Mountain,
}

/// Flattened 2D world map produced by the generation pipeline.
#[derive(Debug, Clone)]
pub struct WorldMap {
    pub width: usize,
    pub height: usize,
    pub seed: u64,

    /// Generator algorithm that produced this map.
    pub generator_version: u32,
    /// Width and height of one grid cell in meters.
    pub cell_size_m: f64,
    /// Elevation in meters at normalized elevation 0.
    pub ocean_floor_m: f64,
    /// Elevation in meters at normalized elevation 1.
    pub max_elevation_m: f64,
    pub sea_level_m: f64,

    pub elevation: Vec<f32>,
    /// Ridge noise × ridge envelope; populated during elevation generation.
    pub ridge_influence: Vec<f32>,
    pub temperature: Vec<f32>,
    pub rainfall: Vec<f32>,

    pub water_mask: Vec<bool>,
    /// Manhattan distance in cells to the nearest cell below sea level, measured before
    /// small ponds are filled (populated during water classification).
    pub dist_to_water: Vec<u32>,

    pub biome: Vec<Biome>,
}

impl WorldMap {
    /// Empty map with the default physical scale. Prefer [`WorldMap::from_config`].
    pub fn new(width: usize, height: usize, seed: u64) -> Self {
        let defaults = WorldGenConfig::default();
        let len = width * height;
        Self {
            width,
            height,
            seed,
            generator_version: defaults.generator_version,
            cell_size_m: defaults.cell_size_m.0,
            ocean_floor_m: defaults.ocean_floor_m.0,
            max_elevation_m: defaults.max_elevation_m.0,
            sea_level_m: defaults.sea_level_m.0,
            elevation: vec![0.0; len],
            ridge_influence: vec![0.0; len],
            temperature: vec![0.0; len],
            rainfall: vec![0.0; len],
            water_mask: vec![false; len],
            dist_to_water: vec![u32::MAX; len],
            biome: vec![Biome::Grassland; len],
        }
    }

    /// Empty map carrying the physical scale of `config`.
    pub fn from_config(config: &WorldGenConfig) -> Self {
        Self {
            generator_version: config.generator_version,
            cell_size_m: config.cell_size_m.0,
            ocean_floor_m: config.ocean_floor_m.0,
            max_elevation_m: config.max_elevation_m.0,
            sea_level_m: config.sea_level_m.0,
            ..Self::new(config.width, config.height, config.seed)
        }
    }

    #[inline]
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    /// Map width in meters.
    pub fn width_m(&self) -> f64 {
        self.width as f64 * self.cell_size_m
    }

    /// Map height in meters.
    pub fn height_m(&self) -> f64 {
        self.height as f64 * self.cell_size_m
    }

    /// Elevation of cell `idx` in meters.
    pub fn elevation_m(&self, idx: usize) -> f64 {
        let span = (self.max_elevation_m - self.ocean_floor_m).max(1.0);
        self.ocean_floor_m + f64::from(self.elevation[idx]) * span
    }

    pub fn biome_histogram(&self) -> HashMap<Biome, usize> {
        let mut counts = HashMap::new();
        for &b in &self.biome {
            *counts.entry(b).or_insert(0) += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Meters;

    #[test]
    fn from_config_carries_physical_scale() {
        let config = WorldGenConfig {
            width: 10,
            height: 5,
            ..WorldGenConfig::default().rescaled_to_cell_size(Meters(400.0))
        };
        let mut map = WorldMap::from_config(&config);
        assert_eq!(map.cell_size_m, 400.0);
        assert_eq!(map.width_m(), 4000.0);
        assert_eq!(map.height_m(), 2000.0);

        map.elevation[3] = 0.4;
        assert!(map.elevation_m(3).abs() < 0.01, "0.4 is sea level");
        map.elevation[3] = 1.0;
        assert_eq!(map.elevation_m(3), config.max_elevation_m.0);
        assert_eq!(config.ocean_floor_m, Meters(-6000.0));
    }
}
