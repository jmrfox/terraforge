//! Pipeline-level tests.

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
fn rescaling_cell_size_keeps_the_terrain() {
    for (seed, cell_size_m) in [(42, 137.0), (7, 40.0), (99, 1000.0)] {
        let base = WorldGenConfig::test_config(seed, 128);
        let rescaled = base.rescaled_to_cell_size(Meters(cell_size_m));
        let (a, b) = (generate_world(&base), generate_world(&rescaled));
        assert_eq!(b.cell_size_m, cell_size_m);
        for (x, y) in a.elevation.iter().zip(&b.elevation) {
            assert!((x - y).abs() < 1e-4, "elevation {x} vs {y}");
        }
        // Lakes and rivers respond to the new real size, but coastlines do not move.
        let same_coast = a
            .biome
            .iter()
            .zip(&b.biome)
            .filter(|(x, y)| (**x == Biome::Ocean) == (**y == Biome::Ocean))
            .count();
        assert!(same_coast as f32 / a.biome.len() as f32 > 0.999);
    }
}

#[test]
fn try_generate_world_reports_invalid_config() {
    let config = WorldGenConfig {
        width: 0,
        ..WorldGenConfig::default()
    };
    assert!(try_generate_world(&config).is_err());
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

fn sized(seed: u64, width: usize, height: usize) -> WorldGenConfig {
    WorldGenConfig {
        seed,
        width,
        height,
        ..WorldGenConfig::default()
    }
}

fn majority_downsample(map: &WorldMap) -> Vec<Biome> {
    let (cw, ch) = (map.width / 2, map.height / 2);
    let mut out = Vec::with_capacity(cw * ch);
    for y in 0..ch {
        for x in 0..cw {
            let cells = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .map(|(dx, dy)| map.biome[map.index(2 * x + dx, 2 * y + dy)]);
            let best = cells
                .iter()
                .max_by_key(|b| cells.iter().filter(|c| c == b).count())
                .unwrap();
            out.push(*best);
        }
    }
    out
}

#[test]
fn deterministic_and_in_range() {
    let config = sized(99, 96, 64);
    let (a, b) = (generate_world(&config), generate_world(&config));
    assert_eq!(a.elevation, b.elevation);
    assert_eq!(a.temperature_c, b.temperature_c);
    assert_eq!(a.rainfall_mm, b.rainfall_mm);
    assert_eq!(a.biome, b.biome);
    assert_eq!(a.generator_version, GENERATOR_VERSION);
    for layer in [&a.elevation, &a.temperature, &a.rainfall] {
        assert!(layer.iter().all(|v| (0.0..=1.0).contains(v)));
    }
    assert!(a.rainfall_mm.iter().all(|v| v.is_finite() && *v >= 0.0));
}

#[test]
fn lapse_rate_applies_at_configured_rate() {
    let base = sized(42, 128, 128);
    let mut flat = base.clone();
    flat.lapse_rate_c_per_km = 0.0;
    let (a, b) = (generate_world(&base), generate_world(&flat));
    let mut checked = 0;
    for i in 0..a.elevation.len() {
        let km = (a.elevation_m(i) / 1000.0) as f32;
        if !a.water_mask[i] && km > 0.2 {
            let rate = (b.temperature_c[i] - a.temperature_c[i]) / km;
            assert!((rate - 6.5).abs() < 0.01, "rate {rate}");
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn latitude_cools_toward_the_pole() {
    let config = WorldGenConfig {
        target_land_fraction: None,
        edge_ocean_bias: 0.0,
        cell_size_m: Meters(4000.0),
        ..sized(5, 64, 256)
    };
    let map = generate_world(&config);
    let band = |range: std::ops::Range<usize>| {
        let n = (range.len() * map.width) as f32;
        let lapse_free: f32 = range
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let i = map.index(x, y);
                map.temperature_c[i] + 6.5 * (map.elevation_m(i).max(0.0) / 1000.0) as f32
            })
            .sum();
        lapse_free / n
    };
    assert!(band(0..32) < band(224..256) - 2.0);
}

#[test]
fn full_seed_is_used_and_large_seeds_are_safe() {
    let a = generate_world(&sized(42, 48, 48));
    let b = generate_world(&sized(42 + (1 << 32), 48, 48));
    assert_ne!(a.elevation, b.elevation);
    for seed in [u32::MAX as u64 - 1, u32::MAX as u64, u64::MAX] {
        generate_world(&sized(seed, 16, 16));
    }
}

#[test]
fn non_square_maps_are_isotropic() {
    let config = WorldGenConfig {
        target_land_fraction: None,
        edge_ocean_bias: 0.0,
        ..sized(42, 512, 128)
    };
    let m = generate_world(&config);
    let (mut dx, mut dy) = (0.0f64, 0.0f64);
    for y in 0..m.height - 1 {
        for x in 0..m.width - 1 {
            let i = m.index(x, y);
            dx += f64::from((m.elevation[i] - m.elevation[i + 1]).abs());
            dy += f64::from((m.elevation[i] - m.elevation[i + m.width]).abs());
        }
    }
    let ratio = dy / dx;
    assert!((0.75..1.33).contains(&ratio), "gradient ratio {ratio}");
}

#[test]
fn filled_ponds_become_real_land() {
    for seed in [42, 7, 99, 1234] {
        let config = sized(seed, 256, 256);
        let map = generate_world(&config);
        for i in 0..map.elevation.len() {
            if map.water_mask[i] {
                assert_eq!(map.dist_to_water[i], 0);
            } else {
                assert!(map.dist_to_water[i] > 0, "land reported as water");
                assert!(map.rainfall[i] > 0.0, "land with no rain");
            }
        }
    }
}

#[test]
fn land_fraction_tracks_target() {
    for (seed, target) in [(42, 0.35), (7, 0.5), (99, 0.7)] {
        let config = WorldGenConfig {
            target_land_fraction: Some(target),
            ..sized(seed, 256, 256)
        };
        let sea = config.resolve().sea_level_norm;
        let map = generate_world(&config);
        // The target describes terrain above sea level, including ground under lakes.
        let land =
            map.elevation.iter().filter(|&&e| e >= sea).count() as f32 / map.elevation.len() as f32;
        assert!((land - target).abs() < 0.03, "land {land} vs {target}");
    }
}

#[test]
fn biomes_agree_across_resolutions() {
    for seed in [42, 7, 99] {
        let coarse = sized(seed, 256, 256);
        let fine = WorldGenConfig {
            width: 512,
            height: 512,
            cell_size_m: Meters(coarse.cell_size_m.0 / 2.0),
            ..coarse.clone()
        };
        let (a, b) = (generate_world(&coarse), generate_world(&fine));
        let down = majority_downsample(&b);
        let same = a.biome.iter().zip(&down).filter(|(x, y)| x == y).count();
        let agree = same as f32 / a.biome.len() as f32;
        assert!(agree > 0.93, "seed {seed}: biome agreement {agree}");
    }
}

#[test]
fn fixed_land_reference_keeps_coast_across_map_sizes() {
    let small = WorldGenConfig {
        edge_ocean_bias: 0.0,
        land_reference_extent_m: Some(Meters(204_800.0)),
        ..sized(42, 256, 256)
    };
    let large = WorldGenConfig {
        width: 512,
        height: 384,
        ..small.clone()
    };
    let sea = small.resolve().sea_level_norm;
    let (a, b) = (generate_world(&small), generate_world(&large));
    for y in 0..a.height {
        for x in 0..a.width {
            let (ea, eb) = (a.elevation[a.index(x, y)], b.elevation[b.index(x, y)]);
            // A pond cut by the small map's edge counts as ocean there and is only
            // filled to sea level in the larger map.
            assert!(ea == eb || ea == sea || eb == sea, "{ea} vs {eb}");
        }
    }
}
