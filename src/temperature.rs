use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
use super::grid_ops::normalize01;
use super::progress::{report_stage, ProgressHandle};
use super::world::WorldMap;

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

/// Temperature from spatial noise minus elevation lapse (no latitude/longitude).
///
/// Sea-level cells can still be cold when the noise field is low; high terrain is
/// always colder than the noise baseline at the same horizontal location.
pub fn generate_temperature(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;
    let climate = TemperatureNoise::new(config, params);
    let sea = params.sea_level_norm;
    let cooling = params.elevation_cooling_factor;
    let width_f = w as f32;
    let height_f = h as f32;
    let elevation = map.elevation.as_slice();

    map.temperature
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / height_f,
                    "Simulating temperature",
                );
            }
            let ny = y as f32 / height_f.max(1.0);

            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                let nx = x as f32 / width_f.max(1.0);

                let noise = climate.sample(nx, ny);
                let elev_above_sea = (elevation[idx] - sea).max(0.0);
                *cell = normalize01(noise - elev_above_sea * cooling);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldGenConfig;
    use crate::world::WorldMap;

    #[test]
    fn sea_level_can_be_cold_from_noise() {
        let config = WorldGenConfig::test_config(42, 128);
        let params = config.resolve();
        let mut map = WorldMap::new(128, 128, config.seed);
        map.elevation.fill(params.sea_level_norm);
        generate_temperature(&mut map, &config, &params, &None, 0.0, 1.0);
        let min = map
            .temperature
            .iter()
            .cloned()
            .fold(f32::INFINITY, f32::min);
        let max = map
            .temperature
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            min < 0.5,
            "noise should produce cold sea-level cells, min={min}"
        );
        assert!(
            max > 0.5,
            "noise should produce warm sea-level cells, max={max}"
        );
    }

    #[test]
    fn high_elevation_cooler_than_nearby_low_at_same_noise_scale() {
        let config = WorldGenConfig::test_config(42, 64);
        let params = config.resolve();
        let mut map = WorldMap::new(64, 64, config.seed);
        map.elevation.fill(params.sea_level_norm);
        let peak = map.index(32, 32);
        map.elevation[peak] = 0.95;
        generate_temperature(&mut map, &config, &params, &None, 0.0, 1.0);
        let low = map.temperature[map.index(10, 10)];
        let high = map.temperature[peak];
        assert!(high < low, "peak should be colder: high={high} low={low}");
    }
}
