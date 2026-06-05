use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::WorldGenConfig;
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

// Exponent < 1.0 keeps mid-latitudes warmer and shrinks polar cold bands.
const LATITUDE_SOFTENING: f32 = 0.58;

fn normalize01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

struct ClimateNoise {
    field: Fbm<Perlin>,
}

impl ClimateNoise {
    fn new(config: &WorldGenConfig) -> Self {
        Self {
            field: Fbm::<Perlin>::new((config.seed as u32).wrapping_add(4))
                .set_octaves(4)
                .set_frequency(1.6)
                .set_lacunarity(2.0)
                .set_persistence(0.45),
        }
    }

    // Organic local variation (replaces sine wobble that caused zigzag biome edges).
    fn sample(&self, nx: f32, ny: f32) -> f32 {
        (self.field.get([nx as f64 * 3.0, ny as f64 * 3.0]) as f32) * 0.16
    }
}

/// Latitude gradient (hot equator) plus elevation cooling.
pub fn generate_temperature(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let h = map.height as f32;
    let equator = h * 0.5;
    let climate = ClimateNoise::new(config);

    let width = map.width;
    let height = map.height;
    let water_mask = map.water_mask.as_slice();
    let elevation = map.elevation.as_slice();
    let temperature_scale = config.temperature_scale;
    let elevation_cooling_factor = config.elevation_cooling_factor;

    map.temperature
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / height as f32,
                    "Simulating temperature",
                );
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * width + x;
                if water_mask[idx] {
                    *cell = 0.5;
                    continue;
                }

                let nx = x as f32 / width as f32;
                let ny = y as f32 / height as f32;
                let lat_dist = ((y as f32 + 0.5) - equator).abs() / equator;
                let latitude_temp =
                    (1.0 - lat_dist.powf(LATITUDE_SOFTENING) + climate.sample(nx, ny))
                        .clamp(0.0, 1.0);
                let mut temp = latitude_temp * temperature_scale;
                temp -= elevation[idx] * elevation_cooling_factor;
                *cell = normalize01(temp);
            }
        });
}
