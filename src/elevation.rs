use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{
    apply_land_fraction_target, ElevationEnvelopeConfig, ResolvedSimParams, WorldGenConfig,
};
use super::grid_ops::normalize01;
use super::progress::{report_stage, ProgressHandle};
use super::world::WorldMap;

fn edge_ocean_falloff(nx: f32, ny: f32, strength: f32) -> f32 {
    if strength <= 0.0 {
        return 0.0;
    }
    let dx = (nx - 0.5).abs() * 2.0;
    let dy = (ny - 0.5).abs() * 2.0;
    strength * dx.max(dy)
}

struct EnvelopeNoise {
    field: Fbm<Perlin>,
    config: ElevationEnvelopeConfig,
}

impl EnvelopeNoise {
    fn new(seed: u32, frequency: f64, config: &ElevationEnvelopeConfig) -> Self {
        Self {
            field: Fbm::<Perlin>::new(seed)
                .set_octaves(config.octaves.max(1) as usize)
                .set_frequency(frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            config: config.clone(),
        }
    }

    fn sample(&self, nx: f64, ny: f64) -> f32 {
        if !self.config.enabled {
            return 1.0;
        }
        let raw = normalize01((self.field.get([nx, ny]) as f32 + 1.0) * 0.5);
        self.config.floor + raw * self.config.strength
    }
}

struct ElevationNoise {
    continent: Fbm<Perlin>,
    detail: Fbm<Perlin>,
    ridge: Fbm<Perlin>,
    detail_envelope: EnvelopeNoise,
    ridge_envelope: EnvelopeNoise,
}

impl ElevationNoise {
    fn new(config: &WorldGenConfig, params: &ResolvedSimParams) -> Self {
        let seed = config.seed as u32;
        Self {
            continent: Fbm::<Perlin>::new(seed)
                .set_octaves(config.elevation_octaves as usize)
                .set_frequency(params.continent_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(config.elevation_persistence),
            detail: Fbm::<Perlin>::new(seed.wrapping_add(1))
                .set_octaves(3)
                .set_frequency(params.detail_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.45),
            ridge: Fbm::<Perlin>::new(seed.wrapping_add(2))
                .set_octaves(4)
                .set_frequency(params.detail_noise_frequency * 1.5)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            detail_envelope: EnvelopeNoise::new(
                seed.wrapping_add(11),
                params.detail_envelope_frequency,
                &config.elevation_detail_envelope,
            ),
            ridge_envelope: EnvelopeNoise::new(
                seed.wrapping_add(10),
                params.ridge_envelope_frequency,
                &config.elevation_ridge_envelope,
            ),
        }
    }

    fn sample(&self, nx: f64, ny: f64, w_cont: f32, w_detail: f32, w_ridge: f32) -> (f32, f32) {
        let continent01 = normalize01((self.continent.get([nx, ny]) as f32 + 1.0) * 0.5);
        let detail01 = normalize01((self.detail.get([nx, ny]) as f32 + 1.0) * 0.5);
        let ridge_raw = self.ridge.get([nx, ny]) as f32;
        let ridge01 = normalize01(1.0 - ridge_raw.abs());

        let env_detail = self.detail_envelope.sample(nx, ny);
        let env_ridge = self.ridge_envelope.sample(nx, ny);
        let ridge_influence = ridge01 * env_ridge;

        let wd = w_detail * env_detail;
        let base_denom = (w_cont + wd).max(0.001);
        let base = (w_cont * continent01 + wd * detail01) / base_denom;

        let elev = normalize01(base + w_ridge * ridge_influence);
        (elev, ridge_influence)
    }
}

/// Generate elevation from multi-scale FBM noise with optional land-fraction targeting.
pub fn generate_elevation(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;
    let noise = ElevationNoise::new(config, params);
    let w_cont = config.elevation_continent_weight;
    let w_detail = config.elevation_detail_weight;
    let w_ridge = config.elevation_ridge_weight;
    let edge_bias = config.edge_ocean_bias;

    map.elevation
        .par_chunks_mut(w)
        .zip(map.ridge_influence.par_chunks_mut(w))
        .enumerate()
        .for_each(|(y, (row, ridge_row))| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / h as f32,
                    "Generating elevation",
                );
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let nx = x as f64 / w as f64;
                let ny = y as f64 / h as f64;
                let (mut elev, ridge_influence) = noise.sample(nx, ny, w_cont, w_detail, w_ridge);
                ridge_row[x] = ridge_influence;
                elev -= edge_ocean_falloff(nx as f32, ny as f32, edge_bias);
                *cell = normalize01(elev);
            }
        });

    if let Some(target) = config.target_land_fraction {
        apply_land_fraction_target(&mut map.elevation, params.sea_level_norm, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ElevationEnvelopeConfig;
    use crate::generate_world;

    fn assert_range01(values: &[f32], name: &str) {
        for (i, &v) in values.iter().enumerate() {
            assert!((0.0..=1.0).contains(&v), "{name}[{i}] = {v} out of range");
        }
    }

    fn config_with_ridge_envelope(seed: u64, strength: f32) -> WorldGenConfig {
        WorldGenConfig {
            seed,
            width: 128,
            height: 128,
            target_land_fraction: None,
            edge_ocean_bias: 0.0,
            elevation_ridge_envelope: ElevationEnvelopeConfig {
                enabled: true,
                strength,
                ..ElevationEnvelopeConfig::ridge_default()
            },
            elevation_detail_envelope: ElevationEnvelopeConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn elevation_deterministic_with_envelopes() {
        let config = WorldGenConfig::test_config(99, 64);
        let mut a = WorldMap::new(config.width, config.height, config.seed);
        let mut b = WorldMap::new(config.width, config.height, config.seed);
        let params = config.resolve();
        generate_elevation(&mut a, &config, &params, &None, 0.0, 1.0);
        generate_elevation(&mut b, &config, &params, &None, 0.0, 1.0);
        assert_eq!(a.elevation, b.elevation);
        assert_eq!(a.ridge_influence, b.ridge_influence);
    }

    #[test]
    fn disabled_envelopes_produce_valid_range() {
        let mut config = WorldGenConfig::test_config(7, 64);
        config.elevation_ridge_envelope.enabled = false;
        config.elevation_detail_envelope.enabled = false;
        config.target_land_fraction = None;
        let mut map = WorldMap::new(config.width, config.height, config.seed);
        let params = config.resolve();
        generate_elevation(&mut map, &config, &params, &None, 0.0, 1.0);
        assert_range01(&map.elevation, "elevation");
    }

    #[test]
    fn ridge_envelope_modulates_elevation() {
        let flat = config_with_ridge_envelope(42, 0.0);
        let varied = config_with_ridge_envelope(42, 1.0);

        let map_flat = generate_world(&flat);
        let map_varied = generate_world(&varied);

        assert_ne!(
            map_flat.elevation, map_varied.elevation,
            "ridge envelope strength should change the elevation field"
        );

        let max_flat = map_flat
            .elevation
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        let max_varied = map_varied
            .elevation
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max_varied > max_flat,
            "ridge envelope should raise peak elevation: flat={max_flat}, varied={max_varied}"
        );
    }

    #[test]
    fn disabled_ridge_envelope_ignores_strength() {
        let mut a = config_with_ridge_envelope(42, 0.0);
        let mut b = config_with_ridge_envelope(42, 1.0);
        a.elevation_ridge_envelope.enabled = false;
        b.elevation_ridge_envelope.enabled = false;

        let map_a = generate_world(&a);
        let map_b = generate_world(&b);
        assert_eq!(map_a.elevation, map_b.elevation);
    }
}
