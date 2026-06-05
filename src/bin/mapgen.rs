//! Headless map generation CLI — no Bevy window required.
//!
//! ```bash
//! cargo run --bin mapgen -- -o out/map.png --width 512 --seed 42
//! cargo run --bin mapgen -- --batch presets.json --out-dir out/ --stats
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use serde::Deserialize;

use terraforge::{
    WorldGenConfig, compute_map_stats, generate_world, write_map_png, write_map_stats,
};

#[derive(Parser)]
#[command(name = "mapgen", about = "Generate procedural map PNGs without running the game")]
struct Cli {
    /// Output PNG path (single-run mode).
    #[arg(short, long, conflicts_with = "batch")]
    output: Option<PathBuf>,

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
    #[serde(alias = "plate_count")]
    plates: Option<u32>,
    #[arg(long)]
    sea_level: Option<f32>,
    #[arg(long)]
    #[serde(alias = "mountain_elevation_threshold")]
    mountain_elev: Option<f32>,
    #[arg(long)]
    #[serde(alias = "mountain_slope_threshold")]
    mountain_slope: Option<f32>,
    #[arg(long)]
    #[serde(alias = "river_flow_threshold")]
    river_threshold: Option<f32>,
    #[arg(long)]
    #[serde(alias = "temperature_scale")]
    temperature: Option<f32>,
    #[arg(long)]
    #[serde(alias = "elevation_cooling_factor")]
    elev_cooling: Option<f32>,
    #[arg(long)]
    #[serde(alias = "rainfall_scale")]
    rainfall: Option<f32>,
    #[arg(long)]
    #[serde(alias = "plate_boundary_strength")]
    plate_boundary: Option<f32>,
    #[arg(long)]
    #[serde(alias = "continent_noise_frequency")]
    continent_noise: Option<f64>,
    #[arg(long)]
    #[serde(alias = "mountain_noise_frequency")]
    mountain_noise: Option<f64>,
    #[arg(long)]
    #[serde(alias = "hill_noise_frequency")]
    hill_noise: Option<f64>,
    #[arg(long)]
    land_mask_method: Option<String>,
    #[arg(long)]
    coast_sharpening: Option<f32>,
    #[arg(long)]
    river_meander_strength: Option<f32>,
    #[arg(long)]
    hybrid_noise_blend: Option<f32>,
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
        if let Some(v) = self.plates {
            config.plate_count = v;
        }
        if let Some(v) = self.sea_level {
            config.sea_level = v;
        }
        if let Some(v) = self.mountain_elev {
            config.mountain_elevation_threshold = v;
        }
        if let Some(v) = self.mountain_slope {
            config.mountain_slope_threshold = v;
        }
        if let Some(v) = self.river_threshold {
            config.river_flow_threshold = v;
        }
        if let Some(v) = self.temperature {
            config.temperature_scale = v;
        }
        if let Some(v) = self.elev_cooling {
            config.elevation_cooling_factor = v;
        }
        if let Some(v) = self.rainfall {
            config.rainfall_scale = v;
        }
        if let Some(v) = self.plate_boundary {
            config.plate_boundary_strength = v;
        }
        if let Some(v) = self.continent_noise {
            config.continent_noise_frequency = v;
        }
        if let Some(v) = self.mountain_noise {
            config.mountain_noise_frequency = v;
        }
        if let Some(v) = self.hill_noise {
            config.hill_noise_frequency = v;
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

fn run_single(config: &WorldGenConfig, output: &Path, stats_arg: &Option<Option<String>>) -> Result<u64, String> {
    eprintln!(
        "Generating {}x{} seed={} ...",
        config.width, config.height, config.seed
    );
    let started = Instant::now();
    let map = generate_world(config);
    let elapsed_ms = started.elapsed().as_millis() as u64;

    write_map_png(&map, output).map_err(|e| format!("write {}: {e}", output.display()))?;
    eprintln!("Wrote {} ({elapsed_ms} ms)", output.display());

    if let Some(stats_path) = stats_path_from_flag(output, stats_arg) {
        let stats = compute_map_stats(&map, config, elapsed_ms);
        write_map_stats(&stats, &stats_path)
            .map_err(|e| format!("write {}: {e}", stats_path.display()))?;
        eprintln!("Wrote {}", stats_path.display());
    }

    Ok(elapsed_ms)
}

fn run_batch(cli: &Cli) -> Result<(), String> {
    let batch_path = cli.batch.as_ref().expect("batch path");
    let out_dir = cli.out_dir.as_ref().expect("out_dir");
    let write_stats = cli.stats.is_some();

    let text =
        fs::read_to_string(batch_path).map_err(|e| format!("read {}: {e}", batch_path.display()))?;
    let manifest: BatchManifest =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", batch_path.display()))?;

    fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;

    let mut failures = 0u32;
    for variant in &manifest.variants {
        let mut config = WorldGenConfig::default();
        manifest.base.apply_to(&mut config)?;
        variant.patch.apply_to(&mut config)?;

        let png_path = out_dir.join(format!("{}.png", variant.name));
        eprintln!(
            "Variant {}: {}x{} seed={} ...",
            variant.name, config.width, config.height, config.seed
        );

        let started = Instant::now();
        let map = generate_world(&config);
        let elapsed_ms = started.elapsed().as_millis() as u64;

        match write_map_png(&map, &png_path) {
            Ok(()) => eprintln!("  Wrote {} ({elapsed_ms} ms)", png_path.display()),
            Err(e) => {
                eprintln!("  ERROR {}: {e}", png_path.display());
                failures += 1;
                continue;
            }
        }

        if write_stats {
            let stats_path = out_dir.join(format!("{}_stats.json", variant.name));
            let stats = compute_map_stats(&map, &config, elapsed_ms);
            if let Err(e) = write_map_stats(&stats, &stats_path) {
                eprintln!("  ERROR {}: {e}", stats_path.display());
                failures += 1;
            } else {
                eprintln!("  Wrote {}", stats_path.display());
            }
        }
    }

    if failures > 0 {
        Err(format!("{failures} variant(s) failed"))
    } else {
        Ok(())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = if cli.batch.is_some() {
        run_batch(&cli)
    } else {
        let output = match cli.output {
            Some(ref p) => p.clone(),
            None => {
                eprintln!("error: --output is required in single-run mode (or use --batch)");
                return ExitCode::from(2);
            }
        };
        match build_config(&cli) {
            Ok(config) => run_single(&config, &output, &cli.stats).map(|_| ()),
            Err(e) => Err(e),
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
