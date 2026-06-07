//! Prior distributions for random configuration sampling (GUI / exploration).

use rand::Rng;

use crate::config::WorldGenConfig;
use crate::units::{Celsius, Degrees, Meters, SquareKilometers, SquareMeters};

/// Identifies a single sampleable numerical config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleParam {
    PlateDensityPerKm2,
    DrunkardWalkerDensityPerKm2,
    MaxElevationM,
    SeaLevelM,
    OceanFloorM,
    ContinentalMarginM,
    MinIsthmusWidthM,
    MountainBeltWidthM,
    MountainCoastBufferM,
    CoastCleanupProximityM,
    DrunkardBrushRadiusM,
    RiverMinLengthM,
    MinLakeAreaM2,
    RiverMinDrainageAreaKm2,
    RiverTributaryDrainageAreaKm2,
    MountainMinElevationM,
    MountainMinSlopeDeg,
    EquatorMeanTempC,
    PoleMeanTempC,
    LapseRateCPerKm,
    RainfallScale,
    ContinentWavelengthM,
    HillWavelengthM,
    MountainDetailWavelengthM,
    LandMaskWavelengthM,
    LandShapeCellSizeM,
    TemperatureWavelengthM,
    OrogenyMountainThreshold,
    MountainClusterThreshold,
    PlateBoundaryStrength,
    ContinentalPlateFraction,
    OceanicUpliftFactor,
    CaFillProbability,
    CaIterations,
    CaSmoothingPasses,
    CaCoarseCellSizeM,
    LandMaskBlurM,
    LandMaskCloseRadiusM,
    MinLandmassAreaKm2,
    MaxLandmassDensityPerKm2,
    OrogenyPeakRadiusM,
    DrunkardPathLengthM,
    MaxLandmassCompactness,
    HybridNoiseBlend,
    CoastSharpening,
    CoastCleanupPasses,
    MountainBoundaryWeight,
    RiverMeanderStrength,
    OrogenyInteriorMinDistM,
    ShelfWidthM,
    ShelfDepthM,
    PlateLloydIterations,
    ContinentalPlateSpeedMax,
    OceanicPlateSpeedMin,
    MantleFlowAngleDeg,
    OrographicOrogenyWeight,
    InteriorDryingFactor,
    ContinentalityStrength,
    ContinentalityOceanRangeM,
    TectonicUpliftScale,
    LandTextureStrengthM,
    LandTextureCoastBandM,
    IslandZoneM,
    CoarseHydroFactor,
    LandscapeEvolutionFullResPasses,
    LandscapeEvolutionIterations,
    LandscapeErosionFactor,
    LandscapeUpliftFactor,
    ErodibilityPlains,
    ErodibilityMountains,
    RiverIncisionFactor,
    RainfallErodibilityCoupling,
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

    /// Sample enabled parameters into `config`. Grid fields (seed, width, height, cell size),
    /// enums, and boolean toggles are left unchanged.
    pub fn sample_into<R: Rng + ?Sized>(&self, config: &mut WorldGenConfig, rng: &mut R) {
        for param in &self.params {
            if !param.enabled {
                continue;
            }
            apply_sample(config, param.id, param.dist.sample_f64(rng));
        }
        if config.pole_mean_temp_c.0 >= config.equator_mean_temp_c.0 - 5.0 {
            config.pole_mean_temp_c =
                Celsius((config.equator_mean_temp_c.0 - 15.0).clamp(-60.0, 20.0));
        }
        if config.ocean_floor_m.0 >= config.sea_level_m.0 - 100.0 {
            config.ocean_floor_m = Meters(config.sea_level_m.0 - 500.0);
        }
    }
}

fn u(min: f64, max: f64) -> PriorDist {
    PriorDist::Uniform { min, max }
}

fn log_u(default: f64, factor: f64) -> PriorDist {
    PriorDist::LogUniform {
        min: default / factor,
        max: default * factor,
    }
}

fn frac(default: f64, spread: f64) -> PriorDist {
    u((default - spread).max(0.0), (default + spread).min(1.0))
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

fn build_default_priors() -> Vec<SampleableParam> {
    vec![
        // Densities
        entry(
            SampleParam::PlateDensityPerKm2,
            "Plate density (per km²)",
            "Densities",
            true,
            u(0.05, 0.65),
        ),
        entry(
            SampleParam::DrunkardWalkerDensityPerKm2,
            "Drunkard walker density (per km²)",
            "Densities",
            true,
            u(0.04, 0.55),
        ),
        entry(
            SampleParam::MaxLandmassDensityPerKm2,
            "Max landmass density (per km²)",
            "Densities",
            false,
            u(0.01, 0.15),
        ),
        // Vertical datum
        entry(
            SampleParam::MaxElevationM,
            "Max elevation (m)",
            "Vertical datum",
            true,
            log_u(9000.0, 2.0),
        ),
        entry(
            SampleParam::SeaLevelM,
            "Sea level (m)",
            "Vertical datum",
            true,
            u(-400.0, 400.0),
        ),
        entry(
            SampleParam::OceanFloorM,
            "Ocean floor (m)",
            "Vertical datum",
            true,
            u(-9000.0, -1500.0),
        ),
        // Horizontal distances
        entry(
            SampleParam::ContinentalMarginM,
            "Continental margin (m)",
            "Horizontal distances",
            true,
            log_u(200.0, 4.0),
        ),
        entry(
            SampleParam::MinIsthmusWidthM,
            "Min isthmus width (m)",
            "Horizontal distances",
            false,
            log_u(120.0, 4.0),
        ),
        entry(
            SampleParam::MountainBeltWidthM,
            "Mountain belt width (m)",
            "Horizontal distances",
            true,
            log_u(60.0, 4.0),
        ),
        entry(
            SampleParam::MountainCoastBufferM,
            "Mountain coast buffer (m)",
            "Horizontal distances",
            true,
            log_u(120.0, 3.0),
        ),
        entry(
            SampleParam::CoastCleanupProximityM,
            "Coast cleanup proximity (m)",
            "Horizontal distances",
            false,
            log_u(80.0, 4.0),
        ),
        entry(
            SampleParam::DrunkardBrushRadiusM,
            "Drunkard brush radius (m)",
            "Horizontal distances",
            true,
            log_u(200.0, 4.0),
        ),
        entry(
            SampleParam::RiverMinLengthM,
            "River min length (m)",
            "Horizontal distances",
            false,
            log_u(100.0, 4.0),
        ),
        // Areas
        entry(
            SampleParam::MinLakeAreaM2,
            "Min lake area (m²)",
            "Areas",
            false,
            log_u(9600.0, 5.0),
        ),
        entry(
            SampleParam::RiverMinDrainageAreaKm2,
            "River min drainage (km²)",
            "Areas",
            false,
            log_u(0.01536, 5.0),
        ),
        entry(
            SampleParam::RiverTributaryDrainageAreaKm2,
            "River tributary drainage (km²)",
            "Areas",
            false,
            log_u(0.00486, 5.0),
        ),
        // Mountains
        entry(
            SampleParam::MountainMinElevationM,
            "Mountain min elevation (m)",
            "Mountains",
            true,
            u(2500.0, 6500.0),
        ),
        entry(
            SampleParam::MountainMinSlopeDeg,
            "Mountain min slope (°)",
            "Mountains",
            false,
            u(2.0, 12.0),
        ),
        entry(
            SampleParam::OrogenyMountainThreshold,
            "Orogeny mountain threshold",
            "Mountains",
            true,
            frac(0.18, 0.12),
        ),
        entry(
            SampleParam::MountainClusterThreshold,
            "Mountain cluster threshold",
            "Mountains",
            false,
            frac(0.55, 0.2),
        ),
        entry(
            SampleParam::MountainBoundaryWeight,
            "Mountain boundary weight",
            "Mountains",
            true,
            u(0.2, 1.0),
        ),
        entry(
            SampleParam::OrogenyInteriorMinDistM,
            "Orogeny interior min dist (m)",
            "Mountains",
            false,
            log_u(120.0, 3.0),
        ),
        entry(
            SampleParam::OrogenyPeakRadiusM,
            "Orogeny peak radius (m)",
            "Mountains",
            false,
            log_u(60.0, 3.0),
        ),
        // Oceans
        entry(
            SampleParam::ShelfWidthM,
            "Shelf width (m)",
            "Oceans",
            true,
            log_u(80.0, 4.0),
        ),
        entry(
            SampleParam::ShelfDepthM,
            "Shelf depth (m)",
            "Oceans",
            true,
            log_u(200.0, 3.0),
        ),
        // Land texture / mask
        entry(
            SampleParam::LandTextureStrengthM,
            "Land texture strength (m)",
            "Land texture",
            true,
            log_u(400.0, 4.0),
        ),
        entry(
            SampleParam::LandTextureCoastBandM,
            "Land texture coast band (m)",
            "Land texture",
            true,
            log_u(2000.0, 3.0),
        ),
        entry(
            SampleParam::IslandZoneM,
            "Island zone (m)",
            "Land texture",
            true,
            log_u(5000.0, 3.0),
        ),
        entry(
            SampleParam::HybridNoiseBlend,
            "Hybrid noise blend",
            "Land texture",
            true,
            frac(0.30, 0.25),
        ),
        entry(
            SampleParam::CaFillProbability,
            "CA fill probability",
            "Land texture",
            true,
            u(0.38, 0.68),
        ),
        entry(
            SampleParam::CaIterations,
            "CA iterations",
            "Land texture",
            false,
            u(3.0, 12.0),
        ),
        entry(
            SampleParam::CaSmoothingPasses,
            "CA smoothing passes",
            "Land texture",
            false,
            u(1.0, 8.0),
        ),
        entry(
            SampleParam::CaCoarseCellSizeM,
            "CA coarse cell size (m)",
            "Land texture",
            false,
            log_u(80.0, 3.0),
        ),
        entry(
            SampleParam::LandShapeCellSizeM,
            "Land shape cell size (m)",
            "Land texture",
            true,
            log_u(50.0, 3.0),
        ),
        entry(
            SampleParam::LandMaskBlurM,
            "Land mask blur (m)",
            "Land texture",
            true,
            log_u(80.0, 3.0),
        ),
        entry(
            SampleParam::LandMaskCloseRadiusM,
            "Land mask close radius (m)",
            "Land texture",
            false,
            log_u(20.0, 4.0),
        ),
        entry(
            SampleParam::MinLandmassAreaKm2,
            "Min landmass area (km²)",
            "Land texture",
            true,
            log_u(0.14, 5.0),
        ),
        entry(
            SampleParam::DrunkardPathLengthM,
            "Drunkard path length (m)",
            "Land texture",
            true,
            log_u(119_160.0, 2.5),
        ),
        entry(
            SampleParam::MaxLandmassCompactness,
            "Max landmass compactness",
            "Land texture",
            false,
            u(20.0, 120.0),
        ),
        // Plates
        entry(
            SampleParam::ContinentalPlateFraction,
            "Continental plate fraction",
            "Plates",
            true,
            u(0.15, 0.65),
        ),
        entry(
            SampleParam::OceanicUpliftFactor,
            "Oceanic uplift factor",
            "Plates",
            true,
            u(0.02, 0.35),
        ),
        entry(
            SampleParam::PlateBoundaryStrength,
            "Plate boundary strength",
            "Plates",
            true,
            u(0.06, 0.45),
        ),
        entry(
            SampleParam::PlateLloydIterations,
            "Lloyd relaxation iterations",
            "Plates",
            false,
            u(0.0, 5.0),
        ),
        entry(
            SampleParam::ContinentalPlateSpeedMax,
            "Continental plate speed max",
            "Plates",
            true,
            u(0.1, 0.9),
        ),
        entry(
            SampleParam::OceanicPlateSpeedMin,
            "Oceanic plate speed min",
            "Plates",
            true,
            u(0.1, 0.9),
        ),
        entry(
            SampleParam::MantleFlowAngleDeg,
            "Mantle flow angle (°)",
            "Plates",
            false,
            u(-90.0, 90.0),
        ),
        // Coast
        entry(
            SampleParam::CoastSharpening,
            "Coast sharpening",
            "Coast",
            true,
            frac(0.15, 0.12),
        ),
        entry(
            SampleParam::CoastCleanupPasses,
            "Coast cleanup passes",
            "Coast",
            false,
            u(0.0, 5.0),
        ),
        // Climate
        entry(
            SampleParam::EquatorMeanTempC,
            "Equator mean temp (°C)",
            "Climate",
            true,
            u(18.0, 38.0),
        ),
        entry(
            SampleParam::PoleMeanTempC,
            "Pole mean temp (°C)",
            "Climate",
            true,
            u(-45.0, 5.0),
        ),
        entry(
            SampleParam::TemperatureWavelengthM,
            "Temperature wavelength (m)",
            "Climate",
            true,
            log_u(12_000.0, 3.0),
        ),
        entry(
            SampleParam::LapseRateCPerKm,
            "Lapse rate (°C/km)",
            "Climate",
            true,
            u(4.0, 9.5),
        ),
        entry(
            SampleParam::RainfallScale,
            "Rainfall scale",
            "Climate",
            true,
            u(0.4, 2.0),
        ),
        entry(
            SampleParam::OrographicOrogenyWeight,
            "Orographic rainfall weight",
            "Climate",
            true,
            u(0.2, 1.2),
        ),
        entry(
            SampleParam::InteriorDryingFactor,
            "Interior drying factor",
            "Climate",
            true,
            u(0.02, 0.2),
        ),
        entry(
            SampleParam::ContinentalityStrength,
            "Continentality strength",
            "Climate",
            true,
            u(0.04, 0.28),
        ),
        entry(
            SampleParam::ContinentalityOceanRangeM,
            "Continentality ocean range (m)",
            "Climate",
            false,
            log_u(8000.0, 2.5),
        ),
        // Land generation
        entry(
            SampleParam::TectonicUpliftScale,
            "Tectonic uplift scale",
            "Land generation",
            true,
            u(0.4, 1.8),
        ),
        // Landscape evolution
        entry(
            SampleParam::CoarseHydroFactor,
            "Coarse hydro factor",
            "Landscape",
            false,
            u(2.0, 8.0),
        ),
        entry(
            SampleParam::LandscapeEvolutionFullResPasses,
            "LEM full-res passes",
            "Landscape",
            false,
            u(0.0, 6.0),
        ),
        entry(
            SampleParam::LandscapeEvolutionIterations,
            "LEM iterations",
            "Landscape",
            true,
            u(4.0, 24.0),
        ),
        entry(
            SampleParam::LandscapeErosionFactor,
            "LEM erosion factor",
            "Landscape",
            true,
            u(0.0005, 0.008),
        ),
        entry(
            SampleParam::LandscapeUpliftFactor,
            "LEM uplift factor",
            "Landscape",
            true,
            u(0.001, 0.015),
        ),
        entry(
            SampleParam::ErodibilityPlains,
            "Erodibility plains",
            "Landscape",
            false,
            u(1.5, 8.0),
        ),
        entry(
            SampleParam::ErodibilityMountains,
            "Erodibility mountains",
            "Landscape",
            false,
            u(0.5, 3.0),
        ),
        entry(
            SampleParam::RainfallErodibilityCoupling,
            "Rainfall erodibility coupling",
            "Landscape",
            false,
            u(0.05, 0.35),
        ),
        // Rivers
        entry(
            SampleParam::RiverIncisionFactor,
            "River incision factor",
            "Rivers",
            false,
            u(0.0005, 0.01),
        ),
        entry(
            SampleParam::RiverMeanderStrength,
            "River meander strength",
            "Rivers",
            true,
            u(0.0, 0.35),
        ),
        // Noise wavelengths
        entry(
            SampleParam::ContinentWavelengthM,
            "Continent wavelength (m)",
            "Noise wavelengths",
            true,
            log_u(5120.0, 3.0),
        ),
        entry(
            SampleParam::HillWavelengthM,
            "Hill wavelength (m)",
            "Noise wavelengths",
            true,
            log_u(1706.67, 3.0),
        ),
        entry(
            SampleParam::MountainDetailWavelengthM,
            "Mountain detail wavelength (m)",
            "Noise wavelengths",
            true,
            log_u(1024.0, 3.0),
        ),
        entry(
            SampleParam::LandMaskWavelengthM,
            "Land mask wavelength (m)",
            "Noise wavelengths",
            true,
            log_u(20480.0, 2.5),
        ),
    ]
}

fn apply_sample(config: &mut WorldGenConfig, id: SampleParam, value: f64) {
    match id {
        SampleParam::PlateDensityPerKm2 => config.plate_density_per_km2 = value.max(0.001),
        SampleParam::DrunkardWalkerDensityPerKm2 => {
            config.drunkard_walker_density_per_km2 = value.max(0.0)
        }
        SampleParam::MaxElevationM => config.max_elevation_m = Meters(value.max(500.0)),
        SampleParam::SeaLevelM => config.sea_level_m = Meters(value),
        SampleParam::OceanFloorM => config.ocean_floor_m = Meters(value),
        SampleParam::ContinentalMarginM => config.continental_margin_m = Meters(value.max(1.0)),
        SampleParam::MinIsthmusWidthM => config.min_isthmus_width_m = Meters(value.max(1.0)),
        SampleParam::MountainBeltWidthM => config.mountain_belt_width_m = Meters(value.max(1.0)),
        SampleParam::MountainCoastBufferM => {
            config.mountain_coast_buffer_m = Meters(value.max(1.0))
        }
        SampleParam::CoastCleanupProximityM => {
            config.coast_cleanup_proximity_m = Meters(value.max(1.0))
        }
        SampleParam::DrunkardBrushRadiusM => config.drunkard_brush_radius_m = Meters(value.max(1.0)),
        SampleParam::RiverMinLengthM => config.river_min_length_m = Meters(value.max(1.0)),
        SampleParam::MinLakeAreaM2 => config.min_lake_area_m2 = SquareMeters(value.max(1.0)),
        SampleParam::RiverMinDrainageAreaKm2 => {
            config.river_min_drainage_area_km2 = SquareKilometers(value.max(1e-6))
        }
        SampleParam::RiverTributaryDrainageAreaKm2 => {
            config.river_tributary_drainage_area_km2 = SquareKilometers(value.max(1e-6))
        }
        SampleParam::MountainMinElevationM => {
            config.mountain_min_elevation_m = Meters(value.max(0.0))
        }
        SampleParam::MountainMinSlopeDeg => {
            config.mountain_min_slope_deg = Degrees(value.clamp(0.5, 45.0))
        }
        SampleParam::EquatorMeanTempC => config.equator_mean_temp_c = Celsius(value),
        SampleParam::PoleMeanTempC => config.pole_mean_temp_c = Celsius(value),
        SampleParam::LapseRateCPerKm => config.lapse_rate_c_per_km = value.clamp(0.0, 20.0),
        SampleParam::RainfallScale => config.rainfall_scale = value as f32,
        SampleParam::ContinentWavelengthM => config.continent_wavelength_m = Meters(value.max(100.0)),
        SampleParam::HillWavelengthM => config.hill_wavelength_m = Meters(value.max(50.0)),
        SampleParam::MountainDetailWavelengthM => {
            config.mountain_detail_wavelength_m = Meters(value.max(50.0))
        }
        SampleParam::LandMaskWavelengthM => config.land_mask_wavelength_m = Meters(value.max(100.0)),
        SampleParam::LandShapeCellSizeM => config.land_shape_cell_size_m = Meters(value.max(5.0)),
        SampleParam::TemperatureWavelengthM => {
            config.temperature_wavelength_m = Meters(value.max(500.0))
        }
        SampleParam::OrogenyMountainThreshold => {
            config.orogeny_mountain_threshold = value.clamp(0.0, 1.0) as f32
        }
        SampleParam::MountainClusterThreshold => {
            config.mountain_cluster_threshold = value.clamp(0.0, 1.0) as f32
        }
        SampleParam::PlateBoundaryStrength => {
            config.plate_boundary_strength = value.clamp(0.0, 2.0) as f32
        }
        SampleParam::ContinentalPlateFraction => {
            config.continental_plate_fraction = value.clamp(0.0, 1.0) as f32
        }
        SampleParam::OceanicUpliftFactor => {
            config.oceanic_uplift_factor = value.clamp(0.0, 2.0) as f32
        }
        SampleParam::CaFillProbability => config.ca_fill_probability = value.clamp(0.0, 1.0) as f32,
        SampleParam::CaIterations => config.ca_iterations = value.round().clamp(1.0, 64.0) as u32,
        SampleParam::CaSmoothingPasses => {
            config.ca_smoothing_passes = value.round().clamp(0.0, 32.0) as u32
        }
        SampleParam::CaCoarseCellSizeM => config.ca_coarse_cell_size_m = Meters(value.max(5.0)),
        SampleParam::LandMaskBlurM => config.land_mask_blur_m = Meters(value.max(1.0)),
        SampleParam::LandMaskCloseRadiusM => config.land_mask_close_radius_m = Meters(value.max(0.0)),
        SampleParam::MinLandmassAreaKm2 => {
            config.min_landmass_area_km2 = SquareKilometers(value.max(0.0))
        }
        SampleParam::MaxLandmassDensityPerKm2 => {
            config.max_landmass_density_per_km2 = value.max(0.001)
        }
        SampleParam::OrogenyPeakRadiusM => config.orogeny_peak_radius_m = Meters(value.max(1.0)),
        SampleParam::DrunkardPathLengthM => config.drunkard_path_length_m = Meters(value.max(1000.0)),
        SampleParam::MaxLandmassCompactness => {
            config.max_landmass_compactness = value.clamp(1.0, 500.0) as f32
        }
        SampleParam::HybridNoiseBlend => config.hybrid_noise_blend = value.clamp(0.0, 1.0) as f32,
        SampleParam::CoastSharpening => config.coast_sharpening = value.clamp(0.0, 1.0) as f32,
        SampleParam::CoastCleanupPasses => {
            config.coast_cleanup_passes = value.round().clamp(0.0, 16.0) as u32
        }
        SampleParam::MountainBoundaryWeight => {
            config.mountain_boundary_weight = value.clamp(0.0, 2.0) as f32
        }
        SampleParam::RiverMeanderStrength => {
            config.river_meander_strength = value.clamp(0.0, 1.0) as f32
        }
        SampleParam::OrogenyInteriorMinDistM => {
            config.orogeny_interior_min_dist_m = Meters(value.max(1.0))
        }
        SampleParam::ShelfWidthM => config.shelf_width_m = Meters(value.max(1.0)),
        SampleParam::ShelfDepthM => config.shelf_depth_m = Meters(value.max(1.0)),
        SampleParam::PlateLloydIterations => {
            config.plate_lloyd_iterations = value.round().clamp(0.0, 8.0) as u32
        }
        SampleParam::ContinentalPlateSpeedMax => {
            config.continental_plate_speed_max = value.clamp(0.0, 2.0) as f32
        }
        SampleParam::OceanicPlateSpeedMin => {
            config.oceanic_plate_speed_min = value.clamp(0.0, 2.0) as f32
        }
        SampleParam::MantleFlowAngleDeg => {
            config.mantle_flow_angle_deg = value.clamp(-180.0, 180.0)
        }
        SampleParam::OrographicOrogenyWeight => {
            config.orographic_orogeny_weight = value.clamp(0.0, 2.0) as f32
        }
        SampleParam::InteriorDryingFactor => {
            config.interior_drying_factor = value.clamp(0.0, 1.0) as f32
        }
        SampleParam::ContinentalityStrength => {
            config.continentality_strength = value.clamp(0.0, 1.0) as f32
        }
        SampleParam::ContinentalityOceanRangeM => {
            config.continentality_ocean_range_m = Meters(value.max(100.0))
        }
        SampleParam::TectonicUpliftScale => {
            config.tectonic_uplift_scale = value.clamp(0.0, 3.0) as f32
        }
        SampleParam::LandTextureStrengthM => config.land_texture_strength_m = Meters(value.max(0.0)),
        SampleParam::LandTextureCoastBandM => {
            config.land_texture_coast_band_m = Meters(value.max(50.0))
        }
        SampleParam::IslandZoneM => config.island_zone_m = Meters(value.max(100.0)),
        SampleParam::CoarseHydroFactor => {
            config.coarse_hydro_factor = value.round().clamp(1.0, 16.0) as u32
        }
        SampleParam::LandscapeEvolutionFullResPasses => {
            config.landscape_evolution_full_res_passes =
                value.round().clamp(0.0, 32.0) as u32
        }
        SampleParam::LandscapeEvolutionIterations => {
            config.landscape_evolution_iterations = value.round().clamp(1.0, 64.0) as u32
        }
        SampleParam::LandscapeErosionFactor => {
            config.landscape_erosion_factor = value.clamp(0.0, 0.05) as f32
        }
        SampleParam::LandscapeUpliftFactor => {
            config.landscape_uplift_factor = value.clamp(0.0, 0.05) as f32
        }
        SampleParam::ErodibilityPlains => config.erodibility_plains = value.clamp(0.1, 10.0) as f32,
        SampleParam::ErodibilityMountains => {
            config.erodibility_mountains = value.clamp(0.1, 10.0) as f32
        }
        SampleParam::RiverIncisionFactor => {
            config.river_incision_factor = value.clamp(0.0, 0.05) as f32
        }
        SampleParam::RainfallErodibilityCoupling => {
            config.rainfall_erodibility_coupling = value.clamp(0.0, 1.0) as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

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
                let v = read_param(&config, param.id);
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
        assert_eq!(config.plate_density_per_km2, defaults.plate_density_per_km2);
        assert_eq!(config.rainfall_scale, defaults.rainfall_scale);
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

    fn read_param(config: &WorldGenConfig, id: SampleParam) -> f64 {
        param_value(config, id)
    }
}

/// Read the current scalar value for a sampleable parameter.
#[allow(dead_code)]
pub fn param_value(config: &WorldGenConfig, id: SampleParam) -> f64 {
    match id {
        SampleParam::PlateDensityPerKm2 => config.plate_density_per_km2,
        SampleParam::DrunkardWalkerDensityPerKm2 => config.drunkard_walker_density_per_km2,
        SampleParam::MaxElevationM => config.max_elevation_m.0,
        SampleParam::SeaLevelM => config.sea_level_m.0,
        SampleParam::OceanFloorM => config.ocean_floor_m.0,
        SampleParam::ContinentalMarginM => config.continental_margin_m.0,
        SampleParam::MinIsthmusWidthM => config.min_isthmus_width_m.0,
        SampleParam::MountainBeltWidthM => config.mountain_belt_width_m.0,
        SampleParam::MountainCoastBufferM => config.mountain_coast_buffer_m.0,
        SampleParam::CoastCleanupProximityM => config.coast_cleanup_proximity_m.0,
        SampleParam::DrunkardBrushRadiusM => config.drunkard_brush_radius_m.0,
        SampleParam::RiverMinLengthM => config.river_min_length_m.0,
        SampleParam::MinLakeAreaM2 => config.min_lake_area_m2.0,
        SampleParam::RiverMinDrainageAreaKm2 => config.river_min_drainage_area_km2.0,
        SampleParam::RiverTributaryDrainageAreaKm2 => config.river_tributary_drainage_area_km2.0,
        SampleParam::MountainMinElevationM => config.mountain_min_elevation_m.0,
        SampleParam::MountainMinSlopeDeg => config.mountain_min_slope_deg.0,
        SampleParam::EquatorMeanTempC => config.equator_mean_temp_c.0,
        SampleParam::PoleMeanTempC => config.pole_mean_temp_c.0,
        SampleParam::LapseRateCPerKm => config.lapse_rate_c_per_km,
        SampleParam::RainfallScale => config.rainfall_scale as f64,
        SampleParam::ContinentWavelengthM => config.continent_wavelength_m.0,
        SampleParam::HillWavelengthM => config.hill_wavelength_m.0,
        SampleParam::MountainDetailWavelengthM => config.mountain_detail_wavelength_m.0,
        SampleParam::LandMaskWavelengthM => config.land_mask_wavelength_m.0,
        SampleParam::LandShapeCellSizeM => config.land_shape_cell_size_m.0,
        SampleParam::TemperatureWavelengthM => config.temperature_wavelength_m.0,
        SampleParam::OrogenyMountainThreshold => config.orogeny_mountain_threshold as f64,
        SampleParam::MountainClusterThreshold => config.mountain_cluster_threshold as f64,
        SampleParam::PlateBoundaryStrength => config.plate_boundary_strength as f64,
        SampleParam::ContinentalPlateFraction => config.continental_plate_fraction as f64,
        SampleParam::OceanicUpliftFactor => config.oceanic_uplift_factor as f64,
        SampleParam::CaFillProbability => config.ca_fill_probability as f64,
        SampleParam::CaIterations => config.ca_iterations as f64,
        SampleParam::CaSmoothingPasses => config.ca_smoothing_passes as f64,
        SampleParam::CaCoarseCellSizeM => config.ca_coarse_cell_size_m.0,
        SampleParam::LandMaskBlurM => config.land_mask_blur_m.0,
        SampleParam::LandMaskCloseRadiusM => config.land_mask_close_radius_m.0,
        SampleParam::MinLandmassAreaKm2 => config.min_landmass_area_km2.0,
        SampleParam::MaxLandmassDensityPerKm2 => config.max_landmass_density_per_km2,
        SampleParam::OrogenyPeakRadiusM => config.orogeny_peak_radius_m.0,
        SampleParam::DrunkardPathLengthM => config.drunkard_path_length_m.0,
        SampleParam::MaxLandmassCompactness => config.max_landmass_compactness as f64,
        SampleParam::HybridNoiseBlend => config.hybrid_noise_blend as f64,
        SampleParam::CoastSharpening => config.coast_sharpening as f64,
        SampleParam::CoastCleanupPasses => config.coast_cleanup_passes as f64,
        SampleParam::MountainBoundaryWeight => config.mountain_boundary_weight as f64,
        SampleParam::RiverMeanderStrength => config.river_meander_strength as f64,
        SampleParam::OrogenyInteriorMinDistM => config.orogeny_interior_min_dist_m.0,
        SampleParam::ShelfWidthM => config.shelf_width_m.0,
        SampleParam::ShelfDepthM => config.shelf_depth_m.0,
        SampleParam::PlateLloydIterations => config.plate_lloyd_iterations as f64,
        SampleParam::ContinentalPlateSpeedMax => config.continental_plate_speed_max as f64,
        SampleParam::OceanicPlateSpeedMin => config.oceanic_plate_speed_min as f64,
        SampleParam::MantleFlowAngleDeg => config.mantle_flow_angle_deg,
        SampleParam::OrographicOrogenyWeight => config.orographic_orogeny_weight as f64,
        SampleParam::InteriorDryingFactor => config.interior_drying_factor as f64,
        SampleParam::ContinentalityStrength => config.continentality_strength as f64,
        SampleParam::ContinentalityOceanRangeM => config.continentality_ocean_range_m.0,
        SampleParam::TectonicUpliftScale => config.tectonic_uplift_scale as f64,
        SampleParam::LandTextureStrengthM => config.land_texture_strength_m.0,
        SampleParam::LandTextureCoastBandM => config.land_texture_coast_band_m.0,
        SampleParam::IslandZoneM => config.island_zone_m.0,
        SampleParam::CoarseHydroFactor => config.coarse_hydro_factor as f64,
        SampleParam::LandscapeEvolutionFullResPasses => {
            config.landscape_evolution_full_res_passes as f64
        }
        SampleParam::LandscapeEvolutionIterations => config.landscape_evolution_iterations as f64,
        SampleParam::LandscapeErosionFactor => config.landscape_erosion_factor as f64,
        SampleParam::LandscapeUpliftFactor => config.landscape_uplift_factor as f64,
        SampleParam::ErodibilityPlains => config.erodibility_plains as f64,
        SampleParam::ErodibilityMountains => config.erodibility_mountains as f64,
        SampleParam::RiverIncisionFactor => config.river_incision_factor as f64,
        SampleParam::RainfallErodibilityCoupling => config.rainfall_erodibility_coupling as f64,
    }
}
