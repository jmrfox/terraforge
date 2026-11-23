use egui::Ui;
use terraforge::{
    ElevationEnvelopeConfig, Meters, RegionalClimateConfig, SquareMeters, WorldGenConfig,
};

use super::world_controls::draw_world_controls;

const RESET_BTN: &str = "↺";

/// Draw all parameters. Returns true when a high-level control asks to regenerate.
pub fn draw_params(
    ui: &mut Ui,
    config: &mut WorldGenConfig,
    rescale_cell_size_m: &mut f64,
    auto_generate: &mut bool,
) -> bool {
    let defaults = WorldGenConfig::default();

    ui.horizontal_wrapped(|ui| {
        if ui.button("Reset all to defaults").clicked() {
            *config = defaults.clone();
            *rescale_cell_size_m = config.cell_size_m.0;
        }
    });
    ui.separator();

    let resolved = config.resolve();
    ui.label(format!(
        "Map extent: {:.2} km × {:.2} km",
        resolved.map_width_m / 1000.0,
        resolved.map_height_m / 1000.0
    ));
    ui.separator();

    let regenerate = draw_world_controls(ui, config, rescale_cell_size_m, auto_generate);
    ui.separator();

    egui::CollapsingHeader::new("Grid")
        .default_open(true)
        .show(ui, |ui| {
            u64_field(ui, "Seed", &mut config.seed, defaults.seed, 1.0);
            usize_field(
                ui,
                "Width (cells)",
                &mut config.width,
                defaults.width,
                1.0,
                Some(16..=2048),
            );
            usize_field(
                ui,
                "Height (cells)",
                &mut config.height,
                defaults.height,
                1.0,
                Some(16..=2048),
            );
            meter_field(
                ui,
                "Cell size (m)",
                &mut config.cell_size_m,
                defaults.cell_size_m,
            );
            cell_size_rescale_control(ui, config, rescale_cell_size_m);
        });

    egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
        ui.label(format!(
            "Resolved sea level norm: {:.3}",
            resolved.sea_level_norm
        ));
        draw_advanced(ui, config, &defaults);
    });
    regenerate
}

fn draw_advanced(ui: &mut Ui, config: &mut WorldGenConfig, defaults: &WorldGenConfig) {
    egui::CollapsingHeader::new("Vertical datum").show(ui, |ui| {
        meter_field(
            ui,
            "Max elevation (m)",
            &mut config.max_elevation_m,
            defaults.max_elevation_m,
        );
        meter_field(
            ui,
            "Sea level (m)",
            &mut config.sea_level_m,
            defaults.sea_level_m,
        );
        meter_field(
            ui,
            "Ocean floor (m)",
            &mut config.ocean_floor_m,
            defaults.ocean_floor_m,
        );
    });

    egui::CollapsingHeader::new("Elevation noise")
        .default_open(true)
        .show(ui, |ui| {
            meter_field(
                ui,
                "Continent wavelength (m)",
                &mut config.continent_wavelength_m,
                defaults.continent_wavelength_m,
            );
            meter_field(
                ui,
                "Detail wavelength (m)",
                &mut config.detail_wavelength_m,
                defaults.detail_wavelength_m,
            );
            u32_field(
                ui,
                "Elevation octaves",
                &mut config.elevation_octaves,
                defaults.elevation_octaves,
                1.0,
                Some(1..=12),
            );
            f64_field(
                ui,
                "Elevation persistence",
                &mut config.elevation_persistence,
                defaults.elevation_persistence,
                0.01,
                Some(0.1..=0.95),
            );
            f32_field(
                ui,
                "Continent weight",
                &mut config.elevation_continent_weight,
                defaults.elevation_continent_weight,
                0.01,
                Some(0.0..=1.0),
            );
            f32_field(
                ui,
                "Detail weight",
                &mut config.elevation_detail_weight,
                defaults.elevation_detail_weight,
                0.01,
                Some(0.0..=1.0),
            );
            f32_field(
                ui,
                "Ridge weight",
                &mut config.elevation_ridge_weight,
                defaults.elevation_ridge_weight,
                0.01,
                Some(0.0..=1.0),
            );
            optional_land_fraction_field(
                ui,
                &mut config.target_land_fraction,
                defaults.target_land_fraction,
            );
            f32_field(
                ui,
                "Edge ocean bias",
                &mut config.edge_ocean_bias,
                defaults.edge_ocean_bias,
                0.01,
                Some(0.0..=0.5),
            );
            meter_field(
                ui,
                "Edge ocean margin (m)",
                &mut config.edge_ocean_margin_m,
                defaults.edge_ocean_margin_m,
            );
            optional_land_reference_field(ui, config);
        });

    egui::CollapsingHeader::new("Elevation envelopes").show(ui, |ui| {
        ui.label("Ridge envelope (mountain belts)");
        draw_envelope_config(
            ui,
            &mut config.elevation_ridge_envelope,
            &defaults.elevation_ridge_envelope,
        );
        ui.separator();
        ui.label("Detail envelope (ruggedness patches)");
        draw_envelope_config(
            ui,
            &mut config.elevation_detail_envelope,
            &defaults.elevation_detail_envelope,
        );
    });

    egui::CollapsingHeader::new("Rivers and lakes").show(ui, |ui| {
        f64_field(
            ui,
            "River threshold (m³/s)",
            &mut config.hydrology.river_min_discharge_m3s,
            defaults.hydrology.river_min_discharge_m3s,
            0.1,
            Some(0.0..=10_000.0),
        );
        square_meter_field(
            ui,
            "Min lake area (m²)",
            &mut config.min_lake_area_m2,
            defaults.min_lake_area_m2,
        );
        f64_field(
            ui,
            "Min lake depth (m)",
            &mut config.hydrology.min_lake_depth_m,
            defaults.hydrology.min_lake_depth_m,
            1.0,
            Some(0.0..=2_000.0),
        );
        f64_field(
            ui,
            "Outlet incision at 10 m³/s (m)",
            &mut config.hydrology.outlet_incision_m,
            defaults.hydrology.outlet_incision_m,
            5.0,
            Some(0.0..=3_000.0),
        );
        ui.label("Higher incision drains more basins, leaving fewer and smaller lakes.");
    });

    egui::CollapsingHeader::new("Regional climate")
        .default_open(true)
        .show(ui, |ui| {
            f64_field(
                ui,
                "Lapse rate (°C/km)",
                &mut config.lapse_rate_c_per_km,
                defaults.lapse_rate_c_per_km,
                0.1,
                Some(0.0..=20.0),
            );
            meter_field(
                ui,
                "Climate noise wavelength (m)",
                &mut config.temperature_wavelength_m,
                defaults.temperature_wavelength_m,
            );
            f32_field(
                ui,
                "Rainfall scale",
                &mut config.rainfall_scale,
                defaults.rainfall_scale,
                0.01,
                Some(0.0..=5.0),
            );
            draw_regional_climate(ui, &mut config.regional_climate, &defaults.regional_climate);
        });

    egui::CollapsingHeader::new("Biomes").show(ui, |ui| {
        meter_field(
            ui,
            "Mountain min elevation (m)",
            &mut config.mountain_min_elevation_m,
            defaults.mountain_min_elevation_m,
        );
        f64_field(
            ui,
            "Mountain min slope (°)",
            &mut config.mountain_min_slope_deg.0,
            defaults.mountain_min_slope_deg.0,
            0.01,
            Some(0.0..=45.0),
        );
        f32_field(
            ui,
            "Min ridge influence",
            &mut config.mountain_min_ridge_influence,
            defaults.mountain_min_ridge_influence,
            0.01,
            Some(0.0..=1.0),
        );
    });
}

fn draw_regional_climate(
    ui: &mut Ui,
    climate: &mut RegionalClimateConfig,
    defaults: &RegionalClimateConfig,
) {
    f64_field(
        ui,
        "Latitude at map center (°)",
        &mut climate.latitude_deg,
        defaults.latitude_deg,
        0.5,
        Some(-90.0..=90.0),
    );
    f64_field(
        ui,
        "Wind from (compass °)",
        &mut climate.wind_from_deg,
        defaults.wind_from_deg,
        1.0,
        Some(0.0..=360.0),
    );
    f64_field(
        ui,
        "Temperature offset (°C)",
        &mut climate.temperature_offset_c,
        defaults.temperature_offset_c,
        0.1,
        Some(-30.0..=30.0),
    );
    f64_field(
        ui,
        "Equator temperature (°C)",
        &mut climate.equator_temperature_c,
        defaults.equator_temperature_c,
        0.5,
        Some(-20.0..=50.0),
    );
    f64_field(
        ui,
        "Pole temperature (°C)",
        &mut climate.pole_temperature_c,
        defaults.pole_temperature_c,
        0.5,
        Some(-60.0..=30.0),
    );
    f64_field(
        ui,
        "Local temperature variation (°C)",
        &mut climate.temperature_variation_c,
        defaults.temperature_variation_c,
        0.1,
        Some(0.0..=20.0),
    );
    f64_field(
        ui,
        "Base rainfall (mm/yr)",
        &mut climate.base_rainfall_mm,
        defaults.base_rainfall_mm,
        10.0,
        Some(0.0..=6000.0),
    );
    f64_field(
        ui,
        "Local rainfall variation",
        &mut climate.rainfall_variation,
        defaults.rainfall_variation,
        0.01,
        Some(0.0..=0.95),
    );
    f64_field(
        ui,
        "Subtropical drying",
        &mut climate.subtropical_drying,
        defaults.subtropical_drying,
        0.01,
        Some(0.0..=1.0),
    );
    f64_field(
        ui,
        "Orographic efficiency",
        &mut climate.orographic_efficiency,
        defaults.orographic_efficiency,
        0.01,
        Some(0.0..=1.0),
    );
    meter_field(
        ui,
        "Rainout length (m)",
        &mut climate.rainout_length_m,
        defaults.rainout_length_m,
    );
    meter_field(
        ui,
        "Airflow terrain smoothing (m)",
        &mut climate.orographic_smoothing_m,
        defaults.orographic_smoothing_m,
    );
}

fn optional_land_reference_field(ui: &mut Ui, config: &mut WorldGenConfig) {
    ui.label("Fixed land reference (m)");
    ui.horizontal_wrapped(|ui| {
        let mut enabled = config.land_reference_extent_m.is_some();
        if ui
            .checkbox(&mut enabled, "Enable")
            .on_hover_text("Keep coastlines fixed when the map's width or height changes")
            .changed()
        {
            config.land_reference_extent_m = enabled
                .then(|| Meters(config.width.max(config.height) as f64 * config.cell_size_m.0));
        }
        if let Some(extent) = config.land_reference_extent_m.as_mut() {
            ui.add(
                egui::DragValue::new(&mut extent.0)
                    .speed(100.0)
                    .range(1.0..=f64::MAX),
            );
        }
    });
    ui.add_space(4.0);
}

/// Change the cell size and scale every length-related parameter to keep the same map.
fn cell_size_rescale_control(ui: &mut Ui, config: &mut WorldGenConfig, target_m: &mut f64) {
    ui.label("Rescale to cell size (m)");
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::DragValue::new(target_m)
                .speed(1.0)
                .range(0.01..=1_000_000.0),
        );
        let current = config.cell_size_m.0;
        let can_apply = current.is_finite() && current > 0.0 && *target_m != current;
        if ui
            .add_enabled(can_apply, egui::Button::new("Apply"))
            .on_hover_text(
                "Set the cell size and scale wavelengths, ranges, lake area, and the mountain slope threshold to keep the terrain. Climate then responds to the new real size",
            )
            .clicked()
        {
            *config = config.rescaled_to_cell_size(Meters(*target_m));
        }
        if can_apply {
            ui.weak(format!("×{:.3}", *target_m / current));
        }
    });
    ui.add_space(4.0);
}

fn draw_envelope_config(
    ui: &mut Ui,
    value: &mut ElevationEnvelopeConfig,
    default: &ElevationEnvelopeConfig,
) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut value.enabled, "Enabled");
        if reset_button(ui) {
            *value = default.clone();
        }
    });
    meter_field(
        ui,
        "Envelope wavelength (m)",
        &mut value.wavelength_m,
        default.wavelength_m,
    );
    u32_field(
        ui,
        "Envelope octaves",
        &mut value.octaves,
        default.octaves,
        1.0,
        Some(1..=8),
    );
    f32_field(
        ui,
        "Envelope floor",
        &mut value.floor,
        default.floor,
        0.01,
        Some(0.0..=1.0),
    );
    f32_field(
        ui,
        "Envelope strength",
        &mut value.strength,
        default.strength,
        0.01,
        Some(0.0..=2.0),
    );
}

fn optional_land_fraction_field(ui: &mut Ui, value: &mut Option<f32>, default: Option<f32>) {
    ui.label("Target land fraction (None = off)");
    ui.horizontal(|ui| {
        let mut enabled = value.is_some();
        if ui.checkbox(&mut enabled, "Enable").changed() {
            *value = if enabled {
                default.or(Some(0.35))
            } else {
                None
            };
        }
        if let Some(v) = value.as_mut() {
            ui.add(egui::DragValue::new(v).speed(0.01).range(0.05..=0.95));
        }
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn reset_button(ui: &mut Ui) -> bool {
    ui.small_button(RESET_BTN).clicked()
}

fn meter_field(ui: &mut Ui, label: &str, value: &mut Meters, default: Meters) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut value.0).speed(1.0));
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn square_meter_field(ui: &mut Ui, label: &str, value: &mut SquareMeters, default: SquareMeters) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut value.0).speed(10.0));
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn f32_field(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    default: f32,
    speed: f64,
    range: Option<std::ops::RangeInclusive<f32>>,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut drag = egui::DragValue::new(value).speed(speed);
        if let Some(r) = range {
            drag = drag.range(r);
        }
        ui.add(drag);
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn f64_field(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    default: f64,
    speed: f64,
    range: Option<std::ops::RangeInclusive<f64>>,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut drag = egui::DragValue::new(value).speed(speed);
        if let Some(r) = range {
            drag = drag.range(r);
        }
        ui.add(drag);
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn u32_field(
    ui: &mut Ui,
    label: &str,
    value: &mut u32,
    default: u32,
    speed: f64,
    range: Option<std::ops::RangeInclusive<u32>>,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut drag = egui::DragValue::new(value).speed(speed);
        if let Some(r) = range {
            drag = drag.range(r);
        }
        ui.add(drag);
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn u64_field(ui: &mut Ui, label: &str, value: &mut u64, default: u64, speed: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(value).speed(speed));
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn usize_field(
    ui: &mut Ui,
    label: &str,
    value: &mut usize,
    default: usize,
    speed: f64,
    range: Option<std::ops::RangeInclusive<usize>>,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut drag = egui::DragValue::new(value).speed(speed);
        if let Some(r) = range {
            drag = drag.range(r);
        }
        ui.add(drag);
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}
