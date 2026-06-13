//! Prior distributions for random configuration sampling (GUI / CLI / library).

use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::config::WorldGenConfig;
use crate::units::{Meters, SquareMeters};

/// Identifies a single sampleable numerical config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleParam {
    MaxElevationM,
    SeaLevelM,
    OceanFloorM,
    ElevationWavelengthM,
    DetailWavelengthM,
    ElevationOctaves,
    ElevationPersistence,
    ElevationRidgeWeight,
    TargetLandFraction,
    EdgeOceanBias,
    MinLakeAreaM2,
    TemperatureRangeC,
    LapseRateCPerKm,
    RainfallScale,
    TemperatureWavelengthM,
    ContinentalityStrength,
    InteriorDryingFactor,
    OrographicElevationWeight,
    MountainMinElevationM,
    MountainMinRidgeInfluence,
}

/// A prior over a scalar (uniform or log-uniform).
#[derive(Debug, Clone, PartialEq)]
pub enum PriorDist {
    Uniform { min: f64, max: f64 },
    LogUniform { min: f64, max: f64 },
}

impl PriorDist {
    pub fn sample_f64<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        match self {
            Self::Uniform { min, max } => {
                if min >= max {
                    *min
                } else {
                    rng.gen::<f64>() * (max - min) + min
                }
            }
            Self::LogUniform { min, max } => {
                let lo = min.max(f64::MIN_POSITIVE);
                let hi = max.max(lo * 1.001);
                let log_lo = lo.ln();
                let log_hi = hi.ln();
                (log_lo + rng.gen::<f64>() * (log_hi - log_lo)).exp()
            }
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Uniform { min, max } => format!("{min:.3} – {max:.3}"),
            Self::LogUniform { min, max } => format!("log {min:.3} – {max:.3}"),
        }
    }
}

/// One sampleable parameter with inclusion flag and prior.
#[derive(Debug, Clone)]
pub struct SampleableParam {
    pub id: SampleParam,
    pub label: &'static str,
    pub category: &'static str,
    pub enabled: bool,
    pub dist: PriorDist,
}

/// Full set of priors for configuration sampling.
#[derive(Debug, Clone)]
pub struct PriorSet {
    pub params: Vec<SampleableParam>,
}

impl PriorSet {
    pub fn default_priors() -> Self {
        Self {
            params: build_default_priors(),
        }
    }

    pub fn enable_all(&mut self) {
        for p in &mut self.params {
            p.enabled = true;
        }
    }

    pub fn disable_all(&mut self) {
        for p in &mut self.params {
            p.enabled = false;
        }
    }

    /// Restore default inclusion checkboxes (which parameters are sampled).
    pub fn reset_sampling_selection(&mut self) {
        *self = Self::default_priors();
    }

    pub fn enabled_count(&self) -> usize {
        self.params.iter().filter(|p| p.enabled).count()
    }

    /// Sample enabled parameters into `config`. Grid fields (seed, width, height, cell size)
    /// and enums are left unchanged.
    pub fn sample_into<R: Rng + ?Sized>(&self, config: &mut WorldGenConfig, rng: &mut R) {
        for param in &self.params {
            if !param.enabled {
                continue;
            }
            apply_sample(config, param.id, param.dist.sample_f64(rng));
        }
        constrain_sampled_config(config);
    }

    /// Return a copy of `base` with enabled parameters drawn from this prior set.
    pub fn sample_config<R: Rng + ?Sized>(
        &self,
        base: &WorldGenConfig,
        rng: &mut R,
    ) -> WorldGenConfig {
        let mut config = base.clone();
        self.sample_into(&mut config, rng);
        config
    }
}

/// Draw enabled parameters from `priors` into `config`.
///
/// Returns the RNG seed used for prior draws (random when `sample_seed` is `None`).
pub fn sample_parameters(
    config: &mut WorldGenConfig,
    priors: &PriorSet,
    sample_seed: Option<u64>,
) -> u64 {
    let seed = sample_seed.unwrap_or_else(|| rand::thread_rng().gen());
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    priors.sample_into(config, &mut rng);
    seed
}

/// Build a config from defaults using the built-in prior set and a fixed sampling seed.
///
/// When `map_seed` is `None`, a random map seed is chosen (non-deterministic overall).
pub fn sample_default_config(sample_seed: u64, map_seed: Option<u64>) -> WorldGenConfig {
    let mut config = WorldGenConfig::default();
    sample_parameters(&mut config, &PriorSet::default_priors(), Some(sample_seed));
    config.seed = map_seed.unwrap_or_else(|| rand::thread_rng().gen());
    config
}

fn log_u(default: f64, factor: f64) -> PriorDist {
    PriorDist::LogUniform {
        min: default / factor,
        max: default * factor,
    }
}

fn entry(
    id: SampleParam,
    label: &'static str,
    category: &'static str,
    enabled: bool,
    dist: PriorDist,
) -> SampleableParam {
    SampleableParam {
        id,
        label,
        category,
        enabled,
        dist,
    }
}

fn around(center: f64, half_width: f64) -> PriorDist {
    PriorDist::Uniform {
        min: center - half_width,
        max: center + half_width,
    }
}

fn build_default_priors() -> Vec<SampleableParam> {
    vec![
        entry(
            SampleParam::MaxElevationM,
            "Max elevation (m)",
            "Vertical datum",
            true,
            log_u(9000.0, 1.35),
        ),
        entry(
            SampleParam::SeaLevelM,
            "Sea level (m)",
            "Vertical datum",
            false,
            around(0.0, 120.0),
        ),
        entry(
            SampleParam::OceanFloorM,
            "Ocean floor (m)",
            "Vertical datum",
            false,
            around(-6000.0, 1200.0),
        ),
        entry(
            SampleParam::ElevationWavelengthM,
            "Continent wavelength (m)",
            "Elevation noise",
            true,
            log_u(5120.0, 1.6),
        ),
        entry(
            SampleParam::DetailWavelengthM,
            "Detail wavelength (m)",
            "Elevation noise",
            true,
            log_u(1024.0, 1.7),
        ),
        entry(
            SampleParam::ElevationOctaves,
            "Elevation octaves",
            "Elevation noise",
            true,
            around(4.0, 1.5),
        ),
        entry(
            SampleParam::ElevationPersistence,
            "Elevation persistence",
            "Elevation noise",
            true,
            around(0.5, 0.12),
        ),
        entry(
            SampleParam::ElevationRidgeWeight,
            "Ridge weight",
            "Elevation noise",
            true,
            around(0.23, 0.07),
        ),
        entry(
            SampleParam::TargetLandFraction,
            "Target land fraction",
            "Elevation noise",
            true,
            around(0.35, 0.07),
        ),
        entry(
            SampleParam::EdgeOceanBias,
            "Edge ocean bias",
            "Elevation noise",
            true,
            around(0.12, 0.06),
        ),
        entry(
            SampleParam::MinLakeAreaM2,
            "Min lake area (m²)",
            "Water",
            false,
            log_u(9600.0, 2.5),
        ),
        entry(
            SampleParam::TemperatureRangeC,
            "Temperature range (°C)",
            "Temperature",
            true,
            around(65.0, 13.0),
        ),
        entry(
            SampleParam::TemperatureWavelengthM,
            "Temperature wavelength (m)",
            "Temperature",
            true,
            log_u(12_000.0, 1.6),
        ),
        entry(
            SampleParam::LapseRateCPerKm,
            "Lapse rate (°C/km)",
            "Temperature",
            true,
            around(6.5, 1.5),
        ),
        entry(
            SampleParam::RainfallScale,
            "Rainfall scale",
            "Rainfall",
            true,
            around(0.90, 0.18),
        ),
        entry(
            SampleParam::ContinentalityStrength,
            "Continentality strength",
            "Rainfall",
            true,
            around(0.08, 0.035),
        ),
        entry(
            SampleParam::InteriorDryingFactor,
            "Interior drying factor",
            "Rainfall",
            true,
            around(0.16, 0.06),
        ),
        entry(
            SampleParam::OrographicElevationWeight,
            "Orographic rain weight",
            "Rainfall",
            true,
            around(0.48, 0.10),
        ),
        entry(
            SampleParam::MountainMinElevationM,
            "Mountain min elevation (m)",
            "Mountains",
            true,
            around(2900.0, 500.0),
        ),
        entry(
            SampleParam::MountainMinRidgeInfluence,
            "Mountain min ridge influence",
            "Mountains",
            true,
            around(0.06, 0.025),
        ),
    ]
}

fn constrain_sampled_config(config: &mut WorldGenConfig) {
    if config.ocean_floor_m.0 >= config.sea_level_m.0 - 100.0 {
        config.ocean_floor_m = Meters(config.sea_level_m.0 - 500.0);
    }
    config.elevation_wavelength_m = Meters(config.elevation_wavelength_m.0.max(800.0));
    config.continent_wavelength_m = config.elevation_wavelength_m;
    if config.detail_wavelength_m.0 >= config.elevation_wavelength_m.0 {
        config.detail_wavelength_m = Meters(config.elevation_wavelength_m.0 / 4.0);
    }
    config.detail_wavelength_m = Meters(config.detail_wavelength_m.0.max(200.0));
    config.rainfall_scale = config.rainfall_scale.clamp(0.5, 1.25);
    config.edge_ocean_bias = config.edge_ocean_bias.clamp(0.0, 0.25);
    config.elevation_ridge_weight = config.elevation_ridge_weight.clamp(0.08, 0.35);
    config.continentality_strength = config.continentality_strength.clamp(0.02, 0.18);
    config.interior_drying_factor = config.interior_drying_factor.clamp(0.05, 0.28);
    config.orographic_elevation_weight = config.orographic_elevation_weight.clamp(0.25, 0.70);
    config.mountain_min_ridge_influence = config.mountain_min_ridge_influence.clamp(0.03, 0.12);
    if let Some(ref mut land) = config.target_land_fraction {
        *land = land.clamp(0.22, 0.48);
    }
}

fn apply_sample(config: &mut WorldGenConfig, id: SampleParam, value: f64) {
    match id {
        SampleParam::MaxElevationM => config.max_elevation_m = Meters(value.max(500.0)),
        SampleParam::SeaLevelM => config.sea_level_m = Meters(value),
        SampleParam::OceanFloorM => config.ocean_floor_m = Meters(value),
        SampleParam::ElevationWavelengthM => {
            config.elevation_wavelength_m = Meters(value.max(100.0));
            config.continent_wavelength_m = config.elevation_wavelength_m;
        }
        SampleParam::DetailWavelengthM => {
            config.detail_wavelength_m = Meters(value.max(100.0));
        }
        SampleParam::ElevationOctaves => {
            config.elevation_octaves = value.round().clamp(1.0, 12.0) as u32
        }
        SampleParam::ElevationPersistence => config.elevation_persistence = value.clamp(0.1, 0.95),
        SampleParam::ElevationRidgeWeight => {
            config.elevation_ridge_weight = value as f32;
        }
        SampleParam::TargetLandFraction => {
            config.target_land_fraction = Some(value as f32);
        }
        SampleParam::EdgeOceanBias => config.edge_ocean_bias = value as f32,
        SampleParam::MinLakeAreaM2 => config.min_lake_area_m2 = SquareMeters(value.max(1.0)),
        SampleParam::TemperatureRangeC => config.temperature_range_c = value.clamp(10.0, 120.0),
        SampleParam::LapseRateCPerKm => config.lapse_rate_c_per_km = value.clamp(0.0, 20.0),
        SampleParam::RainfallScale => config.rainfall_scale = value as f32,
        SampleParam::TemperatureWavelengthM => {
            config.temperature_wavelength_m = Meters(value.max(500.0))
        }
        SampleParam::ContinentalityStrength => config.continentality_strength = value as f32,
        SampleParam::InteriorDryingFactor => config.interior_drying_factor = value as f32,
        SampleParam::OrographicElevationWeight => {
            config.orographic_elevation_weight = value as f32;
        }
        SampleParam::MountainMinElevationM => {
            config.mountain_min_elevation_m = Meters(value.max(500.0));
        }
        SampleParam::MountainMinRidgeInfluence => {
            config.mountain_min_ridge_influence = value as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn param_value(config: &WorldGenConfig, id: SampleParam) -> f64 {
        match id {
            SampleParam::MaxElevationM => config.max_elevation_m.0,
            SampleParam::SeaLevelM => config.sea_level_m.0,
            SampleParam::OceanFloorM => config.ocean_floor_m.0,
            SampleParam::ElevationWavelengthM => config.elevation_wavelength_m.0,
            SampleParam::DetailWavelengthM => config.detail_wavelength_m.0,
            SampleParam::ElevationOctaves => config.elevation_octaves as f64,
            SampleParam::ElevationPersistence => config.elevation_persistence,
            SampleParam::ElevationRidgeWeight => config.elevation_ridge_weight as f64,
            SampleParam::TargetLandFraction => config.target_land_fraction.unwrap_or(0.35) as f64,
            SampleParam::EdgeOceanBias => config.edge_ocean_bias as f64,
            SampleParam::MinLakeAreaM2 => config.min_lake_area_m2.0,
            SampleParam::TemperatureRangeC => config.temperature_range_c,
            SampleParam::LapseRateCPerKm => config.lapse_rate_c_per_km,
            SampleParam::RainfallScale => config.rainfall_scale as f64,
            SampleParam::TemperatureWavelengthM => config.temperature_wavelength_m.0,
            SampleParam::ContinentalityStrength => config.continentality_strength as f64,
            SampleParam::InteriorDryingFactor => config.interior_drying_factor as f64,
            SampleParam::OrographicElevationWeight => config.orographic_elevation_weight as f64,
            SampleParam::MountainMinElevationM => config.mountain_min_elevation_m.0,
            SampleParam::MountainMinRidgeInfluence => config.mountain_min_ridge_influence as f64,
        }
    }

    #[test]
    fn sample_respects_prior_bounds() {
        let priors = PriorSet::default_priors();
        let mut rng = ChaCha8Rng::seed_from_u64(99);
        for _ in 0..200 {
            let mut config = WorldGenConfig::default();
            priors.sample_into(&mut config, &mut rng);
            for param in &priors.params {
                if !param.enabled {
                    continue;
                }
                let v = param_value(&config, param.id);
                match &param.dist {
                    PriorDist::Uniform { min, max } => {
                        assert!(
                            v >= *min - 1e-6 && v <= *max + 1e-6,
                            "{:?} out of uniform range",
                            param.id
                        );
                    }
                    PriorDist::LogUniform { min, max } => {
                        assert!(
                            v >= *min - 1e-6 && v <= *max + 1e-6,
                            "{:?} out of log range",
                            param.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn disabled_params_leave_defaults() {
        let mut priors = PriorSet::default_priors();
        priors.disable_all();
        let defaults = WorldGenConfig::default();
        let mut config = defaults.clone();
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        priors.sample_into(&mut config, &mut rng);
        assert_eq!(config.rainfall_scale, defaults.rainfall_scale);
        assert_eq!(config.elevation_octaves, defaults.elevation_octaves);
    }

    #[test]
    fn grid_fields_unchanged_by_sampling() {
        let priors = PriorSet::default_priors();
        let mut config = WorldGenConfig::default();
        config.seed = 12345;
        config.width = 256;
        config.height = 128;
        config.cell_size_m = Meters(75.0);
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        priors.sample_into(&mut config, &mut rng);
        assert_eq!(config.seed, 12345);
        assert_eq!(config.width, 256);
        assert_eq!(config.height, 128);
        assert_eq!(config.cell_size_m.0, 75.0);
    }

    #[test]
    fn sample_default_config_is_deterministic_with_fixed_seeds() {
        let a = sample_default_config(42, Some(7));
        let b = sample_default_config(42, Some(7));
        assert_eq!(a.rainfall_scale, b.rainfall_scale);
        assert_eq!(a.temperature_range_c, b.temperature_range_c);
        assert_eq!(a.seed, 7);
    }

    #[test]
    fn sample_parameters_returns_used_seed() {
        let mut config = WorldGenConfig::default();
        let used = sample_parameters(&mut config, &PriorSet::default_priors(), Some(123));
        assert_eq!(used, 123);
    }

    #[test]
    fn sampled_configs_stay_plausible() {
        use crate::generate_world;

        let priors = PriorSet::default_priors();
        let mut rng = ChaCha8Rng::seed_from_u64(2024);
        for i in 0..16 {
            let mut config = WorldGenConfig::test_config(i, 128);
            priors.sample_into(&mut config, &mut rng);
            let map = generate_world(&config);

            let land =
                map.water_mask.iter().filter(|&&w| !w).count() as f64 / map.water_mask.len() as f64;
            assert!(
                (0.18..=0.52).contains(&land),
                "sample {i}: land fraction {land} out of plausible range"
            );

            let unique = map
                .biome
                .iter()
                .filter(|&&b| !matches!(b, crate::Biome::Ocean | crate::Biome::Lake))
                .collect::<std::collections::HashSet<_>>()
                .len();
            assert!(unique >= 2, "sample {i}: only {unique} land biomes");

            for &v in map.rainfall.iter().chain(map.temperature.iter()) {
                assert!(v.is_finite() && (0.0..=1.0).contains(&v));
            }
        }
    }
}
