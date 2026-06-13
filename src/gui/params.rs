use egui::Ui;
use terraforge::{ElevationEnvelopeConfig, Meters, SquareMeters, WorldGenConfig};

const RESET_BTN: &str = "↺";

pub fn draw_params(ui: &mut Ui, config: &mut WorldGenConfig) {
    let defaults = WorldGenConfig::default();

    ui.horizontal(|ui| {
        if ui.button("Reset all to defaults").clicked() {
            *config = defaults.clone();
        }
    });
    ui.separator();

    let resolved = config.resolve();
    ui.label(format!(
        "Map extent: {:.2} km × {:.2} km",
        resolved.map_width_m / 1000.0,
        resolved.map_height_m / 1000.0
    ));
    ui.label(format!(
        "Resolved sea level norm: {:.3}",
        resolved.sea_level_norm
    ));
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
        });

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

    egui::CollapsingHeader::new("Water").show(ui, |ui| {
        square_meter_field(
            ui,
            "Min lake area (m²)",
            &mut config.min_lake_area_m2,
            defaults.min_lake_area_m2,
        );
    });

    egui::CollapsingHeader::new("Temperature").show(ui, |ui| {
        meter_field(
            ui,
            "Noise wavelength (m)",
            &mut config.temperature_wavelength_m,
            defaults.temperature_wavelength_m,
        );
        f64_field(
            ui,
            "Lapse rate (°C/km)",
            &mut config.lapse_rate_c_per_km,
            defaults.lapse_rate_c_per_km,
            0.1,
            Some(0.0..=20.0),
        );
        f64_field(
            ui,
            "Temperature range (°C)",
            &mut config.temperature_range_c,
            defaults.temperature_range_c,
            1.0,
            Some(10.0..=120.0),
        );
        ui.label("Range scales how strongly elevation cools (not a lat gradient).");
    });

    egui::CollapsingHeader::new("Rainfall").show(ui, |ui| {
        f32_field(
            ui,
            "Rainfall scale",
            &mut config.rainfall_scale,
            defaults.rainfall_scale,
            0.01,
            Some(0.0..=5.0),
        );
        f32_field(
            ui,
            "Coastal rainfall boost",
            &mut config.continentality_strength,
            defaults.continentality_strength,
            0.01,
            Some(0.0..=1.0),
        );
        meter_field(
            ui,
            "Coastal influence range (m)",
            &mut config.continentality_ocean_range_m,
            defaults.continentality_ocean_range_m,
        );
        f32_field(
            ui,
            "Rain shadow weight",
            &mut config.orographic_elevation_weight,
            defaults.orographic_elevation_weight,
            0.01,
            Some(0.0..=2.0),
        );
        f32_field(
            ui,
            "Interior drying factor",
            &mut config.interior_drying_factor,
            defaults.interior_drying_factor,
            0.01,
            Some(0.0..=0.5),
        );
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
            0.1,
            Some(0.5..=45.0),
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
