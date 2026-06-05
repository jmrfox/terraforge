use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{WindDirection, WorldGenConfig};
use super::progress::{ProgressHandle, report_stage};
use super::world::{Biome, WorldMap};

fn normalize01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Simplified prevailing-wind moisture transport model.
pub fn generate_rainfall(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;
    let mountain_orographic = 0.35;
    let rain_noise = Fbm::<Perlin>::new((config.seed as u32).wrapping_add(5))
        .set_octaves(3)
        .set_frequency(2.2)
        .set_lacunarity(2.0)
        .set_persistence(0.4);

    match config.wind_direction {
        WindDirection::WestToEast => {
            let water_mask = map.water_mask.clone();
            let biome = map.biome.clone();
            let elevation = map.elevation.clone();
            let rainfall_scale = config.rainfall_scale;
            let width_f = w as f32;

            map.rainfall
                .par_chunks_mut(w)
                .enumerate()
                .for_each(|(y, row)| {
                    if y % 8 == 0 {
                        report_stage(
                            progress,
                            stage_start,
                            stage_end,
                            y as f32 / h as f32,
                            "Simulating rainfall",
                        );
                    }
                    let mut moisture = 0.0f32;
                    for (x, cell) in row.iter_mut().enumerate() {
                        let idx = y * w + x;
                        if water_mask[idx] {
                            if biome[idx] == Biome::Ocean {
                                moisture = 1.0;
                            }
                            *cell = 0.0;
                            continue;
                        }

                        let orographic_loss = elevation[idx] * mountain_orographic;
                        moisture = (moisture - orographic_loss * 0.15).max(0.0);

                        if x == 0 && biome[idx] == Biome::Ocean {
                            moisture = 1.0;
                        }

                        if moisture < 0.5 {
                            let west_ocean =
                                x > 0 && biome[y * w + (x - 1)] == Biome::Ocean;
                            if west_ocean {
                                moisture = moisture.max(0.85);
                            }
                        }

                        let nx = x as f32 / width_f;
                        let ny = y as f32 / h as f32;
                        let local_rain =
                            rain_noise.get([nx as f64 * 3.5, ny as f64 * 3.5]) as f32 * 0.10;
                        *cell = normalize01(moisture * rainfall_scale + local_rain);
                        moisture *= 0.92;
                    }
                });
        }
    }
}
