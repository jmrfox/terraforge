//! Procedural 2D world map generation — deterministic, modular pipeline.
//!
//! Pipeline: elevation → sea and coast → climate → hydrology → biomes. Coasts come before
//! climate so wind and humidity see them, and hydrology follows because rivers need rain.

mod biomes;
mod climate;
mod colors;
mod config;
mod controls;
mod elevation;
mod grid_ops;
mod hydrology;
mod map_file;
mod preview;
mod priors;
mod progress;
mod seeds;
#[cfg(test)]
mod tests;
mod units;
mod water;
mod world;

pub use climate::{RAINFALL_NORM_SCALE_MM, TEMPERATURE_NORM_MAX_C, TEMPERATURE_NORM_MIN_C};
pub use colors::{biome_rgba, LEGEND_ENTRIES};
pub use config::{
    calibrate_sea_level_norm, ConfigError, ElevationEnvelopeConfig, HydrologyConfig,
    RegionalClimateConfig, ResolvedSimParams, WorldGenConfig, GENERATOR_VERSION,
};
pub use controls::WorldControl;
pub use map_file::{
    load_map_file, read_stored_map, save_map_file, write_stored_map, MapFileError, StoredMap,
    MAP_FILE_EXTENSION, MAP_FILE_VERSION,
};
pub use preview::{
    biome_from_id, biome_to_id, compute_map_stats, floats_to_gray16, map_biome_id_to_gray16,
    map_elevation_to_gray16, map_to_preview_rgba8, map_to_rgba8, write_map_png, write_map_stats,
    write_map_tiff, write_map_with_tiff_layers, MapExportFormat, MapStats, PreviewLayer,
    TiffLayerSet,
};
pub use priors::{
    sample_default_config, sample_parameters, PriorDist, PriorSet, SampleParam, SampleableParam,
    REFERENCE_CELL_SIZE_M,
};
pub use progress::{new_progress_handle, GenProgressReport, ProgressHandle};
pub use units::{Celsius, Degrees, Meters, SquareKilometers, SquareMeters};
pub use world::{Biome, WorldMap, FLOW_OFFSETS};

use progress::report;

/// Generate a complete world map from configuration.
///
/// # Panics
///
/// Panics if the grid is empty. Use [`try_generate_world`] to handle invalid configs as
/// errors.
pub fn generate_world(config: &WorldGenConfig) -> WorldMap {
    generate_world_with_progress(config, None)
}

/// Validate `config`, then generate its world map.
pub fn try_generate_world(config: &WorldGenConfig) -> Result<WorldMap, ConfigError> {
    config.validate()?;
    Ok(generate_world(config))
}

/// Generate a world map, optionally reporting per-stage progress to a shared handle.
///
/// # Panics
///
/// Panics under the same conditions as [`generate_world`].
pub fn generate_world_with_progress(
    config: &WorldGenConfig,
    progress: Option<ProgressHandle>,
) -> WorldMap {
    assert!(
        config.width > 0 && config.height > 0,
        "map width and height must be at least 1"
    );

    let params = config.resolve();
    let mut map = WorldMap::from_config(config);

    report(&progress, 0.0, "Generating elevation");
    elevation::generate(&mut map, config, &params, &progress, 0.0, 0.40);

    report(&progress, 0.40, "Classifying water");
    water::generate(&mut map, &params);

    report(&progress, 0.50, "Simulating climate");
    climate::generate(&mut map, config, &progress, 0.50, 0.75);

    report(&progress, 0.75, "Routing rivers");
    hydrology::generate(&mut map, config, &params);

    report(&progress, 0.92, "Assigning biomes");
    biomes::generate(&mut map, &params);

    report(&progress, 1.0, "Complete");
    map
}

/// Recompute rivers, lakes, and basins on an existing map, then refresh its biomes.
///
/// This upgrades a stored map that predates hydrology, or applies new settings from
/// `config.hydrology`, without changing its terrain or climate.
pub fn add_hydrology(map: &mut WorldMap, config: &WorldGenConfig) {
    let params = config.resolve();
    hydrology::generate(map, config, &params);
    biomes::generate(map, &params);
}
