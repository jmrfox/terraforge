use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

fn normalize01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

struct TemperatureNoise {
    field: Fbm<Perlin>,
}

impl TemperatureNoise {
    fn new(config: &WorldGenConfig, params: &ResolvedSimParams) -> Self {
        Self {
            field: Fbm::<Perlin>::new((config.seed as u32).wrapping_add(4))
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

/// Macro temperature from spatial noise (pole/equator config sets the value range), plus
/// elevation cooling and continentality modifiers.
pub fn generate_temperature(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let climate = TemperatureNoise::new(config, params);
    let dist_to_ocean = &map.dist_to_water;
    let ocean_range = params.continentality_ocean_range_cells.max(1) as f32;
    let continentality = config.continentality_strength;

    let width = map.width;
    let height = map.height;
    let water_mask = map.water_mask.as_slice();
    let elevation = map.elevation.as_slice();
    let elevation_cooling_factor = params.elevation_cooling_factor;

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
                let nx = x as f32 / width as f32;
                let ny = y as f32 / height as f32;
                let mut temp = climate.sample(nx, ny);

                if !water_mask[idx] {
                    temp -= elevation[idx] * elevation_cooling_factor;
                    let ocean_prox = (dist_to_ocean[idx] as f32 / ocean_range).clamp(0.0, 1.0);
                    let continental_factor = (1.0 - ocean_prox) * continentality;
                    temp -= continental_factor * 0.08;
                }

                *cell = normalize01(temp);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_world;

    #[test]
    fn temperature_is_not_latitude_driven() {
        let config = WorldGenConfig {
            width: 256,
            height: 256,
            seed: 42,
            ..Default::default()
        };
        let map = generate_world(&config);
        let w = map.width;
        let h = map.height;

        let row_mean = |y: usize| -> f32 {
            let row = &map.temperature[y * w..(y + 1) * w];
            row.iter().sum::<f32>() / row.len() as f32
        };

        let top = row_mean(0);
        let mid = row_mean(h / 2);
        let bottom = row_mean(h - 1);
        assert!(
            (top - bottom).abs() < 0.25,
            "latitude gradient removed: top={top:.3} bottom={bottom:.3}"
        );
        assert!(
            (mid - top).abs() > 0.02 || (mid - bottom).abs() > 0.02,
            "temperature field should vary spatially: top={top:.3} mid={mid:.3} bottom={bottom:.3}"
        );
    }
}
