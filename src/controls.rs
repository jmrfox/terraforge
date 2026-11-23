//! High-level world controls layered over [`WorldGenConfig`].
//!
//! Each control reads from and writes to ordinary config fields, so controls add no saved
//! state and cannot change what the generator does with a given config. The center of
//! every 0 to 1 control is the default config, whatever its current tuning.

use std::ops::RangeInclusive;

use crate::config::WorldGenConfig;
use crate::units::Meters;

/// Smoothest and most rugged terrain.
const RIDGE_WEIGHT: (f64, f64) = (0.06, 0.42);
const DETAIL_WEIGHT: (f64, f64) = (0.12, 0.38);
const PERSISTENCE: (f64, f64) = (0.40, 0.60);
/// Ridge noise does not weaken with the ridge weight, so smooth worlds need a higher bar
/// to call a high area a mountain.
const MOUNTAIN_RIDGE_MINIMUM: (f64, f64) = (0.62, 0.36);

/// Polar and tropical ends of the climate control.
const ABS_LATITUDE_DEG: (f64, f64) = (66.0, 6.0);
const TEMPERATURE_OFFSET_C: (f64, f64) = (-1.0, 2.0);

/// Wet and arid ends of the aridity control.
const BASE_RAINFALL_MM: (f64, f64) = (3400.0, 520.0);
const SUBTROPICAL_DRYING: (f64, f64) = (0.25, 0.90);

const WORLD_SIZE_KM: RangeInclusive<f64> = 20.0..=20_000.0;
const MATCH_TOLERANCE: f64 = 1e-6;

/// A single high-level control over a world's character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldControl {
    /// Longest map side in km. Keeps the terrain and changes its real size.
    Scale,
    /// 0 is smooth and 1 is rugged.
    Ruggedness,
    /// 0 is polar and 1 is tropical. Keeps the map's hemisphere.
    Climate,
    /// 0 is wet and 1 is arid.
    Aridity,
    /// Compass direction the prevailing wind blows from, in degrees.
    Wind,
}

impl WorldControl {
    pub const ALL: [Self; 5] = [
        Self::Scale,
        Self::Ruggedness,
        Self::Climate,
        Self::Aridity,
        Self::Wind,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Scale => "World size",
            Self::Ruggedness => "Ruggedness",
            Self::Climate => "Climate",
            Self::Aridity => "Aridity",
            Self::Wind => "Wind from",
        }
    }

    /// Names for the low and high ends of the control.
    pub fn end_labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Scale => ("Regional", "Continental"),
            Self::Ruggedness => ("Smooth", "Rugged"),
            Self::Climate => ("Polar", "Tropical"),
            Self::Aridity => ("Wet", "Arid"),
            Self::Wind => ("N", "N"),
        }
    }

    pub fn range(self) -> RangeInclusive<f64> {
        match self {
            Self::Scale => WORLD_SIZE_KM,
            Self::Wind => 0.0..=360.0,
            Self::Ruggedness | Self::Climate | Self::Aridity => 0.0..=1.0,
        }
    }

    /// The value that matches the default config at this map's dimensions.
    pub fn default_value(self, config: &WorldGenConfig) -> f64 {
        let defaults = WorldGenConfig::default();
        match self {
            Self::Scale => longest_side(config) * defaults.cell_size_m.0 / 1000.0,
            Self::Wind => defaults.regional_climate.wind_from_deg,
            Self::Ruggedness | Self::Climate | Self::Aridity => 0.5,
        }
    }

    /// Current position of this control. For a custom config, this is the position of
    /// the control's main field.
    pub fn value(self, config: &WorldGenConfig) -> f64 {
        let d = WorldGenConfig::default();
        match self {
            Self::Scale => longest_side(config) * config.cell_size_m.0 / 1000.0,
            Self::Ruggedness => position_on(
                f64::from(config.elevation_ridge_weight),
                RIDGE_WEIGHT,
                f64::from(d.elevation_ridge_weight),
            ),
            Self::Climate => position_on(
                config.regional_climate.latitude_deg.abs(),
                ABS_LATITUDE_DEG,
                d.regional_climate.latitude_deg.abs(),
            ),
            Self::Aridity => position_on(
                config.regional_climate.base_rainfall_mm,
                BASE_RAINFALL_MM,
                d.regional_climate.base_rainfall_mm,
            ),
            Self::Wind => config.regional_climate.wind_from_deg.rem_euclid(360.0),
        }
    }

    /// Move this control, writing only the config fields it drives. Non-finite values
    /// are ignored and others are clamped to [`Self::range`].
    pub fn apply(self, config: &mut WorldGenConfig, value: f64) {
        if !value.is_finite() {
            return;
        }
        let range = self.range();
        let v = value.clamp(*range.start(), *range.end());
        let d = WorldGenConfig::default();
        match self {
            Self::Scale => apply_scale(config, v),
            Self::Ruggedness => {
                config.elevation_ridge_weight =
                    along(v, RIDGE_WEIGHT, f64::from(d.elevation_ridge_weight)) as f32;
                config.elevation_detail_weight =
                    along(v, DETAIL_WEIGHT, f64::from(d.elevation_detail_weight)) as f32;
                config.elevation_persistence = along(v, PERSISTENCE, d.elevation_persistence);
                config.mountain_min_ridge_influence = along(
                    v,
                    MOUNTAIN_RIDGE_MINIMUM,
                    f64::from(d.mountain_min_ridge_influence),
                ) as f32;
            }
            Self::Climate => {
                let climate = &mut config.regional_climate;
                let hemisphere = if climate.latitude_deg < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let latitude = along(v, ABS_LATITUDE_DEG, d.regional_climate.latitude_deg.abs());
                climate.latitude_deg = hemisphere * latitude;
                climate.temperature_offset_c = along(
                    v,
                    TEMPERATURE_OFFSET_C,
                    d.regional_climate.temperature_offset_c,
                );
            }
            Self::Aridity => {
                let climate = &mut config.regional_climate;
                climate.base_rainfall_mm =
                    along(v, BASE_RAINFALL_MM, d.regional_climate.base_rainfall_mm);
                climate.subtropical_drying =
                    along(v, SUBTROPICAL_DRYING, d.regional_climate.subtropical_drying);
            }
            Self::Wind => config.regional_climate.wind_from_deg = v.rem_euclid(360.0),
        }
    }

    /// True when the fields this control drives were set to a combination it cannot
    /// produce, for example by editing one directly. Moving the control replaces them.
    pub fn is_custom(self, config: &WorldGenConfig) -> bool {
        if matches!(self, Self::Scale | Self::Wind) {
            return false;
        }
        let mut projected = config.clone();
        self.apply(&mut projected, self.value(config));
        self.driven_fields(config)
            .iter()
            .zip(self.driven_fields(&projected))
            .any(|(&a, b)| (a - b).abs() > MATCH_TOLERANCE * a.abs().max(b.abs()).max(1.0))
    }

    fn driven_fields(self, config: &WorldGenConfig) -> [f64; 4] {
        let climate = &config.regional_climate;
        match self {
            Self::Ruggedness => [
                f64::from(config.elevation_ridge_weight),
                f64::from(config.elevation_detail_weight),
                config.elevation_persistence,
                f64::from(config.mountain_min_ridge_influence),
            ],
            Self::Climate => [
                climate.latitude_deg.abs(),
                climate.temperature_offset_c,
                0.0,
                0.0,
            ],
            Self::Aridity => [
                climate.base_rainfall_mm,
                climate.subtropical_drying,
                0.0,
                0.0,
            ],
            Self::Scale | Self::Wind => [0.0; 4],
        }
    }
}

fn longest_side(config: &WorldGenConfig) -> f64 {
    config.width.max(config.height) as f64
}

fn apply_scale(config: &mut WorldGenConfig, world_km: f64) {
    let side = longest_side(config);
    let current = config.cell_size_m.0;
    if side <= 0.0 || !(current.is_finite() && current > 0.0) {
        return;
    }
    let cell_size_m = world_km * 1000.0 / side;
    // Skipping tiny moves keeps repeated edits from accumulating rounding drift.
    if (cell_size_m - current).abs() > 1e-9 * current {
        *config = config.rescaled_to_cell_size(Meters(cell_size_m));
    }
}

/// Interpolate through `center` at 0.5, landing on it exactly.
fn along(position: f64, (low, high): (f64, f64), center: f64) -> f64 {
    if position < 0.5 {
        low + (center - low) * (position * 2.0)
    } else {
        center + (high - center) * ((position - 0.5) * 2.0)
    }
}

/// Inverse of [`along`], clamped to `[0, 1]`.
fn position_on(value: f64, (low, high): (f64, f64), center: f64) -> f64 {
    let position = if (value - center) * (low - center) > 0.0 {
        0.5 * (value - low) / (center - low)
    } else {
        0.5 + 0.5 * (value - center) / (high - center)
    };
    position.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(config: &WorldGenConfig) -> String {
        serde_json::to_string(config).unwrap()
    }

    #[test]
    fn default_positions_leave_the_default_config_unchanged() {
        let base = WorldGenConfig::default();
        for control in WorldControl::ALL {
            let mut config = base.clone();
            assert_eq!(control.value(&base), control.default_value(&base));
            control.apply(&mut config, control.default_value(&base));
            assert_eq!(json(&config), json(&base), "{control:?}");
            assert!(!control.is_custom(&base), "{control:?}");
        }
    }

    #[test]
    fn every_position_round_trips() {
        for control in [
            WorldControl::Ruggedness,
            WorldControl::Climate,
            WorldControl::Aridity,
        ] {
            for step in 0..=20 {
                let position = f64::from(step) / 20.0;
                let mut config = WorldGenConfig::default();
                control.apply(&mut config, position);
                assert!(
                    (control.value(&config) - position).abs() < 1e-6,
                    "{control:?} at {position}"
                );
                assert!(!control.is_custom(&config), "{control:?} at {position}");
            }
        }
    }

    #[test]
    fn advanced_edits_show_as_custom_until_the_control_moves() {
        let mut config = WorldGenConfig {
            elevation_detail_weight: 0.31,
            ..WorldGenConfig::default()
        };
        assert!(WorldControl::Ruggedness.is_custom(&config));
        assert!(!WorldControl::Aridity.is_custom(&config));

        WorldControl::Aridity.apply(&mut config, 0.8);
        assert_eq!(config.elevation_detail_weight, 0.31, "unrelated edit kept");

        WorldControl::Ruggedness.apply(&mut config, 0.7);
        assert!(!WorldControl::Ruggedness.is_custom(&config));
    }

    #[test]
    fn climate_keeps_the_hemisphere() {
        let mut config = WorldGenConfig::default();
        config.regional_climate.latitude_deg = -35.0;
        assert!(!WorldControl::Climate.is_custom(&config));
        WorldControl::Climate.apply(&mut config, 0.0);
        assert_eq!(config.regional_climate.latitude_deg, -ABS_LATITUDE_DEG.0);
    }

    #[test]
    fn scale_sets_the_longest_side_and_keeps_the_terrain() {
        let base = WorldGenConfig {
            width: 64,
            height: 32,
            ..WorldGenConfig::default()
        };
        let mut scaled = base.clone();
        WorldControl::Scale.apply(&mut scaled, 512.0);
        assert!((scaled.cell_size_m.0 - 8000.0).abs() < 1e-9);
        assert_eq!(
            crate::generate_world(&base).elevation,
            crate::generate_world(&scaled).elevation
        );
    }

    #[test]
    fn invalid_values_are_ignored_and_out_of_range_values_clamp() {
        let base = WorldGenConfig::default();
        let mut config = base.clone();
        for control in WorldControl::ALL {
            control.apply(&mut config, f64::NAN);
        }
        assert_eq!(json(&config), json(&base));

        WorldControl::Aridity.apply(&mut config, 9.0);
        assert_eq!(config.regional_climate.base_rainfall_mm, BASE_RAINFALL_MM.1);
        WorldControl::Wind.apply(&mut config, 360.0);
        assert_eq!(config.regional_climate.wind_from_deg, 0.0);
    }

    /// Land-cell means of temperature, rainfall, and neighbor height difference, plus
    /// the Mountain share of land.
    fn land_summary(config: &WorldGenConfig) -> (f64, f64, f64, f64) {
        let map = crate::generate_world(config);
        let (mut land, mut temperature, mut rain, mut relief, mut mountains) =
            (0.0, 0.0, 0.0, 0.0, 0.0);
        for i in 0..map.biome.len() {
            if map.water_mask[i] {
                continue;
            }
            land += 1.0;
            temperature += f64::from(map.temperature_c[i]);
            rain += f64::from(map.rainfall_mm[i]);
            if map.biome[i] == crate::Biome::Mountain {
                mountains += 1.0;
            }
            if (i + 1) % map.width != 0 {
                relief += (map.elevation_m(i + 1) - map.elevation_m(i)).abs();
            }
        }
        (
            temperature / land,
            rain / land,
            relief / land,
            mountains / land,
        )
    }

    #[test]
    fn controls_move_the_map_in_the_named_direction() {
        let at = |control: WorldControl, position: f64| {
            let mut config = WorldGenConfig {
                width: 192,
                height: 192,
                ..WorldGenConfig::default()
            };
            control.apply(&mut config, position);
            land_summary(&config)
        };

        let (polar, tropical) = (
            at(WorldControl::Climate, 0.0),
            at(WorldControl::Climate, 1.0),
        );
        assert!(tropical.0 > polar.0 + 20.0, "climate should warm the map");

        let (wet, arid) = (
            at(WorldControl::Aridity, 0.0),
            at(WorldControl::Aridity, 1.0),
        );
        assert!(arid.1 < wet.1 * 0.3, "aridity should dry the map");

        let (smooth, rugged) = (
            at(WorldControl::Ruggedness, 0.0),
            at(WorldControl::Ruggedness, 1.0),
        );
        assert!(rugged.2 > smooth.2 * 2.0, "ruggedness should add relief");
        assert!(rugged.3 > smooth.3, "ruggedness should add mountains");
    }

    #[test]
    fn extreme_positions_are_valid_configs() {
        for control in WorldControl::ALL {
            let range = control.range();
            for value in [*range.start(), *range.end()] {
                let mut config = WorldGenConfig::default();
                control.apply(&mut config, value);
                assert_eq!(config.validate(), Ok(()), "{control:?} at {value}");
            }
        }
    }
}
