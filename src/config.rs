use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldGenConfig {
    pub seed: u64,
    pub width: usize,
    pub height: usize,

    pub plate_count: u32,

    pub sea_level: f32,
    pub mountain_elevation_threshold: f32,
    pub mountain_slope_threshold: f32,
    pub river_flow_threshold: f32,
    pub river_min_length: u32,

    pub temperature_scale: f32,
    pub elevation_cooling_factor: f32,
    pub rainfall_scale: f32,
    pub wind_direction: WindDirection,

    pub continent_noise_frequency: f64,
    pub mountain_noise_frequency: f64,
    pub hill_noise_frequency: f64,

    pub plate_boundary_strength: f32,

    pub land_mask_method: LandMaskMethod,
    pub ca_fill_probability: f32,
    pub ca_iterations: u32,
    pub ca_smoothing_passes: u32,
    pub drunkard_walkers: u32,
    pub drunkard_steps: u32,
    pub drunkard_brush_radius: u32,
    pub hybrid_noise_blend: f32,

    pub coast_sharpening: f32,
    pub coast_cleanup_passes: u32,

    pub mountain_spread_radius: u32,
    pub mountain_boundary_weight: f32,

    pub river_meander_strength: f32,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            width: 512,
            height: 512,
            plate_count: 24,
            sea_level: 0.38,
            mountain_elevation_threshold: 0.68,
            mountain_slope_threshold: 0.08,
            river_flow_threshold: 55.0,
            river_min_length: 8,
            temperature_scale: 1.0,
            elevation_cooling_factor: 0.35,
            rainfall_scale: 1.0,
            wind_direction: WindDirection::WestToEast,
            continent_noise_frequency: 0.9,
            mountain_noise_frequency: 4.0,
            hill_noise_frequency: 7.0,
            plate_boundary_strength: 0.12,
            land_mask_method: LandMaskMethod::Hybrid,
            ca_fill_probability: 0.50,
            ca_iterations: 5,
            ca_smoothing_passes: 2,
            drunkard_walkers: 8,
            drunkard_steps: 0,
            drunkard_brush_radius: 4,
            hybrid_noise_blend: 0.25,
            coast_sharpening: 0.7,
            coast_cleanup_passes: 2,
            mountain_spread_radius: 3,
            mountain_boundary_weight: 0.4,
            river_meander_strength: 0.12,
        }
    }
}

impl WorldGenConfig {
    pub fn test_config(seed: u64, size: usize) -> Self {
        Self {
            seed,
            width: size,
            height: size,
            plate_count: 8,
            river_flow_threshold: 20.0,
            river_min_length: 4,
            river_meander_strength: 0.0,
            ..Default::default()
        }
    }

    pub fn map_preview() -> Self {
        Self {
            width: 128,
            height: 128,
            plate_count: 12,
            river_flow_threshold: 10.0,
            river_min_length: 4,
            ..Default::default()
        }
    }

    pub fn drunkard_steps_for_map(&self) -> u32 {
        if self.drunkard_steps > 0 {
            return self.drunkard_steps;
        }
        let area = (self.width * self.height) as u32;
        (area / self.drunkard_walkers.max(1) / 4).clamp(500, 8000)
    }
}
