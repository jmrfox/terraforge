use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
use super::grid_ops::{box_blur, chamfer_distance_water, normalize01};
use super::progress::{report_stage, ProgressHandle};
use super::world::WorldMap;

struct RainfallNoise {
    field: Fbm<Perlin>,
}

impl RainfallNoise {
    fn new(config: &WorldGenConfig, params: &ResolvedSimParams) -> Self {
        Self {
            field: Fbm::<Perlin>::new((config.seed as u32).wrapping_add(5))
                .set_octaves(4)
                .set_frequency(params.temperature_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
        }
    }

    fn sample(&self, nx: f32, ny: f32) -> f32 {
        (self.field.get([nx as f64, ny as f64]) as f32 + 1.0) * 0.5
    }
}

fn orographic_shadow(
    idx: usize,
    x: usize,
    y: usize,
    w: usize,
    smooth_elevation: &[f32],
    weight: f32,
) -> f32 {
    if x == 0 {
        return 0.0;
    }
    let up_idx = y * w + (x - 1);
    let uphill = (smooth_elevation[up_idx] - smooth_elevation[idx]).max(0.0);
    normalize01(uphill * weight)
}

/// Parallel rainfall from spatial noise, coast proximity, and local rain-shadow (no advection).
pub fn generate_rainfall(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;
    let sea = params.sea_level_norm;
    let orographic_weight = config.orographic_elevation_weight;
    let interior_drying = config.interior_drying_factor;
    let coastal_strength = config.continentality_strength;
    let rainfall_scale = config.rainfall_scale;

    let provisional_water: Vec<bool> = map.elevation.iter().map(|&e| e < sea).collect();
    let dist_to_water = chamfer_distance_water(w, h, &provisional_water);
    let ocean_range = params.continentality_ocean_range_cells.max(1) as f32;

    let blur_radius = (w.min(h) / 48).max(2);
    let smooth_elevation = box_blur(&map.elevation, w, h, blur_radius);
    let noise = RainfallNoise::new(config, params);
    let width_f = w as f32;
    let height_f = h as f32;

    map.rainfall
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / height_f,
                    "Simulating rainfall",
                );
            }
            let ny = y as f32 / height_f.max(1.0);

            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                if provisional_water[idx] {
                    *cell = 0.0;
                    continue;
                }

                let nx = x as f32 / width_f.max(1.0);
                let base = noise.sample(nx, ny);

                let dist_norm = (dist_to_water[idx] as f32 / ocean_range).clamp(0.0, 1.0);
                let coastal_boost = 1.0 - coastal_strength * dist_norm;
                let interior = dist_norm * interior_drying;
                let shadow = orographic_shadow(idx, x, y, w, &smooth_elevation, orographic_weight);

                *cell = normalize01(
                    base * coastal_boost * (1.0 - shadow) * (1.0 - interior) * rainfall_scale,
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldGenConfig;
    use crate::world::WorldMap;

    #[test]
    fn land_rainfall_in_unit_range() {
        let config = WorldGenConfig::test_config(42, 128);
        let params = config.resolve();
        let mut map = WorldMap::new(128, 128, config.seed);
        map.elevation.fill(params.sea_level_norm + 0.1);
        generate_rainfall(&mut map, &config, &params, &None, 0.0, 1.0);
        for (idx, &rain) in map.rainfall.iter().enumerate() {
            if map.elevation[idx] >= params.sea_level_norm {
                assert!(
                    (0.0..=1.0).contains(&rain),
                    "land rainfall out of range: {rain}"
                );
            }
        }
    }

    #[test]
    fn water_cells_have_zero_rainfall() {
        let config = WorldGenConfig::test_config(42, 64);
        let params = config.resolve();
        let mut map = WorldMap::new(64, 64, config.seed);
        map.elevation.fill(params.sea_level_norm - 0.1);
        generate_rainfall(&mut map, &config, &params, &None, 0.0, 1.0);
        assert!(map.rainfall.iter().all(|&r| r == 0.0));
    }

    #[test]
    fn coast_tends_wetter_than_deep_interior() {
        let config = WorldGenConfig::test_config(99, 256);
        let params = config.resolve();
        let mut map = WorldMap::new(256, 256, config.seed);
        let sea = params.sea_level_norm;
        map.elevation.fill(sea + 0.15);
        // West ocean strip so coast/interior distance bands exist on land.
        for y in 0..map.height {
            map.elevation[y * map.width] = sea - 0.2;
        }
        generate_rainfall(&mut map, &config, &params, &None, 0.0, 1.0);

        let w = map.width;
        let h = map.height;
        let provisional_water: Vec<bool> = map.elevation.iter().map(|&e| e < sea).collect();
        let dist_to_water = chamfer_distance_water(w, h, &provisional_water);
        let interior_min_dist = (params.continentality_ocean_range_cells / 2).max(64);

        let mut coast_sum = 0.0f32;
        let mut coast_n = 0usize;
        let mut interior_sum = 0.0f32;
        let mut interior_n = 0usize;

        for idx in 0..map.elevation.len() {
            if map.elevation[idx] < sea {
                continue;
            }
            let dist = dist_to_water[idx];
            if dist <= 8 {
                coast_sum += map.rainfall[idx];
                coast_n += 1;
            } else if dist >= interior_min_dist {
                interior_sum += map.rainfall[idx];
                interior_n += 1;
            }
        }

        assert!(
            coast_n > 0 && interior_n > 0,
            "need both coast and interior land cells"
        );
        let coast_mean = coast_sum / coast_n as f32;
        let interior_mean = interior_sum / interior_n as f32;
        assert!(
            coast_mean > interior_mean,
            "coast mean {coast_mean} should exceed interior mean {interior_mean}"
        );
    }
}
