//! Prior distributions for random configuration sampling (GUI / CLI / library).

use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::config::WorldGenConfig;
use crate::units::{Meters, SquareMeters};

/// Cell size at which the built-in horizontal priors are expressed.
pub const REFERENCE_CELL_SIZE_M: f64 = 400.0;

fn horizontal_scale(config: &WorldGenConfig) -> f64 {
    let scale = config.cell_size_m.0 / REFERENCE_CELL_SIZE_M;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Identifies a single sampleable numerical config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleParam {
    MaxElevationM,
    SeaLevelM,
    OceanFloorM,
    ContinentWavelengthM,
    DetailWavelengthM,
    ElevationOctaves,
    ElevationPersistence,
    ElevationRidgeWeight,
    TargetLandFraction,
    EdgeOceanBias,
    MinLakeAreaM2,
    LapseRateCPerKm,
    RainfallScale,
    TemperatureWavelengthM,
    MountainMinElevationM,
    MountainMinRidgeInfluence,
    LatitudeDeg,
    WindFromDeg,
    BaseRainfallMm,
    TemperatureOffsetC,
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

    fn is_enabled(&self, id: SampleParam) -> bool {
        self.params.iter().any(|p| p.id == id && p.enabled)
    }

    /// Sample enabled parameters into `config`. Grid fields (seed, width, height, cell size)
    /// and enums are left unchanged, as are parameters whose prior is disabled.
    ///
    /// Horizontal priors are defined at [`REFERENCE_CELL_SIZE_M`] and scale with
    /// `config.cell_size_m`, so sampled features span the same number of cells.
    pub fn sample_into<R: Rng + ?Sized>(&self, config: &mut WorldGenConfig, rng: &mut R) {
        let scale = horizontal_scale(config);
        for param in &self.params {
            if !param.enabled {
                continue;
            }
            apply_sample(config, param.id, param.dist.sample_f64(rng), scale);
        }
        constrain_sampled_config(config, self, scale);
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
    use SampleParam as P;
    const DATUM: &str = "Vertical datum";
    const NOISE: &str = "Elevation noise";
    const CLIMATE: &str = "Climate";
    const MOUNTAINS: &str = "Mountains";
    vec![
        entry(
            P::MaxElevationM,
            "Max elevation (m)",
            DATUM,
            true,
            log_u(9000.0, 1.35),
        ),
        entry(
            P::SeaLevelM,
            "Sea level (m)",
            DATUM,
            false,
            around(0.0, 120.0),
        ),
        entry(
            P::OceanFloorM,
            "Ocean floor (m)",
            DATUM,
            false,
            around(-6000.0, 1200.0),
        ),
        entry(
            P::ContinentWavelengthM,
            "Continent wavelength (m)",
            NOISE,
            true,
            log_u(102_400.0, 1.6),
        ),
        entry(
            P::DetailWavelengthM,
            "Detail wavelength (m)",
            NOISE,
            true,
            log_u(20_480.0, 1.7),
        ),
        entry(
            P::ElevationOctaves,
            "Elevation octaves",
            NOISE,
            true,
            around(4.0, 1.5),
        ),
        entry(
            P::ElevationPersistence,
            "Elevation persistence",
            NOISE,
            true,
            around(0.5, 0.12),
        ),
        entry(
            P::ElevationRidgeWeight,
            "Ridge weight",
            NOISE,
            true,
            around(0.23, 0.07),
        ),
        entry(
            P::TargetLandFraction,
            "Target land fraction",
            NOISE,
            true,
            around(0.35, 0.07),
        ),
        entry(
            P::EdgeOceanBias,
            "Edge ocean bias",
            NOISE,
            true,
            around(0.12, 0.06),
        ),
        entry(
            P::MinLakeAreaM2,
            "Min lake area (m²)",
            "Water",
            false,
            log_u(3_840_000.0, 2.5),
        ),
        entry(
            P::LatitudeDeg,
            "Latitude (°)",
            CLIMATE,
            true,
            around(35.0, 25.0),
        ),
        entry(
            P::WindFromDeg,
            "Wind from (°)",
            CLIMATE,
            true,
            PriorDist::Uniform {
                min: 0.0,
                max: 360.0,
            },
        ),
        entry(
            P::TemperatureOffsetC,
            "Temperature offset (°C)",
            CLIMATE,
            true,
            around(0.0, 4.0),
        ),
        entry(
            P::LapseRateCPerKm,
            "Lapse rate (°C/km)",
            CLIMATE,
            true,
            around(6.5, 1.5),
        ),
        entry(
            P::BaseRainfallMm,
            "Base rainfall (mm/yr)",
            CLIMATE,
            true,
            log_u(1400.0, 1.6),
        ),
        entry(
            P::RainfallScale,
            "Rainfall scale",
            CLIMATE,
            true,
            around(0.90, 0.18),
        ),
        entry(
            P::TemperatureWavelengthM,
            "Climate noise wavelength (m)",
            CLIMATE,
            true,
            log_u(240_000.0, 1.6),
        ),
        entry(
            P::MountainMinElevationM,
            "Mountain min elevation (m)",
            MOUNTAINS,
            true,
            around(2900.0, 500.0),
        ),
        entry(
            P::MountainMinRidgeInfluence,
            "Mountain min ridge influence",
            MOUNTAINS,
            true,
            around(0.06, 0.025),
        ),
    ]
}

/// Keep sampled values plausible without touching parameters the user excluded.
fn constrain_sampled_config(config: &mut WorldGenConfig, priors: &PriorSet, scale: f64) {
    use SampleParam as P;
    let on = |id| priors.is_enabled(id);

    if (on(P::SeaLevelM) || on(P::OceanFloorM))
        && config.ocean_floor_m.0 >= config.sea_level_m.0 - 100.0
    {
        config.ocean_floor_m = Meters(config.sea_level_m.0 - 500.0);
    }
    if on(P::ContinentWavelengthM) {
        config.continent_wavelength_m =
            Meters(config.continent_wavelength_m.0.max(16_000.0 * scale));
    }
    if on(P::ContinentWavelengthM) || on(P::DetailWavelengthM) {
        let continent = config.continent_wavelength_m.0;
        if config.detail_wavelength_m.0 >= continent {
            config.detail_wavelength_m = Meters(continent / 4.0);
        }
        config.detail_wavelength_m = Meters(config.detail_wavelength_m.0.max(4_000.0 * scale));
    }
    if on(P::RainfallScale) {
        config.rainfall_scale = config.rainfall_scale.clamp(0.5, 1.25);
    }
    if on(P::EdgeOceanBias) {
        config.edge_ocean_bias = config.edge_ocean_bias.clamp(0.0, 0.25);
    }
    if on(P::ElevationRidgeWeight) {
        config.elevation_ridge_weight = config.elevation_ridge_weight.clamp(0.08, 0.35);
    }
    if on(P::MountainMinRidgeInfluence) {
        config.mountain_min_ridge_influence = config.mountain_min_ridge_influence.clamp(0.03, 0.12);
    }
    if on(P::TargetLandFraction) {
        if let Some(ref mut land) = config.target_land_fraction {
            *land = land.clamp(0.22, 0.48);
        }
    }
}

fn apply_sample(config: &mut WorldGenConfig, id: SampleParam, value: f64, scale: f64) {
    match id {
        SampleParam::MaxElevationM => config.max_elevation_m = Meters(value.max(500.0)),
        SampleParam::SeaLevelM => config.sea_level_m = Meters(value),
        SampleParam::OceanFloorM => config.ocean_floor_m = Meters(value),
        SampleParam::ContinentWavelengthM => {
            config.continent_wavelength_m = Meters((value * scale).max(2_000.0 * scale));
        }
        SampleParam::DetailWavelengthM => {
            config.detail_wavelength_m = Meters((value * scale).max(2_000.0 * scale));
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
        SampleParam::MinLakeAreaM2 => {
            config.min_lake_area_m2 = SquareMeters((value * scale * scale).max(1.0));
        }
        SampleParam::LapseRateCPerKm => config.lapse_rate_c_per_km = value.clamp(0.0, 20.0),
        SampleParam::RainfallScale => config.rainfall_scale = value as f32,
        SampleParam::TemperatureWavelengthM => {
            config.temperature_wavelength_m = Meters((value * scale).max(10_000.0 * scale));
        }
        SampleParam::MountainMinElevationM => {
            config.mountain_min_elevation_m = Meters(value.max(500.0));
        }
        SampleParam::MountainMinRidgeInfluence => {
            config.mountain_min_ridge_influence = value as f32;
        }
        SampleParam::LatitudeDeg => config.regional_climate.latitude_deg = value,
        SampleParam::WindFromDeg => config.regional_climate.wind_from_deg = value,
        SampleParam::BaseRainfallMm => config.regional_climate.base_rainfall_mm = value,
        SampleParam::TemperatureOffsetC => config.regional_climate.temperature_offset_c = value,
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
            SampleParam::ContinentWavelengthM => config.continent_wavelength_m.0,
            SampleParam::DetailWavelengthM => config.detail_wavelength_m.0,
            SampleParam::ElevationOctaves => config.elevation_octaves as f64,
            SampleParam::ElevationPersistence => config.elevation_persistence,
            SampleParam::ElevationRidgeWeight => config.elevation_ridge_weight as f64,
            SampleParam::TargetLandFraction => config.target_land_fraction.unwrap_or(0.35) as f64,
            SampleParam::EdgeOceanBias => config.edge_ocean_bias as f64,
            SampleParam::MinLakeAreaM2 => config.min_lake_area_m2.0,
            SampleParam::LapseRateCPerKm => config.lapse_rate_c_per_km,
            SampleParam::RainfallScale => config.rainfall_scale as f64,
            SampleParam::TemperatureWavelengthM => config.temperature_wavelength_m.0,
            SampleParam::MountainMinElevationM => config.mountain_min_elevation_m.0,
            SampleParam::MountainMinRidgeInfluence => config.mountain_min_ridge_influence as f64,
            SampleParam::LatitudeDeg => config.regional_climate.latitude_deg,
            SampleParam::WindFromDeg => config.regional_climate.wind_from_deg,
            SampleParam::BaseRainfallMm => config.regional_climate.base_rainfall_mm,
            SampleParam::TemperatureOffsetC => config.regional_climate.temperature_offset_c,
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
                let (PriorDist::Uniform { min, max } | PriorDist::LogUniform { min, max }) =
                    &param.dist;
                assert!(
                    v >= *min - 1e-6 && v <= *max + 1e-6,
                    "{:?} out of range",
                    param.id
                );
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
        let mut config = WorldGenConfig {
            seed: 12345,
            width: 256,
            height: 128,
            cell_size_m: Meters(75.0),
            ..WorldGenConfig::default()
        };
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        priors.sample_into(&mut config, &mut rng);
        assert_eq!(config.seed, 12345);
        assert_eq!(config.width, 256);
        assert_eq!(config.height, 128);
        assert_eq!(config.cell_size_m.0, 75.0);
    }

    #[test]
    fn disabled_priors_leave_user_values_untouched() {
        let mut priors = PriorSet::default_priors();
        priors.disable_all();
        let mut config = WorldGenConfig {
            continent_wavelength_m: Meters(150_000.0),
            rainfall_scale: 2.0,
            target_land_fraction: Some(0.7),
            ..WorldGenConfig::default()
        };
        let before = serde_json::to_value(&config).unwrap();
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        priors.sample_into(&mut config, &mut rng);
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
    }

    #[test]
    fn horizontal_priors_scale_with_cell_size() {
        let priors = PriorSet::default_priors();
        let base = WorldGenConfig::default();
        let coarse = base.rescaled_to_cell_size(Meters(2000.0));
        let mut rng_a = ChaCha8Rng::seed_from_u64(11);
        let mut rng_b = ChaCha8Rng::seed_from_u64(11);
        for _ in 0..100 {
            let a = priors.sample_config(&base, &mut rng_a);
            let b = priors.sample_config(&coarse, &mut rng_b);
            for (fine, wide) in [
                (a.continent_wavelength_m.0, b.continent_wavelength_m.0),
                (a.detail_wavelength_m.0, b.detail_wavelength_m.0),
                (a.temperature_wavelength_m.0, b.temperature_wavelength_m.0),
            ] {
                assert!((wide / fine - 5.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn sampled_configs_validate() {
        let priors = PriorSet::default_priors();
        let mut rng = ChaCha8Rng::seed_from_u64(5);
        let base = WorldGenConfig::default();
        for config in [base.clone(), base.rescaled_to_cell_size(Meters(50.0))] {
            for _ in 0..200 {
                assert_eq!(priors.sample_config(&config, &mut rng).validate(), Ok(()));
            }
        }
    }

    #[test]
    fn sampling_varies_regional_climate() {
        let priors = PriorSet::default_priors();
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let base = WorldGenConfig::default();
        for _ in 0..100 {
            assert_ne!(
                priors.sample_config(&base, &mut rng).regional_climate,
                base.regional_climate
            );
        }
    }

    #[test]
    fn sample_default_config_is_deterministic_with_fixed_seeds() {
        let a = sample_default_config(42, Some(7));
        let b = sample_default_config(42, Some(7));
        assert_eq!(a.rainfall_scale, b.rainfall_scale);
        assert_eq!(a.regional_climate, b.regional_climate);
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
                (0.15..=0.55).contains(&land),
                "sample {i}: land fraction {land} out of plausible range"
            );

            let unique = map
                .biome
                .iter()
                .filter(|&&b| !matches!(b, crate::Biome::Ocean | crate::Biome::Lake))
                .collect::<std::collections::HashSet<_>>()
                .len();
            assert!(unique >= 1, "sample {i}: no land biomes");

            for &v in map.rainfall.iter().chain(map.temperature.iter()) {
                assert!(v.is_finite() && (0.0..=1.0).contains(&v));
            }
        }
    }
}
