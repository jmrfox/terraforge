use serde::{Deserialize, Serialize};

use crate::units::{wavelength_to_frequency, Degrees, Meters, SquareMeters};

/// Spatial envelope modulating where an elevation noise layer applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ElevationEnvelopeConfig {
    pub enabled: bool,
    /// Macro wavelength of the envelope field (much larger than layer noise).
    pub wavelength_m: Meters,
    pub octaves: u32,
    /// Remap raw noise `[0, 1]` to `[floor, floor + strength]`.
    pub floor: f32,
    pub strength: f32,
}

impl Default for ElevationEnvelopeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            wavelength_m: Meters(16_000.0),
            octaves: 3,
            floor: 0.0,
            strength: 1.0,
        }
    }
}

impl ElevationEnvelopeConfig {
    pub fn ridge_default() -> Self {
        Self {
            enabled: true,
            wavelength_m: Meters(16_000.0),
            floor: 0.08,
            strength: 0.92,
            ..Self::default()
        }
    }
}

/// User-facing configuration in physical units where applicable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldGenConfig {
    pub seed: u64,
    pub width: usize,
    pub height: usize,

    // --- Grid and vertical datum ---
    pub cell_size_m: Meters,
    pub max_elevation_m: Meters,
    pub sea_level_m: Meters,
    pub ocean_floor_m: Meters,

    // --- Elevation noise ---
    /// Macro continent wavelength (alias for continent scale).
    pub elevation_wavelength_m: Meters,
    pub continent_wavelength_m: Meters,
    pub detail_wavelength_m: Meters,
    pub elevation_octaves: u32,
    pub elevation_persistence: f64,
    pub elevation_continent_weight: f32,
    pub elevation_detail_weight: f32,
    pub elevation_ridge_weight: f32,
    pub elevation_detail_envelope: ElevationEnvelopeConfig,
    pub elevation_ridge_envelope: ElevationEnvelopeConfig,
    /// When set, elevation is shifted so this land fraction emerges at `sea_level_norm`.
    pub target_land_fraction: Option<f32>,
    /// Subtract elevation near map edges (0 = off) to encourage edge-connected ocean.
    pub edge_ocean_bias: f32,

    // --- Water ---
    pub min_lake_area_m2: SquareMeters,

    // --- Climate ---
    /// Temperature span (°C) used to normalize elevation lapse into simulation units.
    pub temperature_range_c: f64,
    pub lapse_rate_c_per_km: f64,
    pub rainfall_scale: f32,
    pub temperature_wavelength_m: Meters,
    pub continentality_strength: f32,
    pub continentality_ocean_range_m: Meters,
    pub orographic_elevation_weight: f32,
    pub interior_drying_factor: f32,

    // --- Biomes ---
    pub mountain_min_elevation_m: Meters,
    pub mountain_min_slope_deg: Degrees,
    /// Minimum ridge_influence (ridge noise × envelope) required for Mountain biome.
    pub mountain_min_ridge_influence: f32,
}

/// Simulation-internal parameters derived from physical config.
#[derive(Debug, Clone)]
pub struct ResolvedSimParams {
    pub cell_size_m: f64,
    pub map_width_m: f64,
    pub map_height_m: f64,
    pub max_elevation_m: f64,
    pub sea_level_m: f64,
    pub ocean_floor_m: f64,
    pub sea_level_norm: f32,
    pub min_lake_cells: usize,

    pub elevation_cooling_factor: f32,

    pub continent_noise_frequency: f64,
    pub detail_noise_frequency: f64,
    pub temperature_noise_frequency: f64,
    pub detail_envelope_frequency: f64,
    pub ridge_envelope_frequency: f64,

    pub continentality_ocean_range_cells: u32,
    pub mountain_elev_norm: f32,
    /// Minimum normalized elevation delta between land neighbors (not tan of degrees).
    pub mountain_slope_norm: f32,
    pub mountain_min_ridge_influence: f32,
}

impl WorldGenConfig {
    pub fn effective_continent_wavelength_m(&self) -> Meters {
        if self.continent_wavelength_m.0 > 0.0 {
            self.continent_wavelength_m
        } else {
            self.elevation_wavelength_m
        }
    }

    pub fn resolve(&self) -> ResolvedSimParams {
        let cell = self.cell_size_m.0;
        let map_width_m = self.width as f64 * cell;
        let map_height_m = self.height as f64 * cell;
        let max_elev = self.max_elevation_m.0;
        let ocean_floor = self.ocean_floor_m.0;
        let elev_span = (max_elev - ocean_floor).max(1.0);

        let sea_level_norm =
            (((self.sea_level_m.0 - ocean_floor) / elev_span) as f32).clamp(0.0, 1.0);

        let temp_range_c = self.temperature_range_c.max(1.0);
        let elevation_cooling_factor =
            ((self.lapse_rate_c_per_km / 1000.0 * max_elev) / temp_range_c) as f32;

        let mountain_elev_norm =
            (((self.mountain_min_elevation_m.0 - ocean_floor) / elev_span) as f32).clamp(0.0, 1.0);
        let slope_tan = self.mountain_min_slope_deg.to_radians().tan();
        let mountain_slope_norm = (slope_tan * cell / elev_span).clamp(0.0, 1.0) as f32;

        ResolvedSimParams {
            cell_size_m: cell,
            map_width_m,
            map_height_m,
            max_elevation_m: max_elev,
            sea_level_m: self.sea_level_m.0,
            ocean_floor_m: ocean_floor,
            sea_level_norm,
            min_lake_cells: self.min_lake_area_m2.to_cell_count(cell).max(1),

            elevation_cooling_factor,

            continent_noise_frequency: wavelength_to_frequency(
                map_width_m,
                self.effective_continent_wavelength_m(),
            ),
            detail_noise_frequency: wavelength_to_frequency(map_width_m, self.detail_wavelength_m),
            temperature_noise_frequency: wavelength_to_frequency(
                map_width_m,
                self.temperature_wavelength_m,
            ),
            detail_envelope_frequency: wavelength_to_frequency(
                map_width_m,
                self.elevation_detail_envelope.wavelength_m,
            ),
            ridge_envelope_frequency: wavelength_to_frequency(
                map_width_m,
                self.elevation_ridge_envelope.wavelength_m,
            ),

            continentality_ocean_range_cells: self
                .continentality_ocean_range_m
                .to_cells(cell)
                .max(1),
            mountain_elev_norm,
            mountain_slope_norm,
            mountain_min_ridge_influence: self.mountain_min_ridge_influence,
        }
    }

    /// Suggest a physical sea level (meters) that yields approximately `target_land_fraction`
    /// land on a preview elevation field. Editor/workflow utility — not used during generation.
    pub fn suggest_sea_level_m_for_fraction(
        &self,
        elevation: &[f32],
        target_land_fraction: f32,
    ) -> Meters {
        let elev_span = (self.max_elevation_m.0 - self.ocean_floor_m.0).max(1.0);
        let norm = calibrate_sea_level_norm(elevation, target_land_fraction);
        let meters = self.ocean_floor_m.0 + norm as f64 * elev_span;
        Meters(meters.clamp(self.ocean_floor_m.0, self.max_elevation_m.0))
    }

    #[cfg(test)]
    pub fn test_config(seed: u64, size: usize) -> Self {
        Self {
            seed,
            width: size,
            height: size,
            target_land_fraction: Some(0.35),
            ..Default::default()
        }
    }
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            width: 512,
            height: 512,

            cell_size_m: Meters(20.0),
            max_elevation_m: Meters(9000.0),
            sea_level_m: Meters(0.0),
            ocean_floor_m: Meters(-6000.0),

            elevation_wavelength_m: Meters(5120.0),
            continent_wavelength_m: Meters(5120.0),
            detail_wavelength_m: Meters(1024.0),
            elevation_octaves: 4,
            elevation_persistence: 0.5,
            elevation_continent_weight: 0.65,
            elevation_detail_weight: 0.25,
            elevation_ridge_weight: 0.23,
            elevation_detail_envelope: ElevationEnvelopeConfig::default(),
            elevation_ridge_envelope: ElevationEnvelopeConfig::ridge_default(),
            target_land_fraction: Some(0.35),
            edge_ocean_bias: 0.12,

            min_lake_area_m2: SquareMeters(9600.0),

            temperature_range_c: 65.0,
            lapse_rate_c_per_km: 6.5,
            rainfall_scale: 0.90,
            temperature_wavelength_m: Meters(12_000.0),
            continentality_strength: 0.08,
            continentality_ocean_range_m: Meters(7000.0),
            orographic_elevation_weight: 0.48,
            interior_drying_factor: 0.16,

            mountain_min_elevation_m: Meters(2900.0),
            mountain_min_slope_deg: Degrees(4.0),
            mountain_min_ridge_influence: 0.06,
        }
    }
}

/// Binary-search normalized sea level for a target land fraction on a preview elevation field.
pub fn calibrate_sea_level_norm(elevation: &[f32], target_land_fraction: f32) -> f32 {
    let len = elevation.len().max(1) as f32;
    let target = target_land_fraction.clamp(0.0, 1.0);
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;

    for _ in 0..28 {
        let mid = (lo + hi) * 0.5;
        let land = elevation.iter().filter(|&&e| e >= mid).count() as f32 / len;
        if land > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

/// Shift elevation so `target_land_fraction` of cells lie at or above `sea_level_norm`.
pub fn apply_land_fraction_target(
    elevation: &mut [f32],
    sea_level_norm: f32,
    target_land_fraction: f32,
) {
    if elevation.is_empty() {
        return;
    }
    let target = target_land_fraction.clamp(0.01, 0.99);
    let mut sorted: Vec<f32> = elevation.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((1.0 - target) * sorted.len() as f32).floor() as usize;
    let idx = idx.min(sorted.len() - 1);
    let threshold = sorted[idx];
    let shift = threshold - sea_level_norm;
    for e in elevation.iter_mut() {
        *e = (*e - shift).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_default_cell_counts() {
        let config = WorldGenConfig::default();
        let p = config.resolve();
        assert!((p.cell_size_m - 20.0).abs() < f64::EPSILON);
        assert_eq!(p.min_lake_cells, 24);
    }

    #[test]
    fn resolve_elevation_noise_frequency() {
        let p = WorldGenConfig::default().resolve();
        assert!((p.continent_noise_frequency - 2.0).abs() < 0.01);
        assert!((p.detail_noise_frequency - 10.0).abs() < 0.01);
        assert!(p.temperature_noise_frequency > 0.0);
    }

    #[test]
    fn calibrate_sea_level_norm_hits_target() {
        let elev: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let sea = calibrate_sea_level_norm(&elev, 0.30);
        let land = elev.iter().filter(|&&e| e >= sea).count() as f32 / 100.0;
        assert!((land - 0.30).abs() < 0.02);
    }

    #[test]
    fn apply_land_fraction_target_hits_target() {
        let mut elev: Vec<f32> = (0..1000)
            .map(|i| (i as f32 / 1000.0).sin() * 0.5 + 0.5)
            .collect();
        apply_land_fraction_target(&mut elev, 0.4, 0.35);
        let land = elev.iter().filter(|&&e| e >= 0.4).count() as f32 / elev.len() as f32;
        assert!((land - 0.35).abs() < 0.02);
    }

    #[test]
    fn resolve_mountain_slope_norm_uses_cell_and_elev_span() {
        let p = WorldGenConfig::default().resolve();
        assert!(p.mountain_slope_norm < 0.001);
        assert!(p.mountain_slope_norm > 0.0);
    }

    #[test]
    fn steep_synthetic_step_exceeds_mountain_slope_norm() {
        let config = WorldGenConfig::default();
        let params = config.resolve();
        let elev = [0.5_f32, 0.5 + params.mountain_slope_norm * 2.0];
        let delta = (elev[0] - elev[1]).abs();
        assert!(delta >= params.mountain_slope_norm);
    }

    #[test]
    fn resolve_sea_level_norm_from_datum() {
        let p = WorldGenConfig::default().resolve();
        assert!((p.sea_level_norm - 0.4).abs() < 0.01);
    }
}
