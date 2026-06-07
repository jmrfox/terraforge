//! Headless map generation CLI — no Bevy window required.
//!
//! ```bash
//! cargo run --bin mapgen -- -o out/map.png --width 512 --seed 42
//! cargo run --bin mapgen -- -o out/map.tiff --format tiff --width 512 --seed 42
//! cargo run --bin mapgen -- --batch presets.json --out-dir out/ --stats
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use serde::Deserialize;

use terraforge::{
    Celsius, Degrees, MapExportFormat, Meters, SquareKilometers, SquareMeters, TiffLayerSet,
    WorldGenConfig, compute_map_stats, generate_world, write_map_stats, write_map_with_tiff_layers,
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum FormatArg {
    #[default]
    Auto,
    Png,
    Tiff,
}

impl FormatArg {
    fn resolve(self, output: &Path) -> MapExportFormat {
        match self {
            Self::Auto => MapExportFormat::from_path(output),
            Self::Png => MapExportFormat::Png,
            Self::Tiff => MapExportFormat::Tiff,
        }
    }
}

#[derive(Parser)]
#[command(name = "mapgen", about = "Generate procedural map previews (PNG or multi-page TIFF)")]
struct Cli {
    /// Output path (single-run mode). Extension `.tiff`/`.tif` selects TIFF unless `--format` overrides.
    #[arg(short, long, conflicts_with = "batch")]
    output: Option<PathBuf>,

    /// Output raster format (`auto` uses the file extension).
    #[arg(long, value_enum, default_value = "auto")]
    format: FormatArg,

    /// TIFF page selection (`full`, `default`, or comma-separated layer names).
    /// Layers: biomes, elevation, temperature, rainfall, biome_id, plate_id, water, river, mountain, orogeny.
    #[arg(long, default_value = "full")]
    tiff_layers: String,

    /// JSON config file (merged over defaults; CLI flags override file fields).
    #[arg(long, conflicts_with = "batch")]
    config: Option<PathBuf>,

    /// Batch manifest JSON (runs many variants).
    #[arg(long, conflicts_with_all = ["output", "config"])]
    batch: Option<PathBuf>,

    /// Output directory for batch mode.
    #[arg(long, requires = "batch")]
    out_dir: Option<PathBuf>,

    /// Write stats JSON. Single-run: optional path (default `{output_stem}_stats.json`).
    /// Batch: always writes `{name}_stats.json` per variant.
    #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
    stats: Option<Option<String>>,

    #[command(flatten)]
    params: ConfigParams,
}

/// Optional overrides for any `WorldGenConfig` field (CLI flags or batch variants).
#[derive(Parser, Default, Deserialize)]
struct ConfigParams {
    #[arg(long)]
    width: Option<usize>,
    #[arg(long)]
    height: Option<usize>,
    #[arg(long)]
    seed: Option<u64>,
    #[arg(long)]
    #[serde(alias = "plate_density_per_km2")]
    plate_density: Option<f64>,
    /// Legacy: absolute plate count (converted to density from current map extent).
    #[arg(long)]
    #[serde(alias = "plate_count")]
    plates: Option<u32>,

    #[arg(long)]
    #[serde(alias = "cell_size_m")]
    cell_size: Option<f64>,
    #[arg(long)]
    #[serde(alias = "max_elevation_m")]
    max_elevation: Option<f64>,
    #[arg(long)]
    #[serde(alias = "sea_level_m")]
    sea_level: Option<f64>,
    #[arg(long)]
    #[serde(alias = "ocean_floor_m")]
    ocean_floor: Option<f64>,

    #[arg(long)]
    #[serde(alias = "continental_margin_m")]
    continental_margin: Option<f64>,
    #[arg(long)]
    #[serde(alias = "min_isthmus_width_m")]
    min_isthmus_width: Option<f64>,
    #[arg(long)]
    #[serde(alias = "mountain_belt_width_m")]
    mountain_belt_width: Option<f64>,
    #[arg(long)]
    #[serde(alias = "mountain_coast_buffer_m")]
    mountain_coast_buffer: Option<f64>,
    #[arg(long)]
    #[serde(alias = "coast_cleanup_proximity_m")]
    coast_cleanup_proximity: Option<f64>,
    #[arg(long)]
    #[serde(alias = "drunkard_brush_radius_m")]
    drunkard_brush_radius: Option<f64>,
    #[arg(long)]
    #[serde(alias = "river_min_length_m")]
    river_min_length: Option<f64>,

    #[arg(long)]
    #[serde(alias = "min_lake_area_m2")]
    min_lake_area: Option<f64>,
    #[arg(long)]
    #[serde(alias = "river_min_drainage_area_km2")]
    river_drainage_area: Option<f64>,
    #[arg(long)]
    #[serde(alias = "river_tributary_drainage_area_km2")]
    river_tributary_drainage: Option<f64>,

    #[arg(long)]
    #[serde(alias = "mountain_min_elevation_m")]
    mountain_min_elevation: Option<f64>,
    #[arg(long)]
    #[serde(alias = "mountain_min_slope_deg")]
    mountain_min_slope: Option<f64>,

    #[arg(long)]
    #[serde(alias = "equator_mean_temp_c")]
    equator_temp: Option<f64>,
    #[arg(long)]
    #[serde(alias = "pole_mean_temp_c")]
    pole_temp: Option<f64>,
    #[arg(long)]
    #[serde(alias = "lapse_rate_c_per_km")]
    lapse_rate: Option<f64>,
    #[arg(long)]
    #[serde(alias = "rainfall_scale")]
    rainfall: Option<f32>,

    #[arg(long)]
    #[serde(alias = "continent_wavelength_m")]
    continent_wavelength: Option<f64>,
    #[arg(long)]
    #[serde(alias = "hill_wavelength_m")]
    hill_wavelength: Option<f64>,
    #[arg(long)]
    #[serde(alias = "mountain_detail_wavelength_m")]
    mountain_detail_wavelength: Option<f64>,
    #[arg(long)]
    #[serde(alias = "land_mask_wavelength_m")]
    land_mask_wavelength: Option<f64>,

    #[arg(long)]
    #[serde(alias = "orogeny_mountain_threshold")]
    orogeny_threshold: Option<f32>,
    #[arg(long)]
    #[serde(alias = "mountain_cluster_threshold")]
    mountain_cluster: Option<f32>,
    #[arg(long)]
    #[serde(alias = "plate_boundary_strength")]
    plate_boundary: Option<f32>,
    #[arg(long)]
    land_mask_method: Option<String>,
    #[arg(long)]
    coast_sharpening: Option<f32>,
    #[arg(long)]
    river_meander_strength: Option<f32>,
    #[arg(long)]
    hybrid_noise_blend: Option<f32>,
    #[arg(long)]
    #[serde(alias = "ca_coarse_cell_size_m")]
    ca_coarse_cell_size: Option<f64>,
    /// Legacy: CA coarse factor relative to cell size.
    #[arg(long)]
    #[serde(alias = "ca_coarse_factor")]
    ca_coarse: Option<u32>,
    #[arg(long)]
    #[serde(alias = "drunkard_walker_density_per_km2")]
    drunkard_walker_density: Option<f64>,
    #[arg(long)]
    #[serde(alias = "land_mask_blur_m")]
    land_mask_blur: Option<f64>,
    #[arg(long)]
    #[serde(alias = "min_landmass_area_km2")]
    min_landmass_area: Option<f64>,
    #[arg(long)]
    #[serde(alias = "orogeny_peak_radius_m")]
    orogeny_peak_radius: Option<f64>,
    #[arg(long)]
    #[serde(alias = "land_mask_close_radius_m")]
    land_mask_close_radius: Option<f64>,
    #[arg(long)]
    #[serde(alias = "max_landmass_density_per_km2")]
    max_landmass_density: Option<f64>,
    #[arg(long)]
    #[serde(alias = "drunkard_path_length_m")]
    drunkard_path_length: Option<f64>,

    #[arg(long)]
    #[serde(alias = "orogeny_interior_min_dist_m")]
    orogeny_interior_min_dist: Option<f64>,
    #[arg(long)]
    mountain_noise_orogeny_only: Option<bool>,

    #[arg(long)]
    target_land_fraction: Option<f32>,
    #[arg(long)]
    #[serde(alias = "shelf_width_m")]
    shelf_width: Option<f64>,
    #[arg(long)]
    #[serde(alias = "shelf_depth_m")]
    shelf_depth: Option<f64>,

    #[arg(long)]
    #[serde(alias = "plate_lloyd_iterations")]
    plate_lloyd_iterations: Option<u32>,
    #[arg(long)]
    continental_plate_speed_max: Option<f32>,
    #[arg(long)]
    oceanic_plate_speed_min: Option<f32>,
    #[arg(long)]
    #[serde(alias = "mantle_flow_angle_deg")]
    mantle_flow_angle: Option<f64>,

    #[arg(long)]
    orographic_orogeny_weight: Option<f32>,
    #[arg(long)]
    interior_drying_factor: Option<f32>,
    #[arg(long)]
    continentality_strength: Option<f32>,
    #[arg(long)]
    #[serde(alias = "continentality_ocean_range_m")]
    continentality_ocean_range: Option<f64>,

    #[arg(long)]
    land_generation: Option<String>,
    #[arg(long)]
    tectonic_uplift_scale: Option<f32>,
    #[arg(long)]
    #[serde(alias = "land_texture_strength_m")]
    land_texture_strength: Option<f64>,
    #[arg(long)]
    #[serde(alias = "land_texture_coast_band_m")]
    land_texture_coast_band: Option<f64>,
    #[arg(long)]
    #[serde(alias = "island_zone_m")]
    island_zone: Option<f64>,
    #[arg(long)]
    landscape_evolution_enabled: Option<bool>,
    #[arg(long)]
    landscape_evolution_iterations: Option<u32>,
    #[arg(long)]
    coarse_hydro_factor: Option<u32>,
    #[arg(long)]
    landscape_evolution_full_res_passes: Option<u32>,
    #[arg(long)]
    landscape_erosion_factor: Option<f32>,
    #[arg(long)]
    landscape_uplift_factor: Option<f32>,
    #[arg(long)]
    erodibility_plains: Option<f32>,
    #[arg(long)]
    erodibility_mountains: Option<f32>,
    #[arg(long)]
    river_incision_enabled: Option<bool>,
    #[arg(long)]
    river_incision_factor: Option<f32>,
    #[arg(long)]
    rainfall_erodibility_coupling: Option<f32>,
    #[arg(long)]
    legacy_coast_cleanup: Option<bool>,
}

#[derive(Deserialize)]
struct BatchManifest {
    #[serde(default)]
    base: ConfigParams,
    variants: Vec<BatchVariant>,
}

#[derive(Deserialize)]
struct BatchVariant {
    name: String,
    #[serde(flatten)]
    patch: ConfigParams,
}

impl ConfigParams {
    fn apply_to(&self, config: &mut WorldGenConfig) -> Result<(), String> {
        if let Some(v) = self.width {
            config.width = v;
        }
        if let Some(v) = self.height {
            config.height = v;
        }
        if let Some(v) = self.seed {
            config.seed = v;
        }
        if let Some(v) = self.cell_size {
            config.cell_size_m = Meters(v);
        }
        if let Some(v) = self.plate_density {
            config.legacy_plate_count = None;
            config.plate_density_per_km2 = v;
        } else if let Some(v) = self.plates {
            config.legacy_plate_count = None;
            config.plate_density_per_km2 = v as f64 / config.map_area_km2().max(1e-9);
        }
        if let Some(v) = self.max_elevation {
            config.max_elevation_m = Meters(v);
        }
        if let Some(v) = self.sea_level {
            config.sea_level_m = Meters(v);
        }
        if let Some(v) = self.ocean_floor {
            config.ocean_floor_m = Meters(v);
        }
        if let Some(v) = self.continental_margin {
            config.continental_margin_m = Meters(v);
        }
        if let Some(v) = self.min_isthmus_width {
            config.min_isthmus_width_m = Meters(v);
        }
        if let Some(v) = self.mountain_belt_width {
            config.mountain_belt_width_m = Meters(v);
        }
        if let Some(v) = self.mountain_coast_buffer {
            config.mountain_coast_buffer_m = Meters(v);
        }
        if let Some(v) = self.coast_cleanup_proximity {
            config.coast_cleanup_proximity_m = Meters(v);
        }
        if let Some(v) = self.drunkard_brush_radius {
            config.drunkard_brush_radius_m = Meters(v);
        }
        if let Some(v) = self.river_min_length {
            config.river_min_length_m = Meters(v);
        }
        if let Some(v) = self.min_lake_area {
            config.min_lake_area_m2 = SquareMeters(v);
        }
        if let Some(v) = self.river_drainage_area {
            config.river_min_drainage_area_km2 = SquareKilometers(v);
        }
        if let Some(v) = self.river_tributary_drainage {
            config.river_tributary_drainage_area_km2 = SquareKilometers(v);
        }
        if let Some(v) = self.mountain_min_elevation {
            config.mountain_min_elevation_m = Meters(v);
        }
        if let Some(v) = self.mountain_min_slope {
            config.mountain_min_slope_deg = Degrees(v);
        }
        if let Some(v) = self.equator_temp {
            config.equator_mean_temp_c = Celsius(v);
        }
        if let Some(v) = self.pole_temp {
            config.pole_mean_temp_c = Celsius(v);
        }
        if let Some(v) = self.lapse_rate {
            config.lapse_rate_c_per_km = v;
        }
        if let Some(v) = self.rainfall {
            config.rainfall_scale = v;
        }
        if let Some(v) = self.continent_wavelength {
            config.continent_wavelength_m = Meters(v);
        }
        if let Some(v) = self.hill_wavelength {
            config.hill_wavelength_m = Meters(v);
        }
        if let Some(v) = self.mountain_detail_wavelength {
            config.mountain_detail_wavelength_m = Meters(v);
        }
        if let Some(v) = self.land_mask_wavelength {
            config.land_mask_wavelength_m = Meters(v);
        }
        if let Some(v) = self.orogeny_threshold {
            config.orogeny_mountain_threshold = v;
        }
        if let Some(v) = self.mountain_cluster {
            config.mountain_cluster_threshold = v;
        }
        if let Some(v) = self.plate_boundary {
            config.plate_boundary_strength = v;
        }
        if let Some(ref v) = self.land_mask_method {
            let json = format!("\"{v}\"");
            config.land_mask_method = serde_json::from_str(&json)
                .map_err(|e| format!("invalid land_mask_method '{v}': {e}"))?;
        }
        if let Some(v) = self.coast_sharpening {
            config.coast_sharpening = v;
        }
        if let Some(v) = self.river_meander_strength {
            config.river_meander_strength = v;
        }
        if let Some(v) = self.hybrid_noise_blend {
            config.hybrid_noise_blend = v;
        }
        if let Some(v) = self.ca_coarse_cell_size {
            config.legacy_ca_coarse_factor = None;
            config.ca_coarse_cell_size_m = Meters(v);
        } else if let Some(v) = self.ca_coarse {
            config.legacy_ca_coarse_factor = None;
            config.ca_coarse_cell_size_m = Meters(config.cell_size_m.0 * v as f64);
        }
        if let Some(v) = self.drunkard_walker_density {
            config.legacy_drunkard_walkers = None;
            config.drunkard_walker_density_per_km2 = v;
        }
        if let Some(v) = self.land_mask_blur {
            config.land_mask_blur_m = Meters(v);
        }
        if let Some(v) = self.min_landmass_area {
            config.min_landmass_area_km2 = SquareKilometers(v);
        }
        if let Some(v) = self.orogeny_peak_radius {
            config.orogeny_peak_radius_m = Meters(v);
        }
        if let Some(v) = self.land_mask_close_radius {
            config.land_mask_close_radius_m = Meters(v);
        }
        if let Some(v) = self.max_landmass_density {
            config.max_landmass_density_per_km2 = v;
        }
        if let Some(v) = self.drunkard_path_length {
            config.drunkard_path_length_m = Meters(v);
        }
        if let Some(v) = self.orogeny_interior_min_dist {
            config.orogeny_interior_min_dist_m = Meters(v);
        }
        if let Some(v) = self.mountain_noise_orogeny_only {
            config.mountain_noise_orogeny_only = v;
        }
        if let Some(v) = self.target_land_fraction {
            config.target_land_fraction = Some(v);
        }
        if let Some(v) = self.shelf_width {
            config.shelf_width_m = Meters(v);
        }
        if let Some(v) = self.shelf_depth {
            config.shelf_depth_m = Meters(v);
        }
        if let Some(v) = self.plate_lloyd_iterations {
            config.plate_lloyd_iterations = v;
        }
        if let Some(v) = self.continental_plate_speed_max {
            config.continental_plate_speed_max = v;
        }
        if let Some(v) = self.oceanic_plate_speed_min {
            config.oceanic_plate_speed_min = v;
        }
        if let Some(v) = self.mantle_flow_angle {
            config.mantle_flow_angle_deg = v;
        }
        if let Some(v) = self.orographic_orogeny_weight {
            config.orographic_orogeny_weight = v;
        }
        if let Some(v) = self.interior_drying_factor {
            config.interior_drying_factor = v;
        }
        if let Some(v) = self.continentality_strength {
            config.continentality_strength = v;
        }
        if let Some(v) = self.continentality_ocean_range {
            config.continentality_ocean_range_m = Meters(v);
        }
        if let Some(ref v) = self.land_generation {
            let json = format!("\"{v}\"");
            config.land_generation = serde_json::from_str(&json)
                .map_err(|e| format!("invalid land_generation '{v}': {e}"))?;
        }
        if let Some(v) = self.tectonic_uplift_scale {
            config.tectonic_uplift_scale = v;
        }
        if let Some(v) = self.land_texture_strength {
            config.land_texture_strength_m = Meters(v);
        }
        if let Some(v) = self.land_texture_coast_band {
            config.land_texture_coast_band_m = Meters(v);
        }
        if let Some(v) = self.island_zone {
            config.island_zone_m = Meters(v);
        }
        if let Some(v) = self.landscape_evolution_enabled {
            config.landscape_evolution_enabled = v;
        }
        if let Some(v) = self.landscape_evolution_iterations {
            config.landscape_evolution_iterations = v;
        }
        if let Some(v) = self.coarse_hydro_factor {
            config.coarse_hydro_factor = v.max(1);
        }
        if let Some(v) = self.landscape_evolution_full_res_passes {
            config.landscape_evolution_full_res_passes = v;
        }
        if let Some(v) = self.landscape_erosion_factor {
            config.landscape_erosion_factor = v;
        }
        if let Some(v) = self.landscape_uplift_factor {
            config.landscape_uplift_factor = v;
        }
        if let Some(v) = self.erodibility_plains {
            config.erodibility_plains = v;
        }
        if let Some(v) = self.erodibility_mountains {
            config.erodibility_mountains = v;
        }
        if let Some(v) = self.river_incision_enabled {
            config.river_incision_enabled = v;
        }
        if let Some(v) = self.river_incision_factor {
            config.river_incision_factor = v;
        }
        if let Some(v) = self.rainfall_erodibility_coupling {
            config.rainfall_erodibility_coupling = v;
        }
        if let Some(v) = self.legacy_coast_cleanup {
            config.legacy_coast_cleanup = v;
        }
        Ok(())
    }
}

fn load_config_file(path: &Path) -> Result<WorldGenConfig, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn build_config(cli: &Cli) -> Result<WorldGenConfig, String> {
    let mut config = if let Some(ref path) = cli.config {
        load_config_file(path)?
    } else {
        WorldGenConfig::default()
    };
    cli.params.apply_to(&mut config)?;
    Ok(config)
}

fn default_stats_path(png_path: &Path) -> PathBuf {
    let stem = png_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("map");
    png_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}_stats.json"))
}

fn stats_path_from_flag(output: &Path, stats_arg: &Option<Option<String>>) -> Option<PathBuf> {
    let arg = stats_arg.as_ref()?;
    match arg {
        None => Some(default_stats_path(output)),
        Some(s) if s == "auto" => Some(default_stats_path(output)),
        Some(s) => Some(PathBuf::from(s)),
    }
}

fn run_single(
    config: &WorldGenConfig,
    output: &Path,
    format: MapExportFormat,
    tiff_layers: &TiffLayerSet,
    stats_path: Option<&Path>,
) -> Result<(), String> {
    let start = Instant::now();
    let map = generate_world(config);
    let elapsed = start.elapsed().as_millis() as u64;

    write_map_with_tiff_layers(&map, output, format, *tiff_layers)
        .map_err(|e| format!("write {}: {e}", output.display()))?;

    if let Some(path) = stats_path {
        let stats = compute_map_stats(&map, config, elapsed);
        write_map_stats(&stats, path).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn run_batch(
    manifest_path: &Path,
    out_dir: &Path,
    format: MapExportFormat,
    tiff_layers: &TiffLayerSet,
    write_stats: bool,
) -> Result<(), String> {
    let text =
        fs::read_to_string(manifest_path).map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let manifest: BatchManifest =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", manifest_path.display()))?;

    fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;

    let mut base = WorldGenConfig::default();
    manifest.base.apply_to(&mut base)?;

    for variant in &manifest.variants {
        let mut config = base.clone();
        variant.patch.apply_to(&mut config)?;

        let output = out_dir.join(format!("{}.{}", variant.name, format.extension()));
        let stats_path = if write_stats {
            Some(out_dir.join(format!("{}_stats.json", variant.name)))
        } else {
            None
        };

        let start = Instant::now();
        let map = generate_world(&config);
        let elapsed = start.elapsed().as_millis() as u64;

        write_map_with_tiff_layers(&map, &output, format, *tiff_layers)
            .map_err(|e| format!("write {}: {e}", output.display()))?;

        if let Some(ref path) = stats_path {
            let stats = compute_map_stats(&map, &config, elapsed);
            write_map_stats(&stats, path).map_err(|e| format!("write {}: {e}", path.display()))?;
        }

        eprintln!("{} -> {}", variant.name, output.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let tiff_layers = match TiffLayerSet::parse(&cli.tiff_layers) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let result = if let Some(ref batch_path) = cli.batch {
        let out_dir = cli.out_dir.as_ref().expect("out_dir required with batch");
        let format = cli.format.resolve(out_dir);
        run_batch(
            batch_path,
            out_dir,
            format,
            &tiff_layers,
            cli.stats.is_some(),
        )
    } else {
        let output = match cli.output {
            Some(ref p) => p.clone(),
            None => {
                eprintln!("error: --output required (or use --batch)");
                return ExitCode::from(2);
            }
        };
        let format = cli.format.resolve(&output);
        let config = match build_config(&cli) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        };
        let stats_path = stats_path_from_flag(&output, &cli.stats);
        run_single(
            &config,
            &output,
            format,
            &tiff_layers,
            stats_path.as_deref(),
        )
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
