//! Headless map generation CLI (no GUI).
//!
//! ```bash
//! cargo run --bin mapgen -- -o out/map.png --width 512 --seed 42
//! cargo run --bin mapgen -- -o out/map.png --sample --sample-seed 99 --seed 42
//! cargo run --bin mapgen -- -o out/map.tiff --format tiff --width 512 --seed 42
//! cargo run --bin mapgen -- --batch presets.json --out-dir out/ --stats
//! cargo run --bin mapgen -- --batch mapgen_presets/sample_batch.json --out-dir out/
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use rand::Rng;
use serde::Deserialize;

use terraforge::{
    compute_map_stats, generate_world, sample_parameters, write_map_stats,
    write_map_with_tiff_layers, Degrees, MapExportFormat, Meters, PriorSet, SquareMeters,
    TiffLayerSet, WorldGenConfig,
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
    about = "Generate procedural map previews (PNG or multi-page TIFF)"
)]
struct Cli {
    /// Output path (single-run mode). Extension `.tiff`/`.tif` selects TIFF unless `--format` overrides.
    #[arg(short, long, conflicts_with = "batch")]
    output: Option<PathBuf>,

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
    #[serde(alias = "elevation_wavelength_m")]
    elevation_wavelength: Option<f64>,
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
    #[serde(alias = "temperature_range_c")]
    temperature_range: Option<f64>,
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
    continentality_strength: Option<f32>,
    #[arg(long)]
    #[serde(alias = "continentality_ocean_range_m")]
    continentality_ocean_range: Option<f64>,
    #[arg(long)]
    orographic_elevation_weight: Option<f32>,
    #[arg(long)]
    interior_drying_factor: Option<f32>,

    #[arg(long)]
    #[serde(alias = "mountain_min_elevation_m")]
    mountain_min_elevation: Option<f64>,
    #[arg(long)]
    #[serde(alias = "mountain_min_slope_deg")]
    mountain_min_slope: Option<f64>,
    #[arg(long)]
    mountain_min_ridge_influence: Option<f32>,

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
        if let Some(v) = self.max_elevation {
            config.max_elevation_m = Meters(v);
        }
        if let Some(v) = self.sea_level {
            config.sea_level_m = Meters(v);
        }
        if let Some(v) = self.ocean_floor {
            config.ocean_floor_m = Meters(v);
        }
        if let Some(v) = self.elevation_wavelength {
            config.elevation_wavelength_m = Meters(v);
            config.continent_wavelength_m = Meters(v);
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
        if let Some(v) = self.temperature_range {
            config.temperature_range_c = v.max(1.0);
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
        if let Some(v) = self.continentality_strength {
            config.continentality_strength = v;
        }
        if let Some(v) = self.continentality_ocean_range {
            config.continentality_ocean_range_m = Meters(v);
        }
        if let Some(v) = self.orographic_elevation_weight {
            config.orographic_elevation_weight = v;
        }
        if let Some(v) = self.interior_drying_factor {
            config.interior_drying_factor = v;
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

fn run_single(
    config: &WorldGenConfig,
    output: &Path,
    format: MapExportFormat,
    tiff_layers: &TiffLayerSet,
    stats_path: Option<&Path>,
) -> Result<(), String> {
    generate_write_and_stats(config, output, format, tiff_layers, stats_path)
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

    fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;

    let mut base = WorldGenConfig::default();
    manifest.base.apply_to(&mut base)?;

    for variant in &manifest.variants {
        let mut config = base.clone();
        let sample = variant.patch.sample || manifest.base.sample;
        let sample_seed = variant.patch.sample_seed.or(manifest.base.sample_seed);
        let had_seed = variant.patch.seed.is_some() || manifest.base.seed.is_some();
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
