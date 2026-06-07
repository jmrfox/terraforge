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
    /// Chamfer distance to nearest water cell (populated after oceans).
    pub dist_to_water: Vec<u32>,
    pub river_mask: Vec<bool>,
    pub mountain_mask: Vec<bool>,
    /// Tectonic orogeny intensity from plate boundary uplift, normalized 0–1.
    pub orogeny: Vec<f32>,
    /// Blurred continental-crust macro mask in `[0, 1]` (shelf + inland gating).
    pub macro_land_mask: Vec<f32>,
    /// Cached steepest-descent neighbor from landscape evolution (for rivers).
    pub flow_downslope: Vec<Option<usize>>,
    /// Cached flow accumulation from landscape evolution.
    pub flow_accumulation: Vec<f32>,

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
            dist_to_water: vec![u32::MAX; len],
            river_mask: vec![false; len],
            mountain_mask: vec![false; len],
            orogeny: vec![0.0; len],
            macro_land_mask: vec![0.0; len],
            flow_downslope: vec![None; len],
            flow_accumulation: vec![0.0; len],
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
