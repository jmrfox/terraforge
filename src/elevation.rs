//! World-space elevation with a sea level that does not depend on grid resolution.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use crate::config::{ElevationEnvelopeConfig, ResolvedSimParams, WorldGenConfig};
use crate::grid_ops::normalize01;
use crate::progress::{report_stage, ProgressHandle};
use crate::seeds::{stream_seed, NoiseStream};
use crate::world::WorldMap;

/// Samples per axis used to place sea level, independent of the map's resolution.
const SEA_LEVEL_LATTICE: usize = 256;

fn fbm(seed: u32, wavelength_m: f64, octaves: usize, persistence: f64) -> Fbm<Perlin> {
    Fbm::<Perlin>::new(seed)
        .set_octaves(octaves)
        .set_frequency(1.0 / wavelength_m)
        .set_lacunarity(2.0)
        .set_persistence(persistence)
}

fn unit(field: &Fbm<Perlin>, x_m: f64, y_m: f64) -> f32 {
    normalize01((field.get([x_m, y_m]) as f32 + 1.0) * 0.5)
}

struct Envelope {
    field: Fbm<Perlin>,
    config: ElevationEnvelopeConfig,
}

impl Envelope {
    fn new(seed: u64, stream: NoiseStream, config: &ElevationEnvelopeConfig) -> Self {
        Self {
            field: fbm(
                stream_seed(seed, stream),
                config.wavelength_m.0.max(1.0),
                config.octaves.max(1) as usize,
                0.5,
            ),
            config: config.clone(),
        }
    }

    fn sample(&self, x_m: f64, y_m: f64) -> f32 {
        if !self.config.enabled {
            return 1.0;
        }
        self.config.floor + unit(&self.field, x_m, y_m) * self.config.strength
    }
}

struct Terrain {
    continent: Fbm<Perlin>,
    detail: Fbm<Perlin>,
    ridge: Fbm<Perlin>,
    detail_envelope: Envelope,
    ridge_envelope: Envelope,
    continent_weight: f32,
    detail_weight: f32,
    ridge_weight: f32,
}

impl Terrain {
    fn new(config: &WorldGenConfig) -> Self {
        let seed = config.seed;
        let detail_m = config.detail_wavelength_m.0;
        Self {
            continent: fbm(
                stream_seed(seed, NoiseStream::Continent),
                config.continent_wavelength_m.0,
                config.elevation_octaves as usize,
                config.elevation_persistence,
            ),
            detail: fbm(stream_seed(seed, NoiseStream::Detail), detail_m, 3, 0.45),
            ridge: fbm(
                stream_seed(seed, NoiseStream::Ridge),
                detail_m / 1.5,
                4,
                0.5,
            ),
            detail_envelope: Envelope::new(
                seed,
                NoiseStream::DetailEnvelope,
                &config.elevation_detail_envelope,
            ),
            ridge_envelope: Envelope::new(
                seed,
                NoiseStream::RidgeEnvelope,
                &config.elevation_ridge_envelope,
            ),
            continent_weight: config.elevation_continent_weight,
            detail_weight: config.elevation_detail_weight,
            ridge_weight: config.elevation_ridge_weight,
        }
    }

    /// Unshifted elevation and ridge influence at a world position.
    fn sample(&self, x_m: f64, y_m: f64) -> (f32, f32) {
        let continent = unit(&self.continent, x_m, y_m);
        let detail = unit(&self.detail, x_m, y_m);
        let ridge = normalize01(1.0 - (self.ridge.get([x_m, y_m]) as f32).abs());
        let ridge_influence = ridge * self.ridge_envelope.sample(x_m, y_m);

        let detail_weight = self.detail_weight * self.detail_envelope.sample(x_m, y_m);
        let denom = (self.continent_weight + detail_weight).max(0.001);
        let base = (self.continent_weight * continent + detail_weight * detail) / denom;
        (base + self.ridge_weight * ridge_influence, ridge_influence)
    }
}

/// Elevation lowered within `margin_m` of the map edge, leaving the interior untouched.
fn edge_falloff(x_m: f64, y_m: f64, width_m: f64, height_m: f64, config: &WorldGenConfig) -> f32 {
    let margin_m = config.edge_ocean_margin_m.0;
    if config.edge_ocean_bias <= 0.0 || margin_m <= 0.0 {
        return 0.0;
    }
    let to_edge = x_m.min(width_m - x_m).min(y_m).min(height_m - y_m);
    if to_edge >= margin_m {
        return 0.0;
    }
    let t = (1.0 - to_edge / margin_m) as f32;
    config.edge_ocean_bias * t * t
}

/// Elevation shift that puts the target land fraction above sea level.
fn sea_level_shift(terrain: &Terrain, config: &WorldGenConfig, params: &ResolvedSimParams) -> f32 {
    let Some(target) = config.target_land_fraction else {
        return 0.0;
    };
    // A fixed reference ignores this map's edges so every map size gets the same shift.
    let (span_x, span_y, use_map_edges) = match config.land_reference_extent_m {
        Some(extent) => (extent.0, extent.0, false),
        None => (params.map_width_m, params.map_height_m, true),
    };
    let n = SEA_LEVEL_LATTICE;
    let mut samples: Vec<f32> = (0..n * n)
        .into_par_iter()
        .map(|i| {
            let x_m = ((i % n) as f64 + 0.5) / n as f64 * span_x;
            let y_m = ((i / n) as f64 + 0.5) / n as f64 * span_y;
            let falloff = if use_map_edges {
                edge_falloff(x_m, y_m, span_x, span_y, config)
            } else {
                0.0
            };
            terrain.sample(x_m, y_m).0 - falloff
        })
        .collect();
    samples.sort_by(f32::total_cmp);
    let target = target.clamp(0.01, 0.99);
    let idx = (((1.0 - target) * samples.len() as f32) as usize).min(samples.len() - 1);
    samples[idx] - params.sea_level_norm
}

pub fn generate(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let (w, h) = (map.width, map.height);
    let cell = params.cell_size_m;
    let terrain = Terrain::new(config);
    let shift = sea_level_shift(&terrain, config, params);

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
            let y_m = (y as f64 + 0.5) * cell;
            for (x, cell_value) in row.iter_mut().enumerate() {
                let x_m = (x as f64 + 0.5) * cell;
                let (raw, ridge_influence) = terrain.sample(x_m, y_m);
                let falloff =
                    edge_falloff(x_m, y_m, params.map_width_m, params.map_height_m, config);
                ridge_row[x] = ridge_influence;
                *cell_value = normalize01(raw - falloff - shift);
            }
        });
}
