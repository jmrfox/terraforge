use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::units::{wavelength_to_frequency, Degrees, Meters, SquareMeters};

/// Generator algorithm produced by this build.
///
/// Bump this for any change that alters output for an existing config, and keep the
/// previous algorithm reachable so saved maps continue to regenerate identically.
pub const GENERATOR_VERSION: u32 = 1;

/// Configs saved before versioning existed were all produced by generator 1.
fn legacy_generator_version() -> u32 {
    1
}

/// One or more problems found while loading or validating a [`WorldGenConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub issues: Vec<String>,
}

impl ConfigError {
    fn single(issue: impl fmt::Display) -> Self {
        Self {
            issues: vec![issue.to_string()],
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid world config: {}", self.issues.join("; "))
    }
}

impl std::error::Error for ConfigError {}

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
    /// Generator algorithm this config regenerates with. Missing in JSON means 1.
    #[serde(default = "legacy_generator_version")]
    pub generator_version: u32,
    pub seed: u64,
    pub width: usize,
    pub height: usize,

    // --- Grid and vertical datum ---
    pub cell_size_m: Meters,
    pub max_elevation_m: Meters,
    pub sea_level_m: Meters,
    pub ocean_floor_m: Meters,

    // --- Elevation noise ---
    /// Legacy duplicate of `continent_wavelength_m`, used only when that field is `<= 0`.
    /// Kept so saved maps that carry a different value regenerate unchanged.
    #[deprecated(note = "set `continent_wavelength_m`; this field is ignored when that is > 0")]
    pub elevation_wavelength_m: Meters,
    /// Macro continent wavelength.
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
    #[allow(deprecated)]
    pub fn effective_continent_wavelength_m(&self) -> Meters {
        if self.continent_wavelength_m.0 > 0.0 {
            self.continent_wavelength_m
        } else {
            self.elevation_wavelength_m
        }
    }

    /// Change the cell size while keeping the generated map the same, by scaling
    /// horizontal wavelengths and ranges, lake area, and the mountain slope threshold.
    /// Vertical units are unchanged.
    #[allow(deprecated)]
    pub fn rescaled_to_cell_size(&self, cell_size_m: Meters) -> Self {
        let k = cell_size_m.0 / self.cell_size_m.0;
        let scale = |m: Meters| Meters(m.0 * k);
        let mut c = self.clone();
        c.cell_size_m = cell_size_m;
        c.elevation_wavelength_m = scale(c.elevation_wavelength_m);
        c.continent_wavelength_m = scale(c.continent_wavelength_m);
        c.detail_wavelength_m = scale(c.detail_wavelength_m);
        c.temperature_wavelength_m = scale(c.temperature_wavelength_m);
        c.elevation_detail_envelope.wavelength_m = scale(c.elevation_detail_envelope.wavelength_m);
        c.elevation_ridge_envelope.wavelength_m = scale(c.elevation_ridge_envelope.wavelength_m);
        c.continentality_ocean_range_m = scale(c.continentality_ocean_range_m);
        c.min_lake_area_m2 = SquareMeters(c.min_lake_area_m2.0 * k * k);
        // The slope gate compares a per-cell rise, so the same rise over a wider cell
        // is a shallower angle.
        let slope_tan = self.mountain_min_slope_deg.to_radians().tan() / k;
        c.mountain_min_slope_deg = Degrees(slope_tan.atan().to_degrees());
        c
    }

    /// Check that this config describes a world the current generator can build.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut issues = Vec::new();

        if self.generator_version != GENERATOR_VERSION {
            issues.push(format!(
                "generator_version {} is not supported (this build generates version {GENERATOR_VERSION})",
                self.generator_version
            ));
        }
        if self.width == 0 || self.height == 0 {
            issues.push("width and height must be at least 1".to_string());
        } else if self.width.checked_mul(self.height).is_none() {
            issues.push("width × height is too large".to_string());
        }
        if !(1..=32).contains(&self.elevation_octaves) {
            issues.push("elevation_octaves must be between 1 and 32".to_string());
        }

        let mut positive = vec![
            ("cell_size_m", self.cell_size_m.0),
            (
                "continent_wavelength_m",
                self.effective_continent_wavelength_m().0,
            ),
            ("detail_wavelength_m", self.detail_wavelength_m.0),
            ("temperature_wavelength_m", self.temperature_wavelength_m.0),
            ("elevation_persistence", self.elevation_persistence),
            ("temperature_range_c", self.temperature_range_c),
        ];
        if self.elevation_ridge_envelope.enabled {
            positive.push((
                "elevation_ridge_envelope.wavelength_m",
                self.elevation_ridge_envelope.wavelength_m.0,
            ));
        }
        if self.elevation_detail_envelope.enabled {
            positive.push((
                "elevation_detail_envelope.wavelength_m",
                self.elevation_detail_envelope.wavelength_m.0,
            ));
        }
        for (name, value) in positive {
            if !(value.is_finite() && value > 0.0) {
                issues.push(format!("{name} must be a positive number (got {value})"));
            }
        }

        for (name, value) in [
            ("min_lake_area_m2", self.min_lake_area_m2.0),
            ("lapse_rate_c_per_km", self.lapse_rate_c_per_km),
            (
                "continentality_ocean_range_m",
                self.continentality_ocean_range_m.0,
            ),
            (
                "elevation_continent_weight",
                f64::from(self.elevation_continent_weight),
            ),
            (
                "elevation_detail_weight",
                f64::from(self.elevation_detail_weight),
            ),
            (
                "elevation_ridge_weight",
                f64::from(self.elevation_ridge_weight),
            ),
            ("edge_ocean_bias", f64::from(self.edge_ocean_bias)),
            ("rainfall_scale", f64::from(self.rainfall_scale)),
            (
                "continentality_strength",
                f64::from(self.continentality_strength),
            ),
            (
                "orographic_elevation_weight",
                f64::from(self.orographic_elevation_weight),
            ),
            (
                "interior_drying_factor",
                f64::from(self.interior_drying_factor),
            ),
            (
                "mountain_min_ridge_influence",
                f64::from(self.mountain_min_ridge_influence),
            ),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                issues.push(format!("{name} must be zero or more (got {value})"));
            }
        }

        for (name, value) in [
            ("max_elevation_m", self.max_elevation_m.0),
            ("sea_level_m", self.sea_level_m.0),
            ("ocean_floor_m", self.ocean_floor_m.0),
            ("mountain_min_elevation_m", self.mountain_min_elevation_m.0),
            (
                "elevation_ridge_envelope.floor",
                f64::from(self.elevation_ridge_envelope.floor),
            ),
            (
                "elevation_ridge_envelope.strength",
                f64::from(self.elevation_ridge_envelope.strength),
            ),
            (
                "elevation_detail_envelope.floor",
                f64::from(self.elevation_detail_envelope.floor),
            ),
            (
                "elevation_detail_envelope.strength",
                f64::from(self.elevation_detail_envelope.strength),
            ),
        ] {
            if !value.is_finite() {
                issues.push(format!("{name} must be a finite number"));
            }
        }

        if self.max_elevation_m.0 <= self.ocean_floor_m.0 {
            issues.push("max_elevation_m must be above ocean_floor_m".to_string());
        }
        if self.sea_level_m.0 < self.ocean_floor_m.0 || self.sea_level_m.0 > self.max_elevation_m.0
        {
            issues.push("sea_level_m must be between ocean_floor_m and max_elevation_m".into());
        }
        if !(0.0..90.0).contains(&self.mountain_min_slope_deg.0) {
            issues.push("mountain_min_slope_deg must be in [0, 90)".to_string());
        }
        if let Some(land) = self.target_land_fraction {
            if !(land > 0.0 && land < 1.0) {
                issues.push("target_land_fraction must be between 0 and 1".to_string());
            }
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(ConfigError { issues })
        }
    }

    /// Strictly load a preset: rejects unknown fields and invalid values.
    ///
    /// Plain `serde_json::from_str` stays lenient so older saved maps keep loading.
    pub fn from_json(text: &str) -> Result<Self, ConfigError> {
        let value: Value = serde_json::from_str(text).map_err(ConfigError::single)?;
        let reference = serde_json::to_value(Self::default()).map_err(ConfigError::single)?;

        let mut issues = Vec::new();
        collect_unknown_fields(&value, &reference, "", &mut issues);
        if value.get("elevation_wavelength_m").is_some()
            && value.get("continent_wavelength_m").is_none()
        {
            issues.push(
                "elevation_wavelength_m has no effect on its own; set continent_wavelength_m"
                    .to_string(),
            );
        }
        if !issues.is_empty() {
            return Err(ConfigError { issues });
        }

        let config: Self = serde_json::from_value(value).map_err(ConfigError::single)?;
        config.validate()?;
        Ok(config)
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

fn collect_unknown_fields(value: &Value, reference: &Value, path: &str, issues: &mut Vec<String>) {
    let (Value::Object(fields), Value::Object(known)) = (value, reference) else {
        return;
    };
    for (key, child) in fields {
        let child_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        match known.get(key) {
            Some(known_child) => collect_unknown_fields(child, known_child, &child_path, issues),
            None => issues.push(format!("unknown field `{child_path}`")),
        }
    }
}

impl Default for WorldGenConfig {
    #[allow(deprecated)]
    fn default() -> Self {
        Self {
            generator_version: GENERATOR_VERSION,
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

    #[test]
    fn default_and_regional_configs_are_valid() {
        assert_eq!(WorldGenConfig::default().validate(), Ok(()));
        assert_eq!(
            WorldGenConfig::default()
                .rescaled_to_cell_size(Meters(400.0))
                .validate(),
            Ok(())
        );
    }

    #[test]
    fn missing_version_means_generator_one() {
        let config: WorldGenConfig = serde_json::from_str(r#"{"seed": 7}"#).unwrap();
        assert_eq!(config.generator_version, 1);
    }

    #[test]
    fn validate_reports_every_problem() {
        let config = WorldGenConfig {
            generator_version: GENERATOR_VERSION + 1,
            width: 0,
            cell_size_m: Meters(f64::NAN),
            ..WorldGenConfig::default()
        };
        let issues = config.validate().unwrap_err().issues;
        assert_eq!(issues.len(), 3, "{issues:?}");
    }

    #[test]
    fn from_json_rejects_misspelled_and_nested_fields() {
        let top = WorldGenConfig::from_json(r#"{"sea_levle_m": 5.0}"#).unwrap_err();
        assert!(top.issues[0].contains("sea_levle_m"));
        let nested =
            WorldGenConfig::from_json(r#"{"elevation_ridge_envelope": {"strenght": 1.0}}"#)
                .unwrap_err();
        assert!(nested.issues[0].contains("elevation_ridge_envelope.strenght"));
    }

    #[test]
    fn from_json_rejects_ineffective_legacy_wavelength() {
        let err = WorldGenConfig::from_json(r#"{"elevation_wavelength_m": 2560.0}"#).unwrap_err();
        assert!(err.issues[0].contains("continent_wavelength_m"));
    }

    #[test]
    fn from_json_accepts_a_saved_config() {
        let saved = include_str!("../tests/fixtures/saved_map_v1.json");
        let config = WorldGenConfig::from_json(saved).unwrap();
        assert_eq!(config.generator_version, 1);
        assert_eq!(config.seed, 2103);
    }

    #[test]
    fn rescale_preserves_grid_quantities() {
        let base = WorldGenConfig::default();
        let (a, b) = (
            base.resolve(),
            WorldGenConfig::default()
                .rescaled_to_cell_size(Meters(400.0))
                .resolve(),
        );
        assert_eq!(b.cell_size_m, 400.0);
        assert_eq!(a.continent_noise_frequency, b.continent_noise_frequency);
        assert_eq!(a.detail_noise_frequency, b.detail_noise_frequency);
        assert_eq!(a.temperature_noise_frequency, b.temperature_noise_frequency);
        assert_eq!(
            a.continentality_ocean_range_cells,
            b.continentality_ocean_range_cells
        );
        assert_eq!(a.min_lake_cells, b.min_lake_cells);
        assert!((a.mountain_slope_norm - b.mountain_slope_norm).abs() < 1e-9);
    }
}
