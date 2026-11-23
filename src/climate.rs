//! Regional climate in physical units.
//!
//! Temperature comes from latitude, elevation, and local variation. Rainfall carries
//! moisture along the prevailing wind: air recharges over water, rains as it travels,
//! and loses moisture when rising terrain cools it below saturation, which leaves a
//! drier lee side.
//!
//! Transcendental math goes through `libm` because the standard library calls each
//! platform's math library, whose last bits differ, and saved maps must match everywhere.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use crate::config::WorldGenConfig;
use crate::grid_ops::box_blur;
use crate::progress::{report_stage, ProgressHandle};
use crate::seeds::{stream_seed, NoiseStream};
use crate::world::WorldMap;

/// °C mapped to 0 in the normalized `WorldMap::temperature` layer.
pub const TEMPERATURE_NORM_MIN_C: f32 = -15.0;
/// °C mapped to 1 in the normalized `WorldMap::temperature` layer.
pub const TEMPERATURE_NORM_MAX_C: f32 = 35.0;
/// Annual rainfall in mm mapped to 1 in the normalized `WorldMap::rainfall` layer.
pub const RAINFALL_NORM_MAX_MM: f32 = 2000.0;

const METERS_PER_DEGREE_LATITUDE: f64 = 111_320.0;
/// Fits Earth's zonal mean temperatures better than a plain cosine.
const LATITUDE_PROFILE_EXPONENT: f64 = 1.3;
/// Saturation humidity gains about 6% per °C (Clausius-Clapeyron).
const HUMIDITY_GROWTH_PER_C: f64 = 0.06;
const OCEAN_RELATIVE_HUMIDITY: f64 = 0.85;
const OCEAN_RECHARGE_LENGTH_M: f64 = 150_000.0;
/// Share of land rainfall that evaporates back into the passing air.
const MOISTURE_RECYCLING: f64 = 0.5;
/// How far condensed water drifts downwind before reaching the ground.
const CONDENSATE_FALL_LENGTH_M: f64 = 30_000.0;
const SUBTROPICAL_LATITUDE_DEG: f64 = 27.0;
const SUBTROPICAL_WIDTH_DEG: f64 = 10.0;

fn latitude_at(config: &WorldGenConfig, y: usize) -> f64 {
    let north_of_center = (config.height as f64 * 0.5 - (y as f64 + 0.5)) * config.cell_size_m.0;
    (config.regional_climate.latitude_deg + north_of_center / METERS_PER_DEGREE_LATITUDE)
        .clamp(-90.0, 90.0)
}

fn sea_level_temperature(config: &WorldGenConfig, latitude_deg: f64) -> f64 {
    let c = &config.regional_climate;
    let profile = libm::pow(
        libm::cos(latitude_deg.to_radians()).max(0.0),
        LATITUDE_PROFILE_EXPONENT,
    );
    c.pole_temperature_c
        + (c.equator_temperature_c - c.pole_temperature_c) * profile
        + c.temperature_offset_c
}

fn subtropical_factor(drying: f64, latitude_deg: f64) -> f64 {
    let d = (latitude_deg.abs() - SUBTROPICAL_LATITUDE_DEG) / SUBTROPICAL_WIDTH_DEG;
    1.0 - drying * libm::exp(-d * d)
}

/// Unit vector the wind blows toward, with +y pointing south.
fn wind_vector(from_deg: f64) -> (f64, f64) {
    let toward = (from_deg + 180.0).to_radians();
    (libm::sin(toward), -libm::cos(toward))
}

fn saturation_humidity(temperature_c: f64) -> f64 {
    libm::exp(HUMIDITY_GROWTH_PER_C * (temperature_c - 15.0))
}

struct MoistureModel {
    wind: (f64, f64),
    cell_size_m: f64,
    base_rainfall_mm: f64,
    rainout_length_m: f64,
    orographic_efficiency: f64,
}

impl MoistureModel {
    /// Annual rainfall per cell from a steady upwind sweep along the wind.
    fn simulate(
        &self,
        w: usize,
        h: usize,
        air_temperature_c: &[f32],
        is_water: &[bool],
    ) -> Vec<f32> {
        let (ux, uy) = self.wind;
        let total = (ux.abs() + uy.abs()).max(1e-9);
        let (weight_x, weight_y) = (ux.abs() / total, uy.abs() / total);
        // Distance along the wind between a cell and its blended upwind point.
        let step_m = self.cell_size_m / total;

        let columns: Vec<usize> = if ux >= 0.0 {
            (0..w).collect()
        } else {
            (0..w).rev().collect()
        };
        let rows: Vec<usize> = if uy >= 0.0 {
            (0..h).collect()
        } else {
            (0..h).rev().collect()
        };
        let upwind_x = |x: usize| {
            if ux >= 0.0 {
                x.checked_sub(1)
            } else {
                (x + 1 < w).then_some(x + 1)
            }
        };
        let upwind_y = |y: usize| {
            if uy >= 0.0 {
                y.checked_sub(1)
            } else {
                (y + 1 < h).then_some(y + 1)
            }
        };

        let recharge = 1.0 - libm::exp(-step_m / OCEAN_RECHARGE_LENGTH_M);
        let fall = 1.0 - libm::exp(-step_m / CONDENSATE_FALL_LENGTH_M);
        // Moisture flux that yields the base rainfall when depleted over the rainout length.
        let rain_per_moisture = self.base_rainfall_mm * self.rainout_length_m / step_m;

        let mut moisture = vec![0.0f64; w * h];
        let mut condensate = vec![0.0f64; w * h];
        let mut rainfall = vec![0.0f32; w * h];

        for &y in &rows {
            for &x in &columns {
                let idx = y * w + x;
                let capacity = saturation_humidity(f64::from(air_temperature_c[idx]));
                let inflow = OCEAN_RELATIVE_HUMIDITY * capacity;

                let from_x = upwind_x(x).map(|ux| y * w + ux);
                let from_y = upwind_y(y).map(|uy| uy * w + x);
                let blend = |field: &[f64], edge: f64| {
                    weight_x * from_x.map_or(edge, |i| field[i])
                        + weight_y * from_y.map_or(edge, |i| field[i])
                };
                let mut m = blend(&moisture, inflow);
                let mut c = blend(&condensate, 0.0);

                if is_water[idx] {
                    m += (OCEAN_RELATIVE_HUMIDITY * capacity - m) * recharge;
                }

                let relative_humidity = (m / capacity).min(1.0);
                let background = self.base_rainfall_mm * m * relative_humidity;
                m *= libm::exp(
                    -(1.0 - MOISTURE_RECYCLING) * relative_humidity * step_m
                        / self.rainout_length_m,
                );

                let condensed = (m - capacity).max(0.0);
                m -= condensed;
                c += condensed;
                let falling = c * fall;
                c -= falling;

                moisture[idx] = m;
                condensate[idx] = c;
                let orographic = self.orographic_efficiency * falling * rain_per_moisture;
                rainfall[idx] = (background + orographic) as f32;
            }
        }
        rainfall
    }
}

pub fn generate(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let (w, h) = (map.width, map.height);
    let cell = config.cell_size_m.0;
    let climate = &config.regional_climate;
    let lapse_c_per_m = config.lapse_rate_c_per_km / 1000.0;

    let noise = |stream, seed| {
        Fbm::<Perlin>::new(stream_seed(seed, stream))
            .set_octaves(4)
            .set_frequency(1.0 / config.temperature_wavelength_m.0)
            .set_lacunarity(2.0)
            .set_persistence(0.5)
    };
    let temperature_noise = noise(NoiseStream::Temperature, config.seed);
    let rainfall_noise = noise(NoiseStream::Rainfall, config.seed);

    let latitudes: Vec<f64> = (0..h).map(|y| latitude_at(config, y)).collect();
    let sea_temperatures: Vec<f64> = latitudes
        .iter()
        .map(|&lat| sea_level_temperature(config, lat))
        .collect();

    let height_above_sea: Vec<f32> = (0..w * h)
        .map(|i| (map.elevation_m(i) - config.sea_level_m.0).max(0.0) as f32)
        .collect();

    let mut temperature_c = vec![0.0f32; w * h];
    temperature_c
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    0.4 * y as f32 / h as f32,
                    "Simulating temperature",
                );
            }
            let y_m = (y as f64 + 0.5) * cell;
            for (x, value) in row.iter_mut().enumerate() {
                let x_m = (x as f64 + 0.5) * cell;
                let local = climate.temperature_variation_c * temperature_noise.get([x_m, y_m]);
                let lapse = lapse_c_per_m * f64::from(height_above_sea[y * w + x]);
                *value = (sea_temperatures[y] + local - lapse) as f32;
            }
        });

    report_stage(progress, stage_start, stage_end, 0.4, "Simulating rainfall");
    let radius = (climate.orographic_smoothing_m.0 / cell).round() as usize;
    let smooth_height = box_blur(&height_above_sea, w, h, radius.min(w.max(h)));
    let air_temperature_c: Vec<f32> = (0..w * h)
        .map(|i| (sea_temperatures[i / w] - lapse_c_per_m * f64::from(smooth_height[i])) as f32)
        .collect();

    let model = MoistureModel {
        wind: wind_vector(climate.wind_from_deg),
        cell_size_m: cell,
        base_rainfall_mm: climate.base_rainfall_mm,
        rainout_length_m: climate.rainout_length_m.0,
        orographic_efficiency: climate.orographic_efficiency,
    };
    let mut rainfall_mm = model.simulate(w, h, &air_temperature_c, &map.water_mask);

    rainfall_mm
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            let belt = subtropical_factor(climate.subtropical_drying, latitudes[y]);
            let y_m = (y as f64 + 0.5) * cell;
            for (x, value) in row.iter_mut().enumerate() {
                let x_m = (x as f64 + 0.5) * cell;
                let local = 1.0 + climate.rainfall_variation * rainfall_noise.get([x_m, y_m]);
                let scaled =
                    f64::from(*value) * f64::from(config.rainfall_scale) * belt * local.max(0.0);
                *value = scaled.max(0.0) as f32;
            }
        });

    for i in 0..w * h {
        map.temperature[i] = normalized_temperature(temperature_c[i]);
        map.rainfall[i] = normalized_rainfall(rainfall_mm[i], map.water_mask[i]);
    }
    map.temperature_c = temperature_c;
    map.rainfall_mm = rainfall_mm;
}

/// Normalized `WorldMap::temperature` value for a temperature in °C.
pub(crate) fn normalized_temperature(temperature_c: f32) -> f32 {
    let span = TEMPERATURE_NORM_MAX_C - TEMPERATURE_NORM_MIN_C;
    ((temperature_c - TEMPERATURE_NORM_MIN_C) / span).clamp(0.0, 1.0)
}

/// Normalized `WorldMap::rainfall` value, which is zero on water.
pub(crate) fn normalized_rainfall(rainfall_mm: f32, is_water: bool) -> f32 {
    if is_water {
        0.0
    } else {
        (rainfall_mm / RAINFALL_NORM_MAX_MM).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH_KM: f64 = 400.0;

    /// East-west transect with an ocean, then a north-south ridge peaking at 3 km.
    fn ridge_rain(cell_m: f64, from_deg: f64) -> (Vec<f32>, usize) {
        let w = (WIDTH_KM * 1000.0 / cell_m) as usize;
        let h = 8;
        let (mut air, mut water) = (vec![0.0f32; w * h], vec![false; w * h]);
        for y in 0..h {
            for x in 0..w {
                let km = (x as f64 + 0.5) * cell_m / 1000.0;
                let ridge = 3000.0 * (-((km - 200.0) / 25.0).powi(2)).exp();
                air[y * w + x] = (18.0 - 0.0065 * ridge) as f32;
                water[y * w + x] = !(40.0..360.0).contains(&km);
            }
        }
        let model = MoistureModel {
            wind: wind_vector(from_deg),
            cell_size_m: cell_m,
            base_rainfall_mm: 1400.0,
            rainout_length_m: 800_000.0,
            orographic_efficiency: 0.3,
        };
        (model.simulate(w, h, &air, &water), w)
    }

    fn rain_at(rain: &[f32], w: usize, cell_m: f64, km: f64) -> f32 {
        let x = (km * 1000.0 / cell_m) as usize;
        rain[4 * w + x.min(w - 1)]
    }

    #[test]
    fn ridge_casts_a_rain_shadow_downwind() {
        let (rain, w) = ridge_rain(1000.0, 270.0);
        let windward = rain_at(&rain, w, 1000.0, 185.0);
        let lee = rain_at(&rain, w, 1000.0, 260.0);
        assert!(windward > 1.5 * lee, "windward {windward}, lee {lee}");
    }

    #[test]
    fn reversing_wind_moves_the_shadow() {
        let (rain, w) = ridge_rain(1000.0, 90.0);
        let west = rain_at(&rain, w, 1000.0, 140.0);
        let east = rain_at(&rain, w, 1000.0, 215.0);
        assert!(east > 1.5 * west, "east {east}, west {west}");
    }

    #[test]
    fn rainfall_is_stable_across_cell_sizes() {
        let (a, wa) = ridge_rain(1000.0, 270.0);
        let (b, wb) = ridge_rain(500.0, 270.0);
        for km in [80.0, 150.0, 185.0, 200.0, 240.0, 300.0] {
            let (ra, rb) = (rain_at(&a, wa, 1000.0, km), rain_at(&b, wb, 500.0, km));
            let relative = (ra - rb).abs() / ra.max(1.0);
            assert!(relative < 0.08, "{km} km: {ra} vs {rb}");
        }
    }

    #[test]
    fn wind_vector_follows_compass() {
        let (ux, uy) = wind_vector(270.0);
        assert!((ux - 1.0).abs() < 1e-9 && uy.abs() < 1e-9, "westerly");
        let (ux, uy) = wind_vector(0.0);
        assert!(
            ux.abs() < 1e-9 && (uy - 1.0).abs() < 1e-9,
            "northerly blows south"
        );
    }

    #[test]
    fn subtropics_are_driest_near_27_degrees() {
        assert!(subtropical_factor(0.6, 27.0) < subtropical_factor(0.6, 5.0));
        assert!(subtropical_factor(0.6, -27.0) < subtropical_factor(0.6, -55.0));
        assert_eq!(subtropical_factor(0.0, 27.0), 1.0);
    }
}
