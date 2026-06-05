use std::collections::HashMap;

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

    pub elevation: Vec<f32>,
    pub temperature: Vec<f32>,
    pub rainfall: Vec<f32>,

    pub water_mask: Vec<bool>,
    pub river_mask: Vec<bool>,
    pub mountain_mask: Vec<bool>,

    pub biome: Vec<Biome>,
    pub plate_id: Vec<u32>,
}

impl WorldMap {
    pub fn new(width: usize, height: usize, seed: u64) -> Self {
        let len = width * height;
        Self {
            width,
            height,
            seed,
            elevation: vec![0.0; len],
            temperature: vec![0.0; len],
            rainfall: vec![0.0; len],
            water_mask: vec![false; len],
            river_mask: vec![false; len],
            mountain_mask: vec![false; len],
            biome: vec![Biome::Grassland; len],
            plate_id: vec![0; len],
        }
    }

    #[inline]
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    pub fn biome_histogram(&self) -> HashMap<Biome, usize> {
        let mut counts = HashMap::new();
        for &b in &self.biome {
            *counts.entry(b).or_insert(0) += 1;
        }
        counts
    }
}
