//! Headless map generation CLI (no GUI).
//!
//! ```bash
//! cargo run --bin mapgen -- -o out/map.png --width 512 --seed 42
//! cargo run --bin mapgen -- -o out/map.png --sample --sample-seed 99 --seed 42
//! cargo run --bin mapgen -- -o out/map.tiff --format tiff --width 512 --seed 42
//! cargo run --bin mapgen -- --batch presets.json --out-dir out/ --stats
//! cargo run --bin mapgen -- --batch mapgen_presets/sample_batch.json --out-dir out/
//! cargo run --bin mapgen -- -o out/world.tfmap --seed 42
//! cargo run --bin mapgen -- --load-map out/world.tfmap -o out/world.png
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use terraforge::{
    compute_map_stats, load_map_file, sample_parameters, save_map_file, try_generate_world,
    write_map_stats, write_map_with_tiff_layers, Degrees, MapExportFormat, Meters, PriorSet,
    SquareMeters, TiffLayerSet, WorldGenConfig, WorldMap, MAP_FILE_EXTENSION,
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
#[command(
    name = "mapgen",
    about = "Generate procedural maps and export PNG, multi-page TIFF, or compressed .tfmap files"
)]
struct Cli {
    /// Output path (single-run mode). `.tfmap` stores the full map; `.tiff`/`.tif` selects
    /// TIFF unless `--format` overrides; anything else writes PNG.
    #[arg(short, long, conflicts_with = "batch")]
    output: Option<PathBuf>,

    /// Also store the full map as a compressed `.tfmap` file.
    #[arg(long, conflicts_with = "batch")]
    save_map: Option<PathBuf>,

    /// Load a stored `.tfmap` map instead of generating one.
    #[arg(long, conflicts_with_all = ["batch", "config", "ConfigParams"])]
    load_map: Option<PathBuf>,

    /// Output raster format (`auto` uses the file extension).
    #[arg(long, value_enum, default_value = "auto")]
    format: FormatArg,

    /// TIFF page selection (`full`, `default`, or comma-separated layer names).
    /// Layers: biomes, elevation, temperature, rainfall, biome_id, water.
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
    #[serde(alias = "continent_wavelength_m")]
    continent_wavelength: Option<f64>,
    #[arg(long)]
    #[serde(alias = "detail_wavelength_m")]
    detail_wavelength: Option<f64>,
    #[arg(long)]
    elevation_octaves: Option<u32>,
    #[arg(long)]
    elevation_persistence: Option<f64>,
    #[arg(long)]
    elevation_continent_weight: Option<f32>,
    #[arg(long)]
    elevation_detail_weight: Option<f32>,
    #[arg(long)]
    elevation_ridge_weight: Option<f32>,
    #[arg(long)]
    elevation_ridge_envelope_enabled: Option<bool>,
    #[arg(long)]
    elevation_ridge_envelope_wavelength_m: Option<f64>,
    #[arg(long)]
    elevation_ridge_envelope_strength: Option<f32>,
    #[arg(long)]
    elevation_detail_envelope_enabled: Option<bool>,
    #[arg(long)]
    elevation_detail_envelope_wavelength_m: Option<f64>,
    #[arg(long)]
    elevation_detail_envelope_strength: Option<f32>,
    #[arg(long)]
    target_land_fraction: Option<f32>,
    #[arg(long)]
    edge_ocean_bias: Option<f32>,

    #[arg(long)]
    #[serde(alias = "min_lake_area_m2")]
    min_lake_area: Option<f64>,

    #[arg(long)]
    #[serde(alias = "lapse_rate_c_per_km")]
    lapse_rate: Option<f64>,
    #[arg(long)]
    #[serde(alias = "rainfall_scale")]
    rainfall: Option<f32>,
    #[arg(long)]
    #[serde(alias = "temperature_wavelength_m")]
    temperature_wavelength: Option<f64>,

    #[arg(long)]
    #[serde(alias = "mountain_min_elevation_m")]
    mountain_min_elevation: Option<f64>,
    #[arg(long)]
    #[serde(alias = "mountain_min_slope_deg")]
    mountain_min_slope: Option<f64>,
    #[arg(long)]
    mountain_min_ridge_influence: Option<f32>,

    /// Latitude at the map center.
    #[arg(long)]
    latitude_deg: Option<f64>,
    /// Compass direction the prevailing wind blows from.
    #[arg(long)]
    wind_from_deg: Option<f64>,
    /// Annual rain from saturated air at 15 °C.
    #[arg(long)]
    base_rainfall_mm: Option<f64>,
    /// Warms or cools the whole world.
    #[arg(long, allow_negative_numbers = true)]
    temperature_offset_c: Option<f64>,

    /// Mean annual discharge at which a channel counts as a river.
    #[arg(long)]
    river_min_discharge_m3s: Option<f64>,
    /// Minimum depression depth that can hold a lake.
    #[arg(long)]
    min_lake_depth_m: Option<f64>,
    /// How far a river flowing at 10 m³/s cuts through a lake outlet.
    #[arg(long)]
    outlet_incision_m: Option<f64>,

    /// Sample numerical parameters from default priors before generation (same as GUI "Sample & generate").
    #[arg(long)]
    #[serde(default)]
    sample: bool,

    /// RNG seed for prior sampling (reproducible parameter draws). Omit for a random draw.
    #[arg(long)]
    sample_seed: Option<u64>,
}

#[derive(Deserialize)]
struct BatchManifest {
    #[serde(default)]
    base: BatchBase,
    variants: Vec<BatchVariant>,
}

#[derive(Default, Deserialize)]
struct BatchBase {
    #[serde(flatten)]
    params: ConfigParams,
    /// Keys not recognized by `params`, reported as errors.
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct BatchVariant {
    name: String,
    #[serde(flatten)]
    patch: ConfigParams,
    /// Keys not recognized by `patch`, reported as errors.
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

impl BatchManifest {
    fn check_unknown_fields(&self) -> Result<(), String> {
        let sections = std::iter::once(("base", &self.base.unknown))
            .chain(self.variants.iter().map(|v| (v.name.as_str(), &v.unknown)));
        let issues: Vec<String> = sections
            .flat_map(|(section, unknown)| {
                unknown
                    .keys()
                    .map(move |key| format!("{section}: unknown field `{key}`"))
            })
            .collect();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join("; "))
        }
    }
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
        if let Some(v) = self.max_elevation {
            config.max_elevation_m = Meters(v);
        }
        if let Some(v) = self.sea_level {
            config.sea_level_m = Meters(v);
        }
        if let Some(v) = self.ocean_floor {
            config.ocean_floor_m = Meters(v);
        }
        if let Some(v) = self.continent_wavelength {
            config.continent_wavelength_m = Meters(v);
        }
        if let Some(v) = self.detail_wavelength {
            config.detail_wavelength_m = Meters(v);
        }
        if let Some(v) = self.elevation_octaves {
            config.elevation_octaves = v.max(1);
        }
        if let Some(v) = self.elevation_persistence {
            config.elevation_persistence = v.clamp(0.1, 0.95);
        }
        if let Some(v) = self.elevation_continent_weight {
            config.elevation_continent_weight = v;
        }
        if let Some(v) = self.elevation_detail_weight {
            config.elevation_detail_weight = v;
        }
        if let Some(v) = self.elevation_ridge_weight {
            config.elevation_ridge_weight = v;
        }
        if let Some(v) = self.elevation_ridge_envelope_enabled {
            config.elevation_ridge_envelope.enabled = v;
        }
        if let Some(v) = self.elevation_ridge_envelope_wavelength_m {
            config.elevation_ridge_envelope.wavelength_m = Meters(v);
        }
        if let Some(v) = self.elevation_ridge_envelope_strength {
            config.elevation_ridge_envelope.strength = v;
        }
        if let Some(v) = self.elevation_detail_envelope_enabled {
            config.elevation_detail_envelope.enabled = v;
        }
        if let Some(v) = self.elevation_detail_envelope_wavelength_m {
            config.elevation_detail_envelope.wavelength_m = Meters(v);
        }
        if let Some(v) = self.elevation_detail_envelope_strength {
            config.elevation_detail_envelope.strength = v;
        }
        if let Some(v) = self.target_land_fraction {
            config.target_land_fraction = Some(v.clamp(0.01, 0.99));
        }
        if let Some(v) = self.edge_ocean_bias {
            config.edge_ocean_bias = v.clamp(0.0, 0.5);
        }
        if let Some(v) = self.min_lake_area {
            config.min_lake_area_m2 = SquareMeters(v);
        }
        if let Some(v) = self.lapse_rate {
            config.lapse_rate_c_per_km = v;
        }
        if let Some(v) = self.rainfall {
            config.rainfall_scale = v;
        }
        if let Some(v) = self.temperature_wavelength {
            config.temperature_wavelength_m = Meters(v);
        }
        if let Some(v) = self.mountain_min_elevation {
            config.mountain_min_elevation_m = Meters(v);
        }
        if let Some(v) = self.mountain_min_slope {
            config.mountain_min_slope_deg = Degrees(v);
        }
        if let Some(v) = self.mountain_min_ridge_influence {
            config.mountain_min_ridge_influence = v.clamp(0.0, 1.0);
        }
        if let Some(v) = self.latitude_deg {
            config.regional_climate.latitude_deg = v;
        }
        if let Some(v) = self.wind_from_deg {
            config.regional_climate.wind_from_deg = v;
        }
        if let Some(v) = self.base_rainfall_mm {
            config.regional_climate.base_rainfall_mm = v;
        }
        if let Some(v) = self.temperature_offset_c {
            config.regional_climate.temperature_offset_c = v;
        }
        if let Some(v) = self.river_min_discharge_m3s {
            config.hydrology.river_min_discharge_m3s = v;
        }
        if let Some(v) = self.min_lake_depth_m {
            config.hydrology.min_lake_depth_m = v;
        }
        if let Some(v) = self.outlet_incision_m {
            config.hydrology.outlet_incision_m = v;
        }
        Ok(())
    }
}

fn load_config_file(path: &Path) -> Result<WorldGenConfig, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    WorldGenConfig::from_json(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn build_config(cli: &Cli) -> Result<WorldGenConfig, String> {
    let mut config = if let Some(ref path) = cli.config {
        load_config_file(path)?
    } else {
        WorldGenConfig::default()
    };
    let sample = cli.params.sample;
    let sample_seed = cli.params.sample_seed;
    let had_seed = cli.params.seed.is_some();
    cli.params.apply_to(&mut config)?;
    if sample {
        let priors = PriorSet::default_priors();
        let used = sample_parameters(&mut config, &priors, sample_seed);
        eprintln!(
            "sampled {} parameters (sample-seed {used})",
            priors.enabled_count()
        );
        if !had_seed {
            config.seed = rand::thread_rng().gen();
            eprintln!("map seed {}", config.seed);
        }
    }
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

fn generate_write_and_stats(
    config: &WorldGenConfig,
    output: &Path,
    format: MapExportFormat,
    tiff_layers: &TiffLayerSet,
    stats_path: Option<&Path>,
) -> Result<(), String> {
    let start = Instant::now();
    let map = try_generate_world(config).map_err(|e| e.to_string())?;
    let elapsed = start.elapsed().as_millis() as u64;

    write_map_with_tiff_layers(&map, output, format, *tiff_layers)
        .map_err(|e| format!("write {}: {e}", output.display()))?;

    if let Some(path) = stats_path {
        let stats = compute_map_stats(&map, config, elapsed);
        write_map_stats(&stats, path).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn is_map_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(MAP_FILE_EXTENSION))
}

fn save_stored(path: &Path, map: &WorldMap, config: &WorldGenConfig) -> Result<(), String> {
    save_map_file(path, map, config).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Generate or load one map, then write every requested output.
fn run_single(
    cli: &Cli,
    generated_config: Option<WorldGenConfig>,
    tiff_layers: &TiffLayerSet,
) -> Result<(), String> {
    let start = Instant::now();
    let (map, config) = match (&cli.load_map, generated_config) {
        (Some(path), _) => {
            let stored =
                load_map_file(path).map_err(|e| format!("read {}: {e}", path.display()))?;
            (stored.map, stored.config)
        }
        (None, Some(config)) => {
            let map = try_generate_world(&config).map_err(|e| e.to_string())?;
            (map, config)
        }
        (None, None) => return Err("no map to generate or load".to_string()),
    };
    let elapsed = start.elapsed().as_millis() as u64;

    if let Some(path) = &cli.save_map {
        save_stored(path, &map, &config)?;
    }
    if let Some(output) = &cli.output {
        if is_map_file(output) {
            save_stored(output, &map, &config)?;
        } else {
            write_map_with_tiff_layers(&map, output, cli.format.resolve(output), *tiff_layers)
                .map_err(|e| format!("write {}: {e}", output.display()))?;
        }
    }

    let stats_base = cli.output.as_ref().or(cli.save_map.as_ref());
    if let Some(path) = stats_base.and_then(|base| stats_path_from_flag(base, &cli.stats)) {
        let stats = compute_map_stats(&map, &config, elapsed);
        write_map_stats(&stats, &path).map_err(|e| format!("write {}: {e}", path.display()))?;
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
    let text = fs::read_to_string(manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let manifest: BatchManifest = serde_json::from_str(&text)
        .map_err(|e| format!("parse {}: {e}", manifest_path.display()))?;
    manifest
        .check_unknown_fields()
        .map_err(|e| format!("parse {}: {e}", manifest_path.display()))?;

    fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;

    let mut base = WorldGenConfig::default();
    manifest.base.params.apply_to(&mut base)?;

    for variant in &manifest.variants {
        let mut config = base.clone();
        let sample = variant.patch.sample || manifest.base.params.sample;
        let sample_seed = variant
            .patch
            .sample_seed
            .or(manifest.base.params.sample_seed);
        let had_seed = variant.patch.seed.is_some() || manifest.base.params.seed.is_some();
        variant.patch.apply_to(&mut config)?;

        if sample {
            let priors = PriorSet::default_priors();
            let used = sample_parameters(&mut config, &priors, sample_seed);
            eprintln!(
                "{}: sampled {} parameters (sample-seed {used})",
                variant.name,
                priors.enabled_count()
            );
            if !had_seed {
                config.seed = rand::thread_rng().gen();
                eprintln!("{}: map seed {}", variant.name, config.seed);
            }
        }

        let output = out_dir.join(format!("{}.{}", variant.name, format.extension()));
        let stats_path = if write_stats {
            Some(out_dir.join(format!("{}_stats.json", variant.name)))
        } else {
            None
        };

        generate_write_and_stats(&config, &output, format, tiff_layers, stats_path.as_deref())?;

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
        if cli.output.is_none() && cli.save_map.is_none() {
            eprintln!("error: --output or --save-map required (or use --batch)");
            return ExitCode::from(2);
        }
        let config = if cli.load_map.is_some() {
            None
        } else {
            match build_config(&cli) {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            }
        };
        run_single(&cli, config, &tiff_layers)
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
