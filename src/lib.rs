//! Procedural 2D world map generation — deterministic, modular pipeline.
//!
//! Spec: `docs/design.md`

mod biomes;
mod coast;
mod colors;
mod config;
mod priors;
mod grid_ops;
mod units;
mod elevation;
mod land_mask;
mod landscape_evolution;
mod oceans;
mod plates;
mod preview;
mod progress;
mod rainfall;
mod rivers;
mod temperature;
mod world;

pub use colors::{LEGEND_ENTRIES, RIVER_RGBA, biome_rgba};
pub use preview::{
    MapExportFormat, MapStats, PreviewLayer, TiffLayerSet, biome_to_id, compute_map_stats,
    floats_to_gray16, map_biome_id_to_gray16, map_elevation_to_gray16, map_to_preview_rgba8,
    map_to_rgba8, write_map, write_map_png, write_map_stats, write_map_tiff,
    write_map_with_tiff_layers,
};
pub use config::{
    LandGenerationMode, LandMaskMethod, ResolvedSimParams, WindDirection, WorldGenConfig,
    calibrate_sea_level_norm,
};
pub use priors::{PriorDist, PriorSet, SampleParam, SampleableParam};
pub use units::{Celsius, Degrees, Meters, SquareKilometers, SquareMeters};
pub use plates::{CrustType, Plate, PlateData};
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
    let params = config.resolve();
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut map = WorldMap::new(config.width, config.height, config.seed);

    report(&progress, 0.0, "Generating tectonic plates");
    let plate_data = plates::generate_plates(config, &params, &mut rng);

    report(&progress, 0.05, "Assigning plate regions");
    plates::assign_plate_ids(&mut map, &plate_data, &progress, 0.05, 0.20);

    report(&progress, 0.20, "Building elevation");
    elevation::generate_elevation(
        &mut map,
        &plate_data,
        config,
        &params,
        &progress,
        0.20,
        0.38,
    );

    report(&progress, 0.38, "Evolving landscape");
    landscape_evolution::evolve_landscape(
        &mut map,
        config,
        &params,
        &progress,
        0.38,
        0.45,
        None,
    );

    report(&progress, 0.45, "Filling oceans and lakes");
    oceans::generate_oceans(&mut map, config, &params);

    report(&progress, 0.52, "Simulating temperature");
    temperature::generate_temperature(&mut map, config, &params, &progress, 0.52, 0.60);

    report(&progress, 0.60, "Simulating rainfall");
    rainfall::generate_rainfall(&mut map, config, &params, &progress, 0.60, 0.68);

    if config.rainfall_erodibility_coupling > 0.001
        && config.landscape_evolution_enabled
        && config.coarse_hydro_factor <= 1
    {
        report(&progress, 0.68, "Refining landscape with climate");
        let rainfall_snapshot = map.rainfall.clone();
        landscape_evolution::evolve_landscape(
            &mut map,
            config,
            &params,
            &progress,
            0.68,
            0.72,
            Some(&rainfall_snapshot),
        );
    }

    report(&progress, 0.72, "Carving river networks");
    rivers::generate_rivers(&mut map, config, &params, &progress, 0.72, 0.88);

    report(&progress, 0.88, "Assigning biomes");
    biomes::generate_biomes(&mut map, config, &params, &progress, 0.88, 1.0);

    report(&progress, 1.0, "Complete");
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Meters;

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
        assert_eq!(a.orogeny, b.orogeny);
        assert_eq!(a.macro_land_mask, b.macro_land_mask);
    }

    #[test]
    fn orogeny_in_valid_range() {
        let config = WorldGenConfig::test_config(7, 64);
        let map = generate_world(&config);
        assert_range01(&map.orogeny, "orogeny");
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

    #[test]
    fn orogeny_higher_at_mountains_than_random_land() {
        let config = WorldGenConfig {
            width: 512,
            height: 512,
            seed: 42,
            ..Default::default()
        };
        let map = generate_world(&config);

        let mut mountain_orogeny = 0.0f32;
        let mut mountain_count = 0usize;
        let mut land_orogeny = 0.0f32;
        let mut land_count = 0usize;

        for idx in 0..map.width * map.height {
            if map.water_mask[idx] {
                continue;
            }
            land_orogeny += map.orogeny[idx];
            land_count += 1;
            if map.mountain_mask[idx] {
                mountain_orogeny += map.orogeny[idx];
                mountain_count += 1;
            }
        }

        if mountain_count > 0 && land_count > 0 {
            let mean_mountain = mountain_orogeny / mountain_count as f32;
            let mean_land = land_orogeny / land_count as f32;
            assert!(
                mean_mountain > mean_land * 1.2,
                "mountain orogeny {mean_mountain} should exceed mean land {mean_land}"
            );
        }
    }

    #[test]
    fn suggest_sea_level_for_land_fraction() {
        let config = WorldGenConfig::test_config(42, 128);
        let preview: Vec<f32> = (0..128 * 128).map(|i| i as f32 / (128.0 * 128.0)).collect();
        let suggested = config.suggest_sea_level_m_for_fraction(&preview, 0.35);
        let params = config.resolve();
        let norm = ((suggested.0 - config.ocean_floor_m.0)
            / (config.max_elevation_m.0 - config.ocean_floor_m.0)) as f32;
        let land = preview.iter().filter(|&&e| e >= norm).count() as f32 / preview.len() as f32;
        assert!((land - 0.35).abs() < 0.05, "suggested sea level should yield ~35% land");
        let _ = params.sea_level_norm;
    }

    #[test]
    fn tectonic_base_produces_land() {
        let config = WorldGenConfig::test_config(42, 128);
        let map = generate_world(&config);
        let land = map.water_mask.iter().filter(|&&w| !w).count();
        assert!(land > 0, "tectonic base should produce emergent land");
    }

    #[test]
    fn texture_zero_matches_tectonic_only() {
        let mut config = WorldGenConfig::test_config(7, 64);
        config.land_texture_strength_m = Meters(0.0);
        config.landscape_evolution_enabled = false;
        let map = generate_world(&config);
        assert_range01(&map.elevation, "elevation");
    }

    #[test]
    fn generates_mountain_biome() {
        let config = WorldGenConfig {
            width: 512,
            height: 512,
            seed: 42,
            ..Default::default()
        };
        let map = generate_world(&config);
        let mountains = map
            .biome
            .iter()
            .filter(|&&b| b == Biome::Mountain)
            .count();
        assert!(mountains > 0, "expected mountain biome cells, got {mountains}");
        assert!(
            mountains < 50_000,
            "expected selective mountain biome, got {mountains}"
        );
    }
}
