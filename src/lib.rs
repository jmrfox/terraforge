//! Procedural 2D world map generation — deterministic, modular pipeline.
//!
//! Pipeline: elevation → temperature → rainfall → water → biomes

mod biomes;
mod colors;
mod config;
mod elevation;
mod grid_ops;
mod preview;
mod priors;
mod progress;
mod rainfall;
mod temperature;
mod units;
mod water;
mod world;

pub use colors::{biome_rgba, LEGEND_ENTRIES};
pub use config::{
    calibrate_sea_level_norm, ElevationEnvelopeConfig, ResolvedSimParams, WorldGenConfig,
};
pub use preview::{
    biome_to_id, compute_map_stats, floats_to_gray16, map_biome_id_to_gray16,
    map_elevation_to_gray16, map_to_preview_rgba8, map_to_rgba8, write_map_png, write_map_stats,
    write_map_tiff, write_map_with_tiff_layers, MapExportFormat, MapStats, PreviewLayer,
    TiffLayerSet,
};
pub use priors::{
    sample_default_config, sample_parameters, PriorDist, PriorSet, SampleParam, SampleableParam,
};
pub use progress::{new_progress_handle, GenProgressReport, ProgressHandle};
pub use units::{Celsius, Degrees, Meters, SquareKilometers, SquareMeters};
pub use world::{Biome, WorldMap};

use progress::report;

/// Generate a complete world map from configuration.
pub fn generate_world(config: &WorldGenConfig) -> WorldMap {
    generate_world_with_progress(config, None)
}

/// Generate a world map, optionally reporting per-stage progress to a shared handle.
pub fn generate_world_with_progress(
    config: &WorldGenConfig,
    progress: Option<ProgressHandle>,
) -> WorldMap {
    let params = config.resolve();
    let mut map = WorldMap::new(config.width, config.height, config.seed);

    report(&progress, 0.0, "Generating elevation");
    elevation::generate_elevation(&mut map, config, &params, &progress, 0.0, 0.30);

    report(&progress, 0.30, "Simulating temperature");
    temperature::generate_temperature(&mut map, config, &params, &progress, 0.30, 0.50);

    report(&progress, 0.50, "Simulating rainfall");
    rainfall::generate_rainfall(&mut map, config, &params, &progress, 0.50, 0.70);

    water::generate_water(&mut map, config, &params, &progress, 0.70, 0.85);

    report(&progress, 0.85, "Assigning biomes");
    biomes::generate_biomes(&mut map, config, &params, &progress, 0.85, 1.0);

    report(&progress, 1.0, "Complete");
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_range01(values: &[f32], name: &str) {
        for (i, &v) in values.iter().enumerate() {
            assert!((0.0..=1.0).contains(&v), "{name}[{i}] = {v} out of range");
        }
    }

    #[test]
    fn determinism_same_seed() {
        let config = WorldGenConfig::test_config(99, 64);
        let a = generate_world(&config);
        let b = generate_world(&config);
        assert_eq!(a.elevation, b.elevation);
        assert_eq!(a.ridge_influence, b.ridge_influence);
        assert_eq!(a.temperature, b.temperature);
        assert_eq!(a.rainfall, b.rainfall);
        assert_eq!(a.biome, b.biome);
        assert_eq!(a.water_mask, b.water_mask);
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
    fn pipeline_produces_land_and_water() {
        let config = WorldGenConfig::test_config(42, 128);
        let map = generate_world(&config);
        let land = map.water_mask.iter().filter(|&&w| !w).count();
        let water = map.water_mask.iter().filter(|&&w| w).count();
        assert!(land > 0, "expected emergent land");
        assert!(water > 0, "expected water bodies");
    }

    #[test]
    fn target_land_fraction_near_configured() {
        let config = WorldGenConfig::test_config(42, 128);
        let map = generate_world(&config);
        let target = config.target_land_fraction.unwrap();
        let sea = config.resolve().sea_level_norm;
        let land =
            map.elevation.iter().filter(|&&e| e >= sea).count() as f32 / map.elevation.len() as f32;
        assert!(
            (land - target).abs() < 0.03,
            "land {land} should be near target {target}"
        );
    }

    #[test]
    fn edge_cells_tend_to_be_water() {
        let config = WorldGenConfig::test_config(5, 64);
        let map = generate_world(&config);
        let w = map.width;
        let h = map.height;
        let mut edge_water = 0usize;
        let mut edge_total = 0usize;
        for x in 0..w {
            for &y in &[0, h - 1] {
                let idx = map.index(x, y);
                edge_total += 1;
                if map.water_mask[idx] {
                    edge_water += 1;
                }
            }
        }
        for y in 0..h {
            for &x in &[0, w - 1] {
                let idx = map.index(x, y);
                edge_total += 1;
                if map.water_mask[idx] {
                    edge_water += 1;
                }
            }
        }
        let edge_water_frac = edge_water as f64 / edge_total as f64;
        assert!(
            edge_water_frac > 0.5,
            "expected majority edge water, got {edge_water_frac}"
        );
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
    fn pipeline_produces_varied_biomes() {
        let config = WorldGenConfig::test_config(11, 128);
        let map = generate_world(&config);
        let hist = map.biome_histogram();
        assert!(hist.len() >= 3, "expected multiple biomes, got {hist:?}");
    }

    #[test]
    fn suggest_sea_level_for_land_fraction() {
        let config = WorldGenConfig::test_config(42, 128);
        let preview: Vec<f32> = (0..128 * 128).map(|i| i as f32 / (128.0 * 128.0)).collect();
        let suggested = config.suggest_sea_level_m_for_fraction(&preview, 0.35);
        let norm = ((suggested.0 - config.ocean_floor_m.0)
            / (config.max_elevation_m.0 - config.ocean_floor_m.0)) as f32;
        let land = preview.iter().filter(|&&e| e >= norm).count() as f32 / preview.len() as f32;
        assert!(
            (land - 0.35).abs() < 0.05,
            "suggested sea level should yield ~35% land"
        );
    }
}
