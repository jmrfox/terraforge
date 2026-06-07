use serde::{Deserialize, Serialize};

use crate::units::{
    Celsius, Degrees, Meters, SquareKilometers, SquareMeters, wavelength_to_frequency,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum WindDirection {
    WestToEast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum LandMaskMethod {
    Noise,
    CellularAutomata,
    DrunkardsWalk,
    #[default]
    Hybrid,
}

/// How bulk land geography is assembled before optional texture overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum LandGenerationMode {
    /// Crust + plate-boundary uplift define continents; texture adds coast detail.
    #[default]
    TectonicBase,
    /// Legacy mask-primary elevation blend (regression / comparison).
    LegacyMask,
}

/// User-facing configuration in physical units where applicable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldGenConfig {
    pub seed: u64,
    pub width: usize,
    pub height: usize,

    /// Tectonic plates per square kilometer of map area.
    pub plate_density_per_km2: f64,
    /// Drunkard-walk agents per square kilometer (land-mask method).
    pub drunkard_walker_density_per_km2: f64,

    // --- Grid and vertical datum ---
    pub cell_size_m: Meters,
    pub max_elevation_m: Meters,
    pub sea_level_m: Meters,
    pub ocean_floor_m: Meters,

    // --- Horizontal distances ---
    pub continental_margin_m: Meters,
    pub min_isthmus_width_m: Meters,
    pub mountain_belt_width_m: Meters,
    pub mountain_coast_buffer_m: Meters,
    pub coast_cleanup_proximity_m: Meters,
    pub drunkard_brush_radius_m: Meters,
    pub river_min_length_m: Meters,

    // --- Areas ---
    pub min_lake_area_m2: SquareMeters,
    pub river_min_drainage_area_km2: SquareKilometers,
    pub river_tributary_drainage_area_km2: SquareKilometers,

    // --- Elevation / slope thresholds ---
    pub mountain_min_elevation_m: Meters,
    pub mountain_min_slope_deg: Degrees,

    // --- Climate ---
    pub equator_mean_temp_c: Celsius,
    pub pole_mean_temp_c: Celsius,
    pub lapse_rate_c_per_km: f64,
    pub rainfall_scale: f32,
    pub wind_direction: WindDirection,

    // --- Noise wavelengths ---
    pub continent_wavelength_m: Meters,
    pub hill_wavelength_m: Meters,
    pub mountain_detail_wavelength_m: Meters,
    pub land_mask_wavelength_m: Meters,
    /// Physical spacing for land-shape generators (CA/drunkard/hybrid detail).
    pub land_shape_cell_size_m: Meters,
    /// Wavelength for macro temperature noise (replaces latitude gradient).
    pub temperature_wavelength_m: Meters,

    // --- Dimensionless / structural ---
    pub orogeny_mountain_threshold: f32,
    pub mountain_cluster_threshold: f32,
    pub use_orogeny_mountains: bool,
    pub plate_boundary_strength: f32,
    pub continental_plate_fraction: f32,
    pub oceanic_uplift_factor: f32,
    pub land_mask_method: LandMaskMethod,
    pub ca_fill_probability: f32,
    pub ca_iterations: u32,
    pub ca_smoothing_passes: u32,
    /// Target CA simulation cell size in meters (coarse pass upsampled to grid).
    pub ca_coarse_cell_size_m: Meters,
    /// Override drunkard steps per walker (0 = auto from map extent).
    pub drunkard_steps: u32,
    pub land_mask_blur_m: Meters,
    /// Morphological closing radius after isthmus breakup (fills narrow gaps).
    pub land_mask_close_radius_m: Meters,
    pub min_landmass_area_km2: SquareKilometers,
    /// Cap on distinct land components kept after mask cleanup.
    pub max_landmass_density_per_km2: f64,
    pub orogeny_peak_radius_m: Meters,
    /// Auto drunkard steps per walker when `drunkard_steps` is 0 (path length in meters).
    pub drunkard_path_length_m: Meters,

    /// Deprecated preset field — use `plate_density_per_km2` instead.
    #[serde(default, skip_serializing, rename = "plate_count")]
    pub legacy_plate_count: Option<u32>,
    /// Deprecated preset field — use `drunkard_walker_density_per_km2` instead.
    #[serde(default, skip_serializing, rename = "drunkard_walkers")]
    pub legacy_drunkard_walkers: Option<u32>,
    /// Deprecated preset field — use `ca_coarse_cell_size_m` instead.
    #[serde(default, skip_serializing, rename = "ca_coarse_factor")]
    pub legacy_ca_coarse_factor: Option<u32>,
    pub hybrid_noise_blend: f32,
    pub use_plate_macro_mask: bool,
    pub max_landmass_compactness: f32,
    pub coast_sharpening: f32,
    pub coast_cleanup_passes: u32,
    pub mountain_boundary_weight: f32,
    pub river_meander_strength: f32,

    // --- Elevation realism (Phase 3) ---
    pub orogeny_interior_min_dist_m: Meters,
    pub mountain_noise_orogeny_only: bool,

    // --- Oceans / coast (Phase 4) ---
    pub target_land_fraction: Option<f32>,
    pub shelf_width_m: Meters,
    pub shelf_depth_m: Meters,

    // --- Plates (Phase 1 Tier B) ---
    pub plate_lloyd_iterations: u32,
    pub continental_plate_speed_max: f32,
    pub oceanic_plate_speed_min: f32,
    pub mantle_flow_angle_deg: f64,

    // --- Climate realism (Phase 6) ---
    pub orographic_orogeny_weight: f32,
    pub interior_drying_factor: f32,
    pub continentality_strength: f32,
    pub continentality_ocean_range_m: Meters,

    // --- Process-driven geography ---
    pub land_generation: LandGenerationMode,
    pub tectonic_uplift_scale: f32,
    pub land_texture_strength_m: Meters,
    pub land_texture_coast_band_m: Meters,
    pub island_zone_m: Meters,
    pub landscape_evolution_enabled: bool,
    /// Downsample factor for coarse-grid hydro (LEM flow/erosion). 1 = full resolution.
    pub coarse_hydro_factor: u32,
    /// Optional full-res LEM passes after coarse hydro (0 = skip).
    pub landscape_evolution_full_res_passes: u32,
    pub landscape_evolution_iterations: u32,
    pub landscape_erosion_factor: f32,
    pub landscape_uplift_factor: f32,
    pub erodibility_plains: f32,
    pub erodibility_mountains: f32,
    pub river_incision_enabled: bool,
    pub river_incision_factor: f32,
    pub rainfall_erodibility_coupling: f32,
    pub legacy_coast_cleanup: bool,
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
    pub equator_mean_temp_c: f64,
    pub pole_mean_temp_c: f64,

    pub sea_level_norm: f32,
    pub ocean_floor_norm: f32,
    pub mountain_elev_norm: f32,
    pub mountain_slope_norm: f32,

    pub continental_blur_radius_cells: u32,
    pub min_isthmus_width_cells: u32,
    pub mountain_spread_radius_cells: u32,
    pub mountain_coast_buffer_cells: u32,
    pub coast_proximity_cells: usize,
    pub drunkard_brush_radius_cells: u32,
    pub river_min_length_cells: u32,
    pub min_lake_cells: usize,

    pub river_flow_threshold_cells: f32,
    pub river_tributary_threshold_cells: f32,

    pub temperature_scale: f32,
    pub elevation_cooling_factor: f32,

    pub continent_noise_frequency: f64,
    pub hill_noise_frequency: f64,
    pub mountain_noise_frequency: f64,
    pub land_mask_noise_frequency: f64,

    pub orogeny_interior_min_dist_cells: u32,
    pub shelf_width_cells: u32,
    pub shelf_depth_norm: f32,
    pub continental_base_norm: f32,
    pub abyssal_base_norm: f32,
    pub continentality_ocean_range_cells: u32,

    pub land_texture_strength_norm: f32,
    pub land_texture_coast_band_cells: u32,
    pub island_zone_cells: u32,
    pub max_elev_norm: f32,

    pub plate_count: u32,
    pub drunkard_walkers: u32,
    pub drunkard_steps_per_walker: u32,
    pub land_mask_blur_cells: u32,
    pub land_mask_close_cells: u32,
    pub min_landmass_cells: usize,
    pub max_landmasses: usize,
    pub orogeny_peak_radius_cells: i32,
    pub ca_coarse_factor: u32,
    pub land_shape_factor: u32,
    pub temperature_noise_frequency: f64,
}

const PLATE_COUNT_MIN: u32 = 8;
const DRUNKARD_WALKER_MIN: u32 = 1;
const DRUNKARD_WALKER_MAX: u32 = 500;
const DRUNKARD_STEPS_MIN: u32 = 800;
const DRUNKARD_STEPS_MAX: u32 = 20_000;

impl WorldGenConfig {
    pub fn map_area_km2(&self) -> f64 {
        let cell = self.cell_size_m.0;
        (self.width as f64 * self.height as f64 * cell * cell) / 1_000_000.0
    }

    fn effective_plate_density(&self) -> f64 {
        if let Some(count) = self.legacy_plate_count {
            count as f64 / self.map_area_km2().max(1e-9)
        } else {
            self.plate_density_per_km2
        }
    }

    fn effective_drunkard_walker_density(&self) -> f64 {
        if let Some(count) = self.legacy_drunkard_walkers {
            count as f64 / self.map_area_km2().max(1e-9)
        } else {
            self.drunkard_walker_density_per_km2
        }
    }

    fn effective_ca_coarse_cell_size_m(&self) -> f64 {
        if let Some(factor) = self.legacy_ca_coarse_factor {
            self.cell_size_m.0 * factor as f64
        } else {
            self.ca_coarse_cell_size_m.0
        }
    }

    pub fn resolve(&self) -> ResolvedSimParams {
        let cell = self.cell_size_m.0;
        let map_width_m = self.width as f64 * cell;
        let map_height_m = self.height as f64 * cell;
        let map_area_km2 = self.map_area_km2();
        let max_elev = self.max_elevation_m.0;
        let ocean_floor = self.ocean_floor_m.0;
        let elev_span = (max_elev - ocean_floor).max(1.0);

        let sea_level_norm =
            (((self.sea_level_m.0 - ocean_floor) / elev_span) as f32).clamp(0.0, 1.0);
        let ocean_floor_norm = 0.0f32;
        let mountain_elev_norm = (((self.mountain_min_elevation_m.0 - ocean_floor) / elev_span)
            as f32)
            .clamp(0.0, 1.0);
        let mountain_slope_norm = self.mountain_min_slope_deg.to_radians().tan() as f32;

        let temp_range_c = (self.equator_mean_temp_c.0 - self.pole_mean_temp_c.0).max(1.0);
        let elevation_cooling_factor = ((self.lapse_rate_c_per_km / 1000.0 * max_elev)
            / temp_range_c) as f32;

        let plate_count_max = ((self.width * self.height) as f64 / 800.0)
            .round()
            .clamp(PLATE_COUNT_MIN as f64, 512.0) as u32;
        let plate_count = (map_area_km2 * self.effective_plate_density())
            .round()
            .clamp(PLATE_COUNT_MIN as f64, plate_count_max as f64) as u32;

        let drunkard_walkers = (map_area_km2 * self.effective_drunkard_walker_density())
            .round()
            .clamp(DRUNKARD_WALKER_MIN as f64, DRUNKARD_WALKER_MAX as f64) as u32;

        let drunkard_steps_per_walker = if self.drunkard_steps > 0 {
            self.drunkard_steps
        } else {
            (self.drunkard_path_length_m.0 / cell)
                .round()
                .clamp(DRUNKARD_STEPS_MIN as f64, DRUNKARD_STEPS_MAX as f64) as u32
        };

        let ca_coarse_factor = (self.effective_ca_coarse_cell_size_m() / cell)
            .round()
            .clamp(1.0, 8.0) as u32;
        let min_dim = self.width.min(self.height);
        let ca_coarse_factor = ca_coarse_factor.min((min_dim / 32).max(1) as u32);

        let land_shape_factor = (cell / self.land_shape_cell_size_m.0)
            .round()
            .clamp(1.0, 32.0) as u32;

        let min_landmass_cells = if self.min_landmass_area_km2.0 <= 0.0 || map_area_km2 < 4.0 {
            0
        } else {
            self.min_landmass_area_km2.to_cell_count(cell).max(1)
        };
        let max_landmasses = if map_area_km2 < 1.0 {
            usize::MAX
        } else {
            (map_area_km2 * self.max_landmass_density_per_km2)
                .round()
                .clamp(1.0, 64.0) as usize
        };

        ResolvedSimParams {
            cell_size_m: cell,
            map_width_m,
            map_height_m,
            max_elevation_m: max_elev,
            sea_level_m: self.sea_level_m.0,
            ocean_floor_m: ocean_floor,
            equator_mean_temp_c: self.equator_mean_temp_c.0,
            pole_mean_temp_c: self.pole_mean_temp_c.0,

            sea_level_norm,
            ocean_floor_norm,
            mountain_elev_norm,
            mountain_slope_norm,

            continental_blur_radius_cells: self.continental_margin_m.to_cells(cell).max(1),
            min_isthmus_width_cells: self.min_isthmus_width_m.to_cells(cell).max(1),
            mountain_spread_radius_cells: self.mountain_belt_width_m.to_cells(cell).max(1),
            mountain_coast_buffer_cells: self.mountain_coast_buffer_m.to_cells(cell).max(1),
            coast_proximity_cells: self.coast_cleanup_proximity_m.to_cells_usize(cell).max(1),
            drunkard_brush_radius_cells: self.drunkard_brush_radius_m.to_cells(cell).max(1),
            river_min_length_cells: self.river_min_length_m.to_cells(cell).max(1),
            min_lake_cells: self.min_lake_area_m2.to_cell_count(cell).max(1),

            river_flow_threshold_cells: self
                .river_min_drainage_area_km2
                .to_cell_count(cell) as f32,
            river_tributary_threshold_cells: self
                .river_tributary_drainage_area_km2
                .to_cell_count(cell) as f32,

            temperature_scale: 1.0,
            elevation_cooling_factor,

            continent_noise_frequency: wavelength_to_frequency(
                map_width_m,
                self.continent_wavelength_m,
            ),
            hill_noise_frequency: wavelength_to_frequency(map_width_m, self.hill_wavelength_m),
            mountain_noise_frequency: wavelength_to_frequency(
                map_width_m,
                self.mountain_detail_wavelength_m,
            ),
            land_mask_noise_frequency: wavelength_to_frequency(
                map_width_m,
                self.land_mask_wavelength_m,
            ),
            temperature_noise_frequency: wavelength_to_frequency(
                map_width_m,
                self.temperature_wavelength_m,
            ),

            orogeny_interior_min_dist_cells: self.orogeny_interior_min_dist_m.to_cells(cell).max(1),
            shelf_width_cells: self.shelf_width_m.to_cells(cell).max(1),
            shelf_depth_norm: (self.shelf_depth_m.0 / elev_span) as f32,
            continental_base_norm: (sea_level_norm + 0.06).min(0.92),
            abyssal_base_norm: (sea_level_norm * 0.35).max(0.04),
            continentality_ocean_range_cells: self
                .continentality_ocean_range_m
                .to_cells(cell)
                .max(1),

            land_texture_strength_norm: (self.land_texture_strength_m.0 / elev_span) as f32,
            land_texture_coast_band_cells: self.land_texture_coast_band_m.to_cells(cell).max(1),
            island_zone_cells: self.island_zone_m.to_cells(cell).max(1),
            max_elev_norm: 1.0,

            plate_count,
            drunkard_walkers,
            drunkard_steps_per_walker,
            land_mask_blur_cells: self.land_mask_blur_m.to_cells(cell).max(1),
            land_mask_close_cells: self.land_mask_close_radius_m.to_cells(cell).max(1),
            min_landmass_cells,
            max_landmasses,
            orogeny_peak_radius_cells: self.orogeny_peak_radius_m.to_cells(cell).max(1) as i32,
            ca_coarse_factor,
            land_shape_factor,
        }
    }

    /// Suggest a physical sea level (meters) that yields approximately `target_land_fraction`
    /// land on a preview elevation field. Editor/workflow utility — not used during generation.
    pub fn suggest_sea_level_m_for_fraction(
        &self,
        elevation: &[f32],
        target_land_fraction: f32,
    ) -> Meters {
        let params = self.resolve();
        let elev_span = (self.max_elevation_m.0 - self.ocean_floor_m.0).max(1.0);
        let norm = calibrate_sea_level_norm(elevation, target_land_fraction);
        let meters = self.ocean_floor_m.0 + norm as f64 * elev_span;
        let _ = params.sea_level_norm;
        Meters(meters.clamp(self.ocean_floor_m.0, self.max_elevation_m.0))
    }

    pub fn test_config(seed: u64, size: usize) -> Self {
        Self {
            seed,
            width: size,
            height: size,
            plate_density_per_km2: 0.5,
            river_min_drainage_area_km2: SquareKilometers(0.008),
            river_tributary_drainage_area_km2: SquareKilometers(0.003),
            river_min_length_m: Meters(80.0),
            river_meander_strength: 0.0,
            ..Default::default()
        }
    }

    pub fn map_preview() -> Self {
        Self {
            width: 128,
            height: 128,
            plate_density_per_km2: 0.35,
            river_min_drainage_area_km2: SquareKilometers(0.004),
            river_min_length_m: Meters(80.0),
            ..Default::default()
        }
    }

    pub fn effective_coast_sharpening(&self) -> f32 {
        if self.land_generation == LandGenerationMode::TectonicBase {
            0.0
        } else {
            self.coast_sharpening
        }
    }

}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            width: 512,
            height: 512,
            plate_density_per_km2: 0.23,
            drunkard_walker_density_per_km2: 0.21,

            cell_size_m: Meters(20.0),
            max_elevation_m: Meters(9000.0),
            sea_level_m: Meters(0.0),
            ocean_floor_m: Meters(-6000.0),

            continental_margin_m: Meters(200.0),
            min_isthmus_width_m: Meters(120.0),
            mountain_belt_width_m: Meters(60.0),
            mountain_coast_buffer_m: Meters(120.0),
            coast_cleanup_proximity_m: Meters(80.0),
            drunkard_brush_radius_m: Meters(200.0),
            river_min_length_m: Meters(100.0),

            min_lake_area_m2: SquareMeters(9600.0),
            river_min_drainage_area_km2: SquareKilometers(0.01536),
            river_tributary_drainage_area_km2: SquareKilometers(0.00486),

            mountain_min_elevation_m: Meters(4635.0),
            mountain_min_slope_deg: Degrees(4.57),

            equator_mean_temp_c: Celsius(30.0),
            pole_mean_temp_c: Celsius(-30.0),
            lapse_rate_c_per_km: 6.5,
            rainfall_scale: 1.0,
            wind_direction: WindDirection::WestToEast,

            continent_wavelength_m: Meters(5120.0),
            hill_wavelength_m: Meters(1706.6666666666665),
            mountain_detail_wavelength_m: Meters(1024.0),
            land_mask_wavelength_m: Meters(20480.0),
            land_shape_cell_size_m: Meters(50.0),
            temperature_wavelength_m: Meters(12_000.0),

            orogeny_mountain_threshold: 0.18,
            mountain_cluster_threshold: 0.55,
            use_orogeny_mountains: true,
            plate_boundary_strength: 0.18,
            continental_plate_fraction: 0.35,
            oceanic_uplift_factor: 0.1,
            land_mask_method: LandMaskMethod::Hybrid,
            ca_fill_probability: 0.52,
            ca_iterations: 7,
            ca_smoothing_passes: 4,
            ca_coarse_cell_size_m: Meters(80.0),
            drunkard_steps: 0,
            land_mask_blur_m: Meters(80.0),
            land_mask_close_radius_m: Meters(20.0),
            min_landmass_area_km2: SquareKilometers(0.14),
            max_landmass_density_per_km2: 0.057,
            orogeny_peak_radius_m: Meters(60.0),
            drunkard_path_length_m: Meters(119_160.0),
            legacy_plate_count: None,
            legacy_drunkard_walkers: None,
            legacy_ca_coarse_factor: None,
            hybrid_noise_blend: 0.30,
            use_plate_macro_mask: true,
            max_landmass_compactness: 72.0,
            coast_sharpening: 0.15,
            coast_cleanup_passes: 2,
            mountain_boundary_weight: 0.62,
            river_meander_strength: 0.12,

            orogeny_interior_min_dist_m: Meters(120.0),
            mountain_noise_orogeny_only: true,

            target_land_fraction: None,
            shelf_width_m: Meters(80.0),
            shelf_depth_m: Meters(200.0),

            plate_lloyd_iterations: 2,
            continental_plate_speed_max: 0.4,
            oceanic_plate_speed_min: 0.4,
            mantle_flow_angle_deg: 0.0,

            orographic_orogeny_weight: 0.65,
            interior_drying_factor: 0.08,
            continentality_strength: 0.12,
            continentality_ocean_range_m: Meters(8000.0),

            land_generation: LandGenerationMode::TectonicBase,
            tectonic_uplift_scale: 1.0,
            land_texture_strength_m: Meters(400.0),
            land_texture_coast_band_m: Meters(2000.0),
            island_zone_m: Meters(5000.0),
            landscape_evolution_enabled: true,
            coarse_hydro_factor: 4,
            landscape_evolution_full_res_passes: 0,
            landscape_evolution_iterations: 12,
            landscape_erosion_factor: 0.002,
            landscape_uplift_factor: 0.006,
            erodibility_plains: 4.0,
            erodibility_mountains: 1.2,
            river_incision_enabled: true,
            river_incision_factor: 0.003,
            rainfall_erodibility_coupling: 0.15,
            legacy_coast_cleanup: false,
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
    fn resolve_default_matches_legacy_cell_counts() {
        let config = WorldGenConfig::default();
        let p = config.resolve();
        assert!((p.cell_size_m - 20.0).abs() < f64::EPSILON);
        assert_eq!(p.continental_blur_radius_cells, 10);
        assert_eq!(p.min_isthmus_width_cells, 6);
        assert_eq!(p.mountain_spread_radius_cells, 3);
        assert_eq!(p.mountain_coast_buffer_cells, 6);
        assert_eq!(p.coast_proximity_cells, 4);
        assert_eq!(p.min_lake_cells, 24);
        assert_eq!(p.river_min_length_cells, 5);
        assert!((p.river_flow_threshold_cells - 38.0).abs() < 0.5);
        assert!((p.river_tributary_threshold_cells - 12.0).abs() < 0.5);
    }

    #[test]
    fn resolve_noise_frequencies_calibrated() {
        let p = WorldGenConfig::default().resolve();
        assert!((p.continent_noise_frequency - 2.0).abs() < 0.01);
        assert!((p.hill_noise_frequency - 6.0).abs() < 0.01);
        assert!((p.mountain_noise_frequency - 10.0).abs() < 0.01);
        assert!((p.land_mask_noise_frequency - 0.5).abs() < 0.01);
        assert!(p.temperature_noise_frequency > 0.0);
    }

    #[test]
    fn land_shape_factor_scales_with_cell_size() {
        let base = WorldGenConfig::default().resolve();
        let mut config = WorldGenConfig::default();
        config.cell_size_m = Meters(100.0);
        let coarse = config.resolve();
        assert_eq!(base.land_shape_factor, 1);
        assert_eq!(coarse.land_shape_factor, 2);
    }

    #[test]
    fn calibrate_sea_level_norm_hits_target() {
        let elev: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let sea = calibrate_sea_level_norm(&elev, 0.30);
        let land = elev.iter().filter(|&&e| e >= sea).count() as f32 / 100.0;
        assert!((land - 0.30).abs() < 0.02);
    }

    #[test]
    fn resolve_sea_level_norm_from_datum() {
        let config = WorldGenConfig::default();
        let p = config.resolve();
        // (0 - (-6000)) / (9000 - (-6000)) = 0.4
        assert!((p.sea_level_norm - 0.4).abs() < 0.01);
        // (4635 - (-6000)) / (9000 - (-6000)) ≈ 0.709 on ocean-floor→max datum
        assert!((p.mountain_elev_norm - 0.709).abs() < 0.02);
        assert!((p.mountain_slope_norm - 0.08).abs() < 0.01);
    }

    #[test]
    fn resolve_default_plate_density_yields_legacy_count() {
        let p = WorldGenConfig::default().resolve();
        assert_eq!(p.plate_count, 24);
        assert_eq!(p.land_mask_blur_cells, 4);
        assert_eq!(p.land_mask_close_cells, 1);
        assert_eq!(p.ca_coarse_factor, 4);
        assert_eq!(p.drunkard_walkers, 22);
        assert_eq!(p.drunkard_steps_per_walker, 5958);
        assert_eq!(p.max_landmasses, 6);
        assert_eq!(p.orogeny_peak_radius_cells, 3);
    }

    #[test]
    fn doubling_cell_size_halves_blur_cells_and_scales_plate_count_with_area() {
        let mut config = WorldGenConfig::default();
        let base = config.resolve();
        config.cell_size_m = Meters(40.0);
        let coarse = config.resolve();

        assert!((coarse.map_width_m - base.map_width_m * 2.0).abs() < 1.0);
        assert_eq!(coarse.continental_blur_radius_cells, base.continental_blur_radius_cells / 2);
        assert_eq!(coarse.land_mask_blur_cells, base.land_mask_blur_cells / 2);
        assert_eq!(coarse.orogeny_peak_radius_cells, 2);
        assert!(coarse.plate_count > base.plate_count * 3);
        assert!(coarse.max_landmasses > base.max_landmasses);
    }

    #[test]
    fn drunkard_path_length_scales_with_cell_size() {
        let base = WorldGenConfig::default().resolve();
        let mut config = WorldGenConfig::default();
        config.cell_size_m = Meters(40.0);
        let coarse = config.resolve();
        let base_path_m = base.drunkard_steps_per_walker as f64 * base.cell_size_m;
        let coarse_path_m = coarse.drunkard_steps_per_walker as f64 * coarse.cell_size_m;
        assert!((base_path_m - coarse_path_m).abs() < 1.0);
        assert!(coarse.drunkard_walkers > base.drunkard_walkers);
        assert!(
            (coarse.drunkard_walkers as f64 * coarse_path_m)
                > (base.drunkard_walkers as f64 * base_path_m) * 3.0
        );
    }

    #[test]
    fn legacy_plate_count_converts_to_density() {
        let mut config = WorldGenConfig::default();
        config.plate_density_per_km2 = 0.23;
        config.legacy_plate_count = Some(48);
        let p = config.resolve();
        assert_eq!(p.plate_count, 48);
    }
}
