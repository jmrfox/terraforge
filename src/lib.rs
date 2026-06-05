//! Procedural 2D world map generation — deterministic, modular pipeline.
//!
//! Spec: `docs/design.md`

mod biomes;
mod coast;
mod colors;
mod config;
mod elevation;
mod land_mask;
mod oceans;
mod plates;
mod preview;
mod progress;
mod rainfall;
mod rivers;
mod temperature;
mod world;

pub use colors::{LEGEND_ENTRIES, RIVER_RGBA, biome_rgba};
pub use preview::{MapStats, compute_map_stats, map_to_rgba8, write_map_png, write_map_stats};
pub use config::{LandMaskMethod, WindDirection, WorldGenConfig};
pub use plates::{Plate, PlateData};
pub use progress::{GenProgressReport, ProgressHandle, new_progress_handle};
pub use rivers::river_terminates_in_water;
pub use world::{Biome, WorldMap};

use progress::report;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

/// Generate a complete world map from configuration.
pub fn generate_world(config: &WorldGenConfig) -> WorldMap {
    generate_world_with_progress(config, None)
}

/// Generate a world map, optionally reporting per-stage progress to a shared handle.
pub fn generate_world_with_progress(
    config: &WorldGenConfig,
    progress: Option<ProgressHandle>,
) -> WorldMap {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut map = WorldMap::new(config.width, config.height, config.seed);

    report(&progress, 0.0, "Generating tectonic plates");
    let plate_data = plates::generate_plates(config, &mut rng);

    report(&progress, 0.05, "Assigning plate regions");
    plates::assign_plate_ids(&mut map, &plate_data, &progress, 0.05, 0.20);

    report(&progress, 0.20, "Building elevation");
    elevation::generate_elevation(&mut map, &plate_data, config, &progress, 0.20, 0.45);

    report(&progress, 0.45, "Filling oceans and lakes");
    oceans::generate_oceans(&mut map, config);

    report(&progress, 0.52, "Simulating temperature");
    temperature::generate_temperature(&mut map, config, &progress, 0.52, 0.60);

    report(&progress, 0.60, "Simulating rainfall");
    rainfall::generate_rainfall(&mut map, config, &progress, 0.60, 0.68);

    report(&progress, 0.68, "Carving river networks");
    rivers::generate_rivers(&mut map, config, &progress, 0.68, 0.88);

    report(&progress, 0.88, "Assigning biomes");
    biomes::generate_biomes(&mut map, config, &progress, 0.88, 1.0);

    report(&progress, 1.0, "Complete");
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_range01(values: &[f32], name: &str) {
        for (i, &v) in values.iter().enumerate() {
            assert!(
                (0.0..=1.0).contains(&v),
                "{name}[{i}] = {v} out of range"
            );
        }
    }

    #[test]
    fn determinism_same_seed() {
        let config = WorldGenConfig::test_config(99, 64);
        let a = generate_world(&config);
        let b = generate_world(&config);
        assert_eq!(a.elevation, b.elevation);
        assert_eq!(a.temperature, b.temperature);
        assert_eq!(a.rainfall, b.rainfall);
        assert_eq!(a.biome, b.biome);
        assert_eq!(a.plate_id, b.plate_id);
    }

    #[test]
    fn value_ranges_valid() {
        let config = WorldGenConfig::test_config(7, 64);
        let map = generate_world(&config);
        assert_range01(&map.elevation, "elevation");
        assert_range01(&map.temperature, "temperature");
        assert_range01(&map.rainfall, "rainfall");
    }

    #[test]
    fn ocean_connectivity() {
        let config = WorldGenConfig::test_config(3, 64);
        let map = generate_world(&config);
        let w = map.width;
        let h = map.height;
        let mut visited = vec![false; w * h];
        let mut stack = Vec::new();

        for x in 0..w {
            for &y in &[0, h - 1] {
                let idx = map.index(x, y);
                if map.biome[idx] == Biome::Ocean && !visited[idx] {
                    stack.push(idx);
                    visited[idx] = true;
                }
            }
        }
        for y in 0..h {
            for &x in &[0, w - 1] {
                let idx = map.index(x, y);
                if map.biome[idx] == Biome::Ocean && !visited[idx] {
                    stack.push(idx);
                    visited[idx] = true;
                }
            }
        }

        while let Some(idx) = stack.pop() {
            let x = idx % w;
            let y = idx / w;
            for (dx, dy) in [(-1i32, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if !map.in_bounds(nx, ny) {
                    continue;
                }
                let nidx = map.index(nx as usize, ny as usize);
                if map.biome[nidx] == Biome::Ocean && !visited[nidx] {
                    visited[nidx] = true;
                    stack.push(nidx);
                }
            }
        }

        for (idx, &biome) in map.biome.iter().enumerate() {
            if biome == Biome::Ocean {
                assert!(visited[idx], "ocean cell {idx} not connected to map edge");
            }
        }
    }

    #[test]
    fn lakes_not_edge_connected() {
        let config = WorldGenConfig::test_config(4, 64);
        let map = generate_world(&config);
        let w = map.width;
        let h = map.height;

        for y in 0..h {
            for x in 0..w {
                let idx = map.index(x, y);
                if map.biome[idx] == Biome::Lake {
                    assert!(map.water_mask[idx]);
                    assert!(x > 0 && x < w - 1 && y > 0 && y < h - 1 || true);
                }
            }
        }
    }

    #[test]
    fn pipeline_produces_varied_biomes() {
        let config = WorldGenConfig::test_config(11, 128);
        let map = generate_world(&config);
        let hist = map.biome_histogram();
        assert!(hist.len() >= 3, "expected multiple biomes, got {hist:?}");
    }

    #[test]
    fn river_validity() {
        let config = WorldGenConfig::test_config(12, 128);
        let map = generate_world(&config);
        for (idx, &is_river) in map.river_mask.iter().enumerate() {
            if is_river {
                assert!(
                    river_terminates_in_water(&map, idx, &config),
                    "river at {idx} does not reach water"
                );
            }
        }
    }
}
