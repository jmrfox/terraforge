use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use rayon::prelude::*;
use serde::Serialize;

use super::colors::{RIVER_RGBA, biome_rgba};
use super::config::WorldGenConfig;
use super::world::{Biome, WorldMap};

/// Summary statistics for a generated map (written as JSON sidecar).
#[derive(Debug, Clone, Serialize)]
pub struct MapStats {
    pub config: WorldGenConfig,
    pub width: usize,
    pub height: usize,
    pub land_fraction: f64,
    pub ocean_fraction: f64,
    pub biomes: HashMap<String, usize>,
    pub elapsed_ms: u64,
}

/// Rasterize a world map to RGBA8 pixels (one pixel per cell).
pub fn map_to_rgba8(map: &WorldMap) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];

    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let mut color = biome_rgba(map.biome[idx]);
            if map.river_mask[idx] {
                color = RIVER_RGBA;
            }
            px.copy_from_slice(&color);
        });

    pixels
}

/// Write a biome-colored PNG preview of the map.
pub fn write_map_png(map: &WorldMap, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let pixels = map_to_rgba8(map);
    let image = image::RgbaImage::from_raw(map.width as u32, map.height as u32, pixels)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid image dimensions"))?;
    image.save(path).map_err(io::Error::other)
}

/// Compute land/ocean fractions and biome histogram for stats export.
pub fn compute_map_stats(map: &WorldMap, config: &WorldGenConfig, elapsed_ms: u64) -> MapStats {
    let total = map.width * map.height;
    let land_cells = map.water_mask.iter().filter(|&&w| !w).count();
    let ocean_cells = map.biome.iter().filter(|&&b| b == Biome::Ocean).count();

    let mut biomes = HashMap::new();
    for &biome in &map.biome {
        *biomes.entry(biome_label(biome).to_string()).or_insert(0) += 1;
    }

    MapStats {
        config: config.clone(),
        width: map.width,
        height: map.height,
        land_fraction: land_cells as f64 / total as f64,
        ocean_fraction: ocean_cells as f64 / total as f64,
        biomes,
        elapsed_ms,
    }
}

/// Write stats JSON to disk.
pub fn write_map_stats(stats: &MapStats, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string_pretty(stats).map_err(io::Error::other)?;
    fs::write(path, json)
}

fn biome_label(biome: Biome) -> &'static str {
    match biome {
        Biome::Ocean => "Ocean",
        Biome::Lake => "Lake",
        Biome::Ice => "Ice",
        Biome::Tundra => "Tundra",
        Biome::Taiga => "Taiga",
        Biome::Grassland => "Grassland",
        Biome::TemperateForest => "TemperateForest",
        Biome::Desert => "Desert",
        Biome::Savanna => "Savanna",
        Biome::TropicalForest => "TropicalForest",
        Biome::Mountain => "Mountain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorldGenConfig, generate_world};

    #[test]
    fn map_to_rgba8_correct_length() {
        let config = WorldGenConfig::test_config(1, 64);
        let map = generate_world(&config);
        let pixels = map_to_rgba8(&map);
        assert_eq!(pixels.len(), 64 * 64 * 4);
    }

    #[test]
    fn write_map_png_produces_valid_file() {
        let config = WorldGenConfig::test_config(2, 32);
        let map = generate_world(&config);
        let path = std::env::temp_dir().join("caravanserai_mapgen_test.png");
        write_map_png(&map, &path).expect("write png");
        let bytes = fs::read(&path).expect("read png");
        assert!(bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        let _ = fs::remove_file(path);
    }
}
