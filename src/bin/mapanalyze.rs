//! Batch map analysis — prints aggregate and per-seed diagnostics.
//!
//! ```bash
//! cargo run --release --bin mapanalyze
//! cargo run --release --bin mapanalyze -- --seeds 42,7,99,1234,5555
//! ```

use std::collections::HashMap;

use terraforge::{generate_world, Biome, WorldGenConfig, WorldMap};

struct MapDiagnostics {
    name: String,
    seed: u64,
    land_fraction: f64,
    ocean_fraction: f64,
    lake_fraction: f64,
    mountain_fraction: f64,
    ice_edge_fraction: f64,
    ice_center_fraction: f64,
    desert_interior_fraction: f64,
    desert_coast_fraction: f64,
    forest_land_fraction: f64,
    rainfall_row_std: f32,
    rainfall_col_std: f32,
    temp_edge_mean: f32,
    temp_center_mean: f32,
    temp_range: f32,
    max_land_biome_share: f64,
    dominant_land_biome: String,
}

fn biome_name(b: Biome) -> &'static str {
    match b {
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

fn analyze_map(name: &str, config: &WorldGenConfig) -> MapDiagnostics {
    let map = generate_world(config);
    let total = (map.width * map.height) as f64;
    let w = map.width;
    let h = map.height;
    let edge_band = (h / 10).max(1);
    let center_band = (h / 5).max(1);
    let center_y0 = h / 2 - center_band / 2;
    let center_y1 = center_y0 + center_band;

    let mut counts: HashMap<Biome, usize> = HashMap::new();
    for &b in &map.biome {
        *counts.entry(b).or_insert(0) += 1;
    }

    let land_cells = map.water_mask.iter().filter(|&&x| !x).count() as f64;
    let ocean = *counts.get(&Biome::Ocean).unwrap_or(&0) as f64;
    let lake = *counts.get(&Biome::Lake).unwrap_or(&0) as f64;
    let mountain = *counts.get(&Biome::Mountain).unwrap_or(&0) as f64;

    let mut ice_edge = 0usize;
    let mut ice_center = 0usize;
    let mut desert_interior = 0usize;
    let mut desert_coast = 0usize;
    let mut desert_total = 0usize;

    let dist_coast = &map.dist_to_water;
    let coast_threshold = (w.min(h) / 8).max(4) as u32;

    for y in 0..h {
        for x in 0..w {
            let idx = map.index(x, y);
            let b = map.biome[idx];
            if b == Biome::Ice {
                if y < edge_band || y >= h - edge_band {
                    ice_edge += 1;
                }
                if y >= center_y0 && y < center_y1 {
                    ice_center += 1;
                }
            }
            if b == Biome::Desert && !map.water_mask[idx] {
                desert_total += 1;
                if dist_coast[idx] >= coast_threshold {
                    desert_interior += 1;
                } else {
                    desert_coast += 1;
                }
            }
        }
    }

    let forest_land = counts.get(&Biome::TropicalForest).unwrap_or(&0)
        + counts.get(&Biome::TemperateForest).unwrap_or(&0);
    let forest_land_fraction = forest_land as f64 / land_cells.max(1.0);

    let (rain_row_std, rain_col_std) = rainfall_anisotropy(&map);
    let (temp_edge, temp_center, temp_range) = temperature_band_profile(&map);

    let mut land_biomes: Vec<_> = counts
        .iter()
        .filter(|(&b, _)| b != Biome::Ocean && b != Biome::Lake)
        .collect();
    land_biomes.sort_by(|a, b| b.1.cmp(a.1));
    let (dominant, dom_count) = land_biomes
        .first()
        .copied()
        .unwrap_or((&Biome::Grassland, &0));

    MapDiagnostics {
        name: name.to_string(),
        seed: config.seed,
        land_fraction: land_cells / total,
        ocean_fraction: ocean / total,
        lake_fraction: lake / total,
        mountain_fraction: mountain / total,
        ice_edge_fraction: ice_edge as f64 / land_cells.max(1.0),
        ice_center_fraction: ice_center as f64 / land_cells.max(1.0),
        desert_interior_fraction: if desert_total > 0 {
            desert_interior as f64 / desert_total as f64
        } else {
            0.0
        },
        desert_coast_fraction: if desert_total > 0 {
            desert_coast as f64 / desert_total as f64
        } else {
            0.0
        },
        forest_land_fraction,
        rainfall_row_std: rain_row_std,
        rainfall_col_std: rain_col_std,
        temp_edge_mean: temp_edge,
        temp_center_mean: temp_center,
        temp_range,
        max_land_biome_share: *dom_count as f64 / land_cells.max(1.0),
        dominant_land_biome: biome_name(*dominant).to_string(),
    }
}

fn rainfall_anisotropy(map: &WorldMap) -> (f32, f32) {
    let w = map.width;
    let h = map.height;

    let mut row_means = vec![0.0f32; h];
    let mut col_means = vec![0.0f32; w];
    for (y, row_mean) in row_means.iter_mut().enumerate().take(h) {
        for (x, col_mean) in col_means.iter_mut().enumerate().take(w) {
            let v = map.rainfall[map.index(x, y)];
            *row_mean += v;
            *col_mean += v;
        }
    }
    for r in &mut row_means {
        *r /= w as f32;
    }
    for c in &mut col_means {
        *c /= h as f32;
    }

    let row_std = std_dev(&row_means);
    let col_std = std_dev(&col_means);
    (row_std, col_std)
}

fn std_dev(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;
    var.sqrt()
}

/// Mean land temperature in top/bottom edge bands vs map center (detects horizontal striping).
fn temperature_band_profile(map: &WorldMap) -> (f32, f32, f32) {
    let w = map.width;
    let h = map.height;
    let edge_band = (h / 10).max(1);

    let mut edge_sum = 0.0f32;
    let mut edge_n = 0usize;
    let mut center_sum = 0.0f32;
    let mut center_n = 0usize;
    let mut min_t = f32::MAX;
    let mut max_t = f32::MIN;

    let center_y0 = h / 2 - edge_band / 2;
    let center_y1 = center_y0 + edge_band;

    for y in 0..h {
        for x in 0..w {
            let idx = map.index(x, y);
            if map.water_mask[idx] {
                continue;
            }
            let t = map.temperature[idx];
            min_t = min_t.min(t);
            max_t = max_t.max(t);
            if y < edge_band || y >= h - edge_band {
                edge_sum += t;
                edge_n += 1;
            }
            if y >= center_y0 && y < center_y1 {
                center_sum += t;
                center_n += 1;
            }
        }
    }

    (
        if edge_n > 0 {
            edge_sum / edge_n as f32
        } else {
            0.0
        },
        if center_n > 0 {
            center_sum / center_n as f32
        } else {
            0.0
        },
        max_t - min_t,
    )
}

fn print_report(d: &MapDiagnostics) {
    println!("--- {} (seed {}) ---", d.name, d.seed);
    println!(
        "  land {:.1}% | ocean {:.1}% | lake {:.1}% | mountain {:.2}%",
        d.land_fraction * 100.0,
        d.ocean_fraction * 100.0,
        d.lake_fraction * 100.0,
        d.mountain_fraction * 100.0
    );
    println!(
        "  land biomes: dominant {} ({:.1}% of land)",
        d.dominant_land_biome,
        d.max_land_biome_share * 100.0
    );
    println!(
        "  forests {:.1}% of land | ice edge {:.2}% center {:.3}% of land",
        d.forest_land_fraction * 100.0,
        d.ice_edge_fraction * 100.0,
        d.ice_center_fraction * 100.0
    );
    println!(
        "  deserts {:.0}% interior / {:.0}% coast (of desert cells)",
        d.desert_interior_fraction * 100.0,
        d.desert_coast_fraction * 100.0
    );
    println!(
        "  temp edge {:.2} center {:.2} range {:.2}",
        d.temp_edge_mean, d.temp_center_mean, d.temp_range
    );
    let striping = d.rainfall_col_std / d.rainfall_row_std.max(1e-6);
    println!(
        "  rainfall anisotropy col/row std ratio {:.2} (>{:.1} = west-east banding)",
        striping, 1.5
    );
    println!();
}

fn main() {
    let seed_list: Vec<u64> = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|w| w[0] == "--seeds")
        .map(|w| {
            w[1].split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect()
        })
        .unwrap_or_else(|| vec![42, 7, 99, 1234, 5555]);

    let mut configs: Vec<(String, WorldGenConfig)> = seed_list
        .iter()
        .map(|&seed| {
            (
                format!("seed_{seed}"),
                WorldGenConfig {
                    seed,
                    ..Default::default()
                },
            )
        })
        .collect();

    configs.push((
        "high_land_50".into(),
        WorldGenConfig {
            seed: 42,
            target_land_fraction: Some(0.50),
            ..Default::default()
        },
    ));
    configs.push((
        "low_land_20".into(),
        WorldGenConfig {
            seed: 42,
            target_land_fraction: Some(0.20),
            ..Default::default()
        },
    ));
    configs.push((
        "no_edge_bias".into(),
        WorldGenConfig {
            seed: 42,
            edge_ocean_bias: 0.0,
            ..Default::default()
        },
    ));
    configs.push((
        "strong_edge_bias".into(),
        WorldGenConfig {
            seed: 42,
            edge_ocean_bias: 0.25,
            ..Default::default()
        },
    ));

    let reports: Vec<MapDiagnostics> = configs
        .iter()
        .map(|(name, cfg)| analyze_map(name, cfg))
        .collect();

    println!("=== Terraforge batch analysis (512x512) ===\n");
    for r in &reports {
        print_report(r);
    }

    let n = reports.len() as f64;
    let avg_mountain = reports.iter().map(|r| r.mountain_fraction).sum::<f64>() / n;
    let avg_lake = reports.iter().map(|r| r.lake_fraction).sum::<f64>() / n;
    let avg_forest = reports.iter().map(|r| r.forest_land_fraction).sum::<f64>() / n;
    let avg_striping = reports
        .iter()
        .map(|r| r.rainfall_col_std as f64 / r.rainfall_row_std.max(1e-6) as f64)
        .sum::<f64>()
        / n;
    let zero_mountain = reports
        .iter()
        .filter(|r| r.mountain_fraction == 0.0)
        .count();

    println!("=== Aggregate ({n} maps) ===");
    println!("  Maps with ZERO mountains: {zero_mountain}/{n}");
    println!("  Avg mountain coverage: {:.3}%", avg_mountain * 100.0);
    println!("  Avg lake coverage: {:.2}%", avg_lake * 100.0);
    println!("  Avg forest share of land: {:.1}%", avg_forest * 100.0);
    println!(
        "  Avg rainfall west-east striping ratio: {:.2}",
        avg_striping
    );

    if let Some(worst) = reports.iter().max_by(|a, b| {
        a.max_land_biome_share
            .partial_cmp(&b.max_land_biome_share)
            .unwrap()
    }) {
        println!(
            "  Most biome-monoculture: {} ({:.1}% {})",
            worst.name,
            worst.max_land_biome_share * 100.0,
            worst.dominant_land_biome
        );
    }
}
