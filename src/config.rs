use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::units::{Degrees, Meters, SquareMeters};

/// Generator algorithm in this build, recorded on every map it produces.
///
/// Bump this when a change alters output for an existing config.
pub const GENERATOR_VERSION: u32 = 4;

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
            wavelength_m: Meters(320_000.0),
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
            floor: 0.08,
            strength: 0.92,
            ..Self::default()
        }
    }
}

/// Regional climate parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionalClimateConfig {
    /// Latitude at the center of the map. North is toward row 0.
    pub latitude_deg: f64,
    /// Mean annual sea-level temperature at the equator.
    pub equator_temperature_c: f64,
    /// Mean annual sea-level temperature at the poles.
    pub pole_temperature_c: f64,
    /// Warms or cools the whole world.
    pub temperature_offset_c: f64,
    /// Amplitude of local temperature variation.
    pub temperature_variation_c: f64,
    /// Compass direction the prevailing wind blows from (270 = westerly).
    pub wind_from_deg: f64,
    /// Annual rain from saturated air at 15 °C.
    pub base_rainfall_mm: f64,
    /// Relative amplitude of local rainfall variation, in `[0, 1)`.
    pub rainfall_variation: f64,
    /// Distance over which rain depletes moisture carried inland.
    pub rainout_length_m: Meters,
    /// Fraction of moisture condensed by rising terrain that reaches the ground.
    pub orographic_efficiency: f64,
    /// Terrain smoothing used for airflow, so wind responds to ranges and not single hills.
    pub orographic_smoothing_m: Meters,
    /// Rainfall reduction at the subtropical dry belt near 27° latitude, in `[0, 1]`.
    pub subtropical_drying: f64,
}

impl Default for RegionalClimateConfig {
    fn default() -> Self {
        Self {
            latitude_deg: 35.0,
            equator_temperature_c: 27.0,
            pole_temperature_c: -18.0,
            temperature_offset_c: 0.0,
            temperature_variation_c: 3.0,
            wind_from_deg: 270.0,
            base_rainfall_mm: 1800.0,
            rainfall_variation: 0.25,
            rainout_length_m: Meters(800_000.0),
            orographic_efficiency: 0.12,
            orographic_smoothing_m: Meters(6_000.0),
            subtropical_drying: 0.6,
        }
    }
}

/// Rivers and lakes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HydrologyConfig {
    /// Mean annual discharge at which a channel counts as a river.
    pub river_min_discharge_m3s: f64,
    /// Shallower depressions are treated as noise that water flows across.
    pub min_lake_depth_m: f64,
    /// How far a river flowing at 10 m³/s cuts down through a lake's outlet. Incision
    /// grows with the square root of flow, and higher values mean fewer, smaller lakes.
    /// The default is large because noise terrain has no history of river erosion.
    pub outlet_incision_m: f64,
}

impl Default for HydrologyConfig {
    fn default() -> Self {
        Self {
            river_min_discharge_m3s: 2.0,
            min_lake_depth_m: 60.0,
            outlet_incision_m: 400.0,
        }
    }
}

/// User-facing configuration in physical units.
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
    /// When set, elevation is shifted so this land fraction emerges at sea level.
    pub target_land_fraction: Option<f32>,
    /// Subtract elevation near map edges (0 = off) to encourage edge-connected ocean.
    pub edge_ocean_bias: f32,
    /// Width of the band along map edges that `edge_ocean_bias` affects.
    pub edge_ocean_margin_m: Meters,
    /// Square region from the world origin used to place sea level. `None` uses the map
    /// itself, which hits the land target closely but ties coastlines to the map extent.
    /// Set it to keep coastlines fixed across different map sizes.
    pub land_reference_extent_m: Option<Meters>,

    // --- Water ---
    pub min_lake_area_m2: SquareMeters,

    // --- Climate ---
    pub lapse_rate_c_per_km: f64,
    pub rainfall_scale: f32,
    /// Wavelength of local temperature and rainfall variation.
    pub temperature_wavelength_m: Meters,
    pub regional_climate: RegionalClimateConfig,

    // --- Hydrology ---
    pub hydrology: HydrologyConfig,

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
    pub mountain_elev_norm: f32,
    /// Minimum normalized elevation change per cell on the smoothed terrain.
    pub mountain_slope_norm: f32,
    /// Smoothing radius for the mountain slope test, in cells.
    pub mountain_smoothing_cells: usize,
    /// Normalized height above the mountain minimum where no slope is required.
    pub mountain_slope_fade_norm: f32,
    pub mountain_min_ridge_influence: f32,
}

impl WorldGenConfig {
    /// Change the cell size while keeping the terrain the same, by scaling horizontal
    /// wavelengths and ranges, lake area, and the mountain slope threshold. Vertical
    /// units are unchanged. Climate constants stay physical, so temperature and rainfall
    /// respond to the world's new real size.
    pub fn rescaled_to_cell_size(&self, cell_size_m: Meters) -> Self {
        let k = cell_size_m.0 / self.cell_size_m.0;
        let scale = |m: Meters| Meters(m.0 * k);
        let mut c = self.clone();
        c.cell_size_m = cell_size_m;
        c.continent_wavelength_m = scale(c.continent_wavelength_m);
        c.detail_wavelength_m = scale(c.detail_wavelength_m);
        c.temperature_wavelength_m = scale(c.temperature_wavelength_m);
        c.elevation_detail_envelope.wavelength_m = scale(c.elevation_detail_envelope.wavelength_m);
        c.elevation_ridge_envelope.wavelength_m = scale(c.elevation_ridge_envelope.wavelength_m);
        c.edge_ocean_margin_m = scale(c.edge_ocean_margin_m);
        c.land_reference_extent_m = c.land_reference_extent_m.map(scale);
        c.min_lake_area_m2 = SquareMeters(c.min_lake_area_m2.0 * k * k);
        // The slope gate compares a per-cell rise, so the same rise over a wider cell
        // is a shallower angle.
        let slope_tan = self.mountain_min_slope_deg.to_radians().tan() / k;
        c.mountain_min_slope_deg = Degrees(slope_tan.atan().to_degrees());
        c
    }

    /// Check that this config describes a world the generator can build.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut issues = Vec::new();

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
            ("continent_wavelength_m", self.continent_wavelength_m.0),
            ("detail_wavelength_m", self.detail_wavelength_m.0),
            ("temperature_wavelength_m", self.temperature_wavelength_m.0),
            ("elevation_persistence", self.elevation_persistence),
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
            ("edge_ocean_margin_m", self.edge_ocean_margin_m.0),
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
        if self
            .land_reference_extent_m
            .is_some_and(|e| !(e.0.is_finite() && e.0 > 0.0))
        {
            issues.push("land_reference_extent_m must be a positive number".to_string());
        }
        for (name, value) in [
            (
                "hydrology.river_min_discharge_m3s",
                self.hydrology.river_min_discharge_m3s,
            ),
            (
                "hydrology.min_lake_depth_m",
                self.hydrology.min_lake_depth_m,
            ),
            (
                "hydrology.outlet_incision_m",
                self.hydrology.outlet_incision_m,
            ),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                issues.push(format!("{name} must be zero or more (got {value})"));
            }
        }
        self.validate_regional_climate(&mut issues);

        if issues.is_empty() {
            Ok(())
        } else {
            Err(ConfigError { issues })
        }
    }

    fn validate_regional_climate(&self, issues: &mut Vec<String>) {
        let c = &self.regional_climate;
        let mut check = |ok: bool, message: &str| {
            if !ok {
                issues.push(message.to_string());
            }
        };
        check(
            (-90.0..=90.0).contains(&c.latitude_deg),
            "regional_climate.latitude_deg must be between -90 and 90",
        );
        check(
            c.equator_temperature_c.is_finite()
                && c.pole_temperature_c.is_finite()
                && c.equator_temperature_c > c.pole_temperature_c,
            "regional_climate.equator_temperature_c must be above pole_temperature_c",
        );
        check(
            c.temperature_offset_c.is_finite(),
            "regional_climate.temperature_offset_c must be a finite number",
        );
        check(
            c.temperature_variation_c.is_finite() && c.temperature_variation_c >= 0.0,
            "regional_climate.temperature_variation_c must be zero or more",
        );
        check(
            c.wind_from_deg.is_finite(),
            "regional_climate.wind_from_deg must be a finite number",
        );
        check(
            c.base_rainfall_mm.is_finite() && c.base_rainfall_mm >= 0.0,
            "regional_climate.base_rainfall_mm must be zero or more",
        );
        check(
            (0.0..1.0).contains(&c.rainfall_variation),
            "regional_climate.rainfall_variation must be in [0, 1)",
        );
        check(
            c.rainout_length_m.0.is_finite() && c.rainout_length_m.0 > 0.0,
            "regional_climate.rainout_length_m must be a positive number",
        );
        check(
            (0.0..=1.0).contains(&c.orographic_efficiency),
            "regional_climate.orographic_efficiency must be in [0, 1]",
        );
        check(
            c.orographic_smoothing_m.0.is_finite() && c.orographic_smoothing_m.0 >= 0.0,
            "regional_climate.orographic_smoothing_m must be zero or more",
        );
        check(
            (0.0..=1.0).contains(&c.subtropical_drying),
            "regional_climate.subtropical_drying must be in [0, 1]",
        );
    }

    /// Strictly load a preset: rejects unknown fields and invalid values.
    pub fn from_json(text: &str) -> Result<Self, ConfigError> {
        let value: Value = serde_json::from_str(text).map_err(ConfigError::single)?;
        let reference = serde_json::to_value(Self::default()).map_err(ConfigError::single)?;

        let mut issues = Vec::new();
        collect_unknown_fields(&value, &reference, "", &mut issues);
        if !issues.is_empty() {
            return Err(ConfigError { issues });
        }

        let config: Self = serde_json::from_value(value).map_err(ConfigError::single)?;
        config.validate()?;
        Ok(config)
    }

    pub fn resolve(&self) -> ResolvedSimParams {
        let cell = self.cell_size_m.0;
        let max_elev = self.max_elevation_m.0;
        let ocean_floor = self.ocean_floor_m.0;
        let elev_span = (max_elev - ocean_floor).max(1.0);

        let sea_level_norm =
            (((self.sea_level_m.0 - ocean_floor) / elev_span) as f32).clamp(0.0, 1.0);
        let mountain_elev_norm =
            (((self.mountain_min_elevation_m.0 - ocean_floor) / elev_span) as f32).clamp(0.0, 1.0);
        let slope_tan = self.mountain_min_slope_deg.to_radians().tan();
        let mountain_slope_norm = (slope_tan * cell / elev_span).clamp(0.0, 1.0) as f32;

        ResolvedSimParams {
            cell_size_m: cell,
            map_width_m: self.width as f64 * cell,
            map_height_m: self.height as f64 * cell,
            max_elevation_m: max_elev,
            sea_level_m: self.sea_level_m.0,
            ocean_floor_m: ocean_floor,
            sea_level_norm,
            min_lake_cells: self.min_lake_area_m2.to_cell_count(cell).max(1),
            mountain_elev_norm,
            mountain_slope_norm,
            // Tied to terrain detail, so it measures hillsides at any cell size.
            mountain_smoothing_cells: ((self.detail_wavelength_m.0 * 0.08 / cell).round() as usize)
                .clamp(1, self.width.max(self.height).max(1)),
            mountain_slope_fade_norm: (1000.0 / elev_span) as f32,
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
    fn default() -> Self {
        Self {
            seed: 42,
            width: 512,
            height: 512,

            cell_size_m: Meters(400.0),
            max_elevation_m: Meters(9000.0),
            sea_level_m: Meters(0.0),
            ocean_floor_m: Meters(-6000.0),

            continent_wavelength_m: Meters(102_400.0),
            detail_wavelength_m: Meters(20_480.0),
            elevation_octaves: 4,
            elevation_persistence: 0.5,
            elevation_continent_weight: 0.65,
            elevation_detail_weight: 0.25,
            elevation_ridge_weight: 0.23,
            elevation_detail_envelope: ElevationEnvelopeConfig::default(),
            elevation_ridge_envelope: ElevationEnvelopeConfig::ridge_default(),
            target_land_fraction: Some(0.35),
            edge_ocean_bias: 0.12,
            edge_ocean_margin_m: Meters(40_000.0),
            land_reference_extent_m: None,

            min_lake_area_m2: SquareMeters(3_840_000.0),

            lapse_rate_c_per_km: 6.5,
            rainfall_scale: 0.90,
            temperature_wavelength_m: Meters(240_000.0),
            regional_climate: RegionalClimateConfig::default(),

            hydrology: HydrologyConfig::default(),

            mountain_min_elevation_m: Meters(2400.0),
            mountain_min_slope_deg: Degrees(6.0),
            mountain_min_ridge_influence: 0.45,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_default_cell_counts() {
        let p = WorldGenConfig::default().resolve();
        assert_eq!(p.cell_size_m, 400.0);
        assert_eq!(p.map_width_m, 204_800.0);
        assert_eq!(p.min_lake_cells, 24);
    }

    #[test]
    fn calibrate_sea_level_norm_hits_target() {
        let elev: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let sea = calibrate_sea_level_norm(&elev, 0.30);
        let land = elev.iter().filter(|&&e| e >= sea).count() as f32 / 100.0;
        assert!((land - 0.30).abs() < 0.02);
    }

    #[test]
    fn resolve_mountain_slope_norm_uses_cell_and_elev_span() {
        let p = WorldGenConfig::default().resolve();
        let expected = 6.0f64.to_radians().tan() * 400.0 / 15_000.0;
        assert!((f64::from(p.mountain_slope_norm) - expected).abs() < 1e-6);
    }

    #[test]
    fn resolve_sea_level_norm_from_datum() {
        let p = WorldGenConfig::default().resolve();
        assert!((p.sea_level_norm - 0.4).abs() < 0.01);
    }

    #[test]
    fn default_and_rescaled_configs_are_valid() {
        let config = WorldGenConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(
            config.rescaled_to_cell_size(Meters(137.0)).validate(),
            Ok(())
        );
    }

    #[test]
    fn validate_reports_every_problem() {
        let mut config = WorldGenConfig {
            width: 0,
            cell_size_m: Meters(f64::NAN),
            ..WorldGenConfig::default()
        };
        config.regional_climate.latitude_deg = 120.0;
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
    fn from_json_accepts_a_saved_config() {
        let saved = serde_json::to_string(&WorldGenConfig::test_config(2103, 128)).unwrap();
        let config = WorldGenConfig::from_json(&saved).unwrap();
        assert_eq!((config.seed, config.width), (2103, 128));
    }

    #[test]
    fn rescale_scales_horizontal_lengths() {
        let base = WorldGenConfig {
            land_reference_extent_m: Some(Meters(100_000.0)),
            ..WorldGenConfig::default()
        };
        let scaled = base.rescaled_to_cell_size(Meters(800.0));
        assert_eq!(scaled.cell_size_m.0, 800.0);
        assert_eq!(scaled.continent_wavelength_m.0, 204_800.0);
        assert_eq!(scaled.edge_ocean_margin_m.0, 80_000.0);
        assert_eq!(scaled.land_reference_extent_m, Some(Meters(200_000.0)));
        assert_eq!(
            scaled.resolve().min_lake_cells,
            base.resolve().min_lake_cells
        );
        assert_eq!(scaled.regional_climate, base.regional_climate);
        let (a, b) = (base.resolve(), scaled.resolve());
        assert!((a.mountain_slope_norm - b.mountain_slope_norm).abs() < 1e-9);
    }
}
