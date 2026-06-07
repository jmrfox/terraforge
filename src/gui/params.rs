use egui::Ui;
use terraforge::{
    Celsius, Degrees, LandGenerationMode, LandMaskMethod, Meters, SquareKilometers, SquareMeters,
    WindDirection, WorldGenConfig,
};

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
    ui.label(format!(
        "Resolved: {} plates | {} walkers | {} landmasses max",
        resolved.plate_count, resolved.drunkard_walkers, resolved.max_landmasses
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
        meter_field(ui, "Sea level (m)", &mut config.sea_level_m, defaults.sea_level_m);
        meter_field(
            ui,
            "Ocean floor (m)",
            &mut config.ocean_floor_m,
            defaults.ocean_floor_m,
        );
    });

    egui::CollapsingHeader::new("Land generation").show(ui, |ui| {
        land_generation_mode_field(ui, &mut config.land_generation, defaults.land_generation);
        f32_field(
            ui,
            "Tectonic uplift scale",
            &mut config.tectonic_uplift_scale,
            defaults.tectonic_uplift_scale,
            0.01,
            Some(0.0..=3.0),
        );
    });

    egui::CollapsingHeader::new("Horizontal distances").show(ui, |ui| {
        meter_field(
            ui,
            "Continental margin (m)",
            &mut config.continental_margin_m,
            defaults.continental_margin_m,
        );
        meter_field(
            ui,
            "Min isthmus width (m)",
            &mut config.min_isthmus_width_m,
            defaults.min_isthmus_width_m,
        );
        meter_field(
            ui,
            "Mountain belt width (m)",
            &mut config.mountain_belt_width_m,
            defaults.mountain_belt_width_m,
        );
        meter_field(
            ui,
            "Mountain coast buffer (m)",
            &mut config.mountain_coast_buffer_m,
            defaults.mountain_coast_buffer_m,
        );
        meter_field(
            ui,
            "Coast cleanup proximity (m)",
            &mut config.coast_cleanup_proximity_m,
            defaults.coast_cleanup_proximity_m,
        );
        meter_field(
            ui,
            "Drunkard brush radius (m)",
            &mut config.drunkard_brush_radius_m,
            defaults.drunkard_brush_radius_m,
        );
        meter_field(
            ui,
            "River min length (m)",
            &mut config.river_min_length_m,
            defaults.river_min_length_m,
        );
    });

    egui::CollapsingHeader::new("Areas").show(ui, |ui| {
        square_meter_field(
            ui,
            "Min lake area (m²)",
            &mut config.min_lake_area_m2,
            defaults.min_lake_area_m2,
        );
        square_km_field(
            ui,
            "River min drainage (km²)",
            &mut config.river_min_drainage_area_km2,
            defaults.river_min_drainage_area_km2,
        );
        square_km_field(
            ui,
            "River tributary drainage (km²)",
            &mut config.river_tributary_drainage_area_km2,
            defaults.river_tributary_drainage_area_km2,
        );
    });

    egui::CollapsingHeader::new("Mountains").show(ui, |ui| {
        meter_field(
            ui,
            "Min elevation (m)",
            &mut config.mountain_min_elevation_m,
            defaults.mountain_min_elevation_m,
        );
        degree_field(
            ui,
            "Min slope (°)",
            &mut config.mountain_min_slope_deg,
            defaults.mountain_min_slope_deg,
        );
        f32_field(
            ui,
            "Orogeny mountain threshold",
            &mut config.orogeny_mountain_threshold,
            defaults.orogeny_mountain_threshold,
            0.01,
            Some(0.0..=1.0),
        );
        f32_field(
            ui,
            "Mountain cluster threshold",
            &mut config.mountain_cluster_threshold,
            defaults.mountain_cluster_threshold,
            0.01,
            Some(0.0..=1.0),
        );
        bool_field(
            ui,
            "Use orogeny mountains",
            &mut config.use_orogeny_mountains,
            defaults.use_orogeny_mountains,
        );
        f32_field(
            ui,
            "Mountain boundary weight",
            &mut config.mountain_boundary_weight,
            defaults.mountain_boundary_weight,
            0.01,
            Some(0.0..=2.0),
        );
        meter_field(
            ui,
            "Orogeny interior min dist (m)",
            &mut config.orogeny_interior_min_dist_m,
            defaults.orogeny_interior_min_dist_m,
        );
        meter_field(
            ui,
            "Orogeny peak radius (m)",
            &mut config.orogeny_peak_radius_m,
            defaults.orogeny_peak_radius_m,
        );
        bool_field(
            ui,
            "Mountain noise orogeny only",
            &mut config.mountain_noise_orogeny_only,
            defaults.mountain_noise_orogeny_only,
        );
    });

    egui::CollapsingHeader::new("Oceans").show(ui, |ui| {
        meter_field(
            ui,
            "Shelf width (m)",
            &mut config.shelf_width_m,
            defaults.shelf_width_m,
        );
        meter_field(
            ui,
            "Shelf depth (m)",
            &mut config.shelf_depth_m,
            defaults.shelf_depth_m,
        );
    });

    egui::CollapsingHeader::new("Land texture").show(ui, |ui| {
        land_mask_method_field(ui, &mut config.land_mask_method, defaults.land_mask_method);
        meter_field(
            ui,
            "Texture strength (m)",
            &mut config.land_texture_strength_m,
            defaults.land_texture_strength_m,
        );
        meter_field(
            ui,
            "Texture coast band (m)",
            &mut config.land_texture_coast_band_m,
            defaults.land_texture_coast_band_m,
        );
        meter_field(
            ui,
            "Island zone (m)",
            &mut config.island_zone_m,
            defaults.island_zone_m,
        );
        f32_field(
            ui,
            "Hybrid noise blend",
            &mut config.hybrid_noise_blend,
            defaults.hybrid_noise_blend,
            0.01,
            Some(0.0..=1.0),
        );
        bool_field(
            ui,
            "Use plate macro mask",
            &mut config.use_plate_macro_mask,
            defaults.use_plate_macro_mask,
        );
        f32_field(
            ui,
            "CA fill probability",
            &mut config.ca_fill_probability,
            defaults.ca_fill_probability,
            0.01,
            Some(0.0..=1.0),
        );
        u32_field(
            ui,
            "CA iterations",
            &mut config.ca_iterations,
            defaults.ca_iterations,
            1.0,
            None,
        );
        u32_field(
            ui,
            "CA smoothing passes",
            &mut config.ca_smoothing_passes,
            defaults.ca_smoothing_passes,
            1.0,
            None,
        );
        meter_field(
            ui,
            "CA coarse cell size (m)",
            &mut config.ca_coarse_cell_size_m,
            defaults.ca_coarse_cell_size_m,
        );
        f64_field(
            ui,
            "Drunkard walker density (per km²)",
            &mut config.drunkard_walker_density_per_km2,
            defaults.drunkard_walker_density_per_km2,
            0.01,
            Some(0.0..=5.0),
        );
        u32_field(
            ui,
            "Drunkard steps (0 = auto)",
            &mut config.drunkard_steps,
            defaults.drunkard_steps,
            10.0,
            None,
        );
        meter_field(
            ui,
            "Land shape cell size (m)",
            &mut config.land_shape_cell_size_m,
            defaults.land_shape_cell_size_m,
        );
        meter_field(
            ui,
            "Land mask blur (m)",
            &mut config.land_mask_blur_m,
            defaults.land_mask_blur_m,
        );
        meter_field(
            ui,
            "Land mask close radius (m)",
            &mut config.land_mask_close_radius_m,
            defaults.land_mask_close_radius_m,
        );
        square_km_field(
            ui,
            "Min landmass area (km²)",
            &mut config.min_landmass_area_km2,
            defaults.min_landmass_area_km2,
        );
        f64_field(
            ui,
            "Max landmass density (per km²)",
            &mut config.max_landmass_density_per_km2,
            defaults.max_landmass_density_per_km2,
            0.005,
            Some(0.0..=1.0),
        );
        meter_field(
            ui,
            "Drunkard path length (m)",
            &mut config.drunkard_path_length_m,
            defaults.drunkard_path_length_m,
        );
        f32_field(
            ui,
            "Max landmass compactness",
            &mut config.max_landmass_compactness,
            defaults.max_landmass_compactness,
            1.0,
            Some(1.0..=500.0),
        );
    });

    egui::CollapsingHeader::new("Plates").show(ui, |ui| {
        f64_field(
            ui,
            "Plate density (per km²)",
            &mut config.plate_density_per_km2,
            defaults.plate_density_per_km2,
            0.01,
            Some(0.01..=2.0),
        );
        ui.label(format!(
            "Resolved plate count for current extent: {}",
            resolved.plate_count
        ));
        f32_field(
            ui,
            "Continental plate fraction",
            &mut config.continental_plate_fraction,
            defaults.continental_plate_fraction,
            0.01,
            Some(0.0..=1.0),
        );
        f32_field(
            ui,
            "Oceanic uplift factor",
            &mut config.oceanic_uplift_factor,
            defaults.oceanic_uplift_factor,
            0.01,
            Some(0.0..=2.0),
        );
        f32_field(
            ui,
            "Plate boundary strength",
            &mut config.plate_boundary_strength,
            defaults.plate_boundary_strength,
            0.01,
            Some(0.0..=2.0),
        );
        u32_field(
            ui,
            "Lloyd relaxation iterations",
            &mut config.plate_lloyd_iterations,
            defaults.plate_lloyd_iterations,
            1.0,
            Some(0..=8),
        );
        f32_field(
            ui,
            "Continental plate speed max",
            &mut config.continental_plate_speed_max,
            defaults.continental_plate_speed_max,
            0.01,
            Some(0.0..=2.0),
        );
        f32_field(
            ui,
            "Oceanic plate speed min",
            &mut config.oceanic_plate_speed_min,
            defaults.oceanic_plate_speed_min,
            0.01,
            Some(0.0..=2.0),
        );
        f64_field(
            ui,
            "Mantle flow angle (°)",
            &mut config.mantle_flow_angle_deg,
            defaults.mantle_flow_angle_deg,
            1.0,
            Some(-180.0..=180.0),
        );
    });

    egui::CollapsingHeader::new("Coast").show(ui, |ui| {
        f32_field(
            ui,
            "Coast sharpening",
            &mut config.coast_sharpening,
            defaults.coast_sharpening,
            0.01,
            Some(0.0..=1.0),
        );
        u32_field(
            ui,
            "Coast cleanup passes",
            &mut config.coast_cleanup_passes,
            defaults.coast_cleanup_passes,
            1.0,
            None,
        );
    });

    egui::CollapsingHeader::new("Climate").show(ui, |ui| {
        celsius_field(
            ui,
            "Equator mean temp (°C)",
            &mut config.equator_mean_temp_c,
            defaults.equator_mean_temp_c,
        );
        celsius_field(
            ui,
            "Pole mean temp (°C)",
            &mut config.pole_mean_temp_c,
            defaults.pole_mean_temp_c,
        );
        meter_field(
            ui,
            "Temperature wavelength (m)",
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
        f32_field(
            ui,
            "Rainfall scale",
            &mut config.rainfall_scale,
            defaults.rainfall_scale,
            0.01,
            Some(0.0..=3.0),
        );
        f32_field(
            ui,
            "Orographic orogeny weight",
            &mut config.orographic_orogeny_weight,
            defaults.orographic_orogeny_weight,
            0.01,
            Some(0.0..=2.0),
        );
        f32_field(
            ui,
            "Interior drying factor",
            &mut config.interior_drying_factor,
            defaults.interior_drying_factor,
            0.01,
            Some(0.0..=1.0),
        );
        f32_field(
            ui,
            "Continentality strength",
            &mut config.continentality_strength,
            defaults.continentality_strength,
            0.01,
            Some(0.0..=1.0),
        );
        meter_field(
            ui,
            "Continentality ocean range (m)",
            &mut config.continentality_ocean_range_m,
            defaults.continentality_ocean_range_m,
        );
        wind_direction_field(ui, &mut config.wind_direction, defaults.wind_direction);
    });

    egui::CollapsingHeader::new("Landscape evolution").show(ui, |ui| {
        bool_field(
            ui,
            "Enabled",
            &mut config.landscape_evolution_enabled,
            defaults.landscape_evolution_enabled,
        );
        u32_field(
            ui,
            "Iterations",
            &mut config.landscape_evolution_iterations,
            defaults.landscape_evolution_iterations,
            1.0,
            Some(1..=64),
        );
        u32_field(
            ui,
            "Coarse hydro factor",
            &mut config.coarse_hydro_factor,
            defaults.coarse_hydro_factor,
            1.0,
            Some(1..=16),
        );
        u32_field(
            ui,
            "Full-res passes",
            &mut config.landscape_evolution_full_res_passes,
            defaults.landscape_evolution_full_res_passes,
            1.0,
            Some(0..=32),
        );
        f32_field(
            ui,
            "Erosion factor",
            &mut config.landscape_erosion_factor,
            defaults.landscape_erosion_factor,
            0.0005,
            Some(0.0..=0.05),
        );
        f32_field(
            ui,
            "Uplift factor",
            &mut config.landscape_uplift_factor,
            defaults.landscape_uplift_factor,
            0.0005,
            Some(0.0..=0.05),
        );
        f32_field(
            ui,
            "Erodibility plains",
            &mut config.erodibility_plains,
            defaults.erodibility_plains,
            0.1,
            Some(0.1..=10.0),
        );
        f32_field(
            ui,
            "Erodibility mountains",
            &mut config.erodibility_mountains,
            defaults.erodibility_mountains,
            0.1,
            Some(0.1..=10.0),
        );
        f32_field(
            ui,
            "Rainfall erodibility coupling",
            &mut config.rainfall_erodibility_coupling,
            defaults.rainfall_erodibility_coupling,
            0.01,
            Some(0.0..=1.0),
        );
    });

    egui::CollapsingHeader::new("Rivers").show(ui, |ui| {
        bool_field(
            ui,
            "River incision enabled",
            &mut config.river_incision_enabled,
            defaults.river_incision_enabled,
        );
        f32_field(
            ui,
            "River incision factor",
            &mut config.river_incision_factor,
            defaults.river_incision_factor,
            0.0005,
            Some(0.0..=0.05),
        );
        f32_field(
            ui,
            "River meander strength",
            &mut config.river_meander_strength,
            defaults.river_meander_strength,
            0.01,
            Some(0.0..=1.0),
        );
    });

    egui::CollapsingHeader::new("Noise wavelengths").show(ui, |ui| {
        meter_field(
            ui,
            "Continent wavelength (m)",
            &mut config.continent_wavelength_m,
            defaults.continent_wavelength_m,
        );
        meter_field(
            ui,
            "Hill wavelength (m)",
            &mut config.hill_wavelength_m,
            defaults.hill_wavelength_m,
        );
        meter_field(
            ui,
            "Mountain detail wavelength (m)",
            &mut config.mountain_detail_wavelength_m,
            defaults.mountain_detail_wavelength_m,
        );
        meter_field(
            ui,
            "Land mask wavelength (m)",
            &mut config.land_mask_wavelength_m,
            defaults.land_mask_wavelength_m,
        );
    });
}

fn reset_button(ui: &mut Ui) -> bool {
    ui.small_button(RESET_BTN)
        .on_hover_text("Reset to default")
        .clicked()
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

fn square_km_field(
    ui: &mut Ui,
    label: &str,
    value: &mut SquareKilometers,
    default: SquareKilometers,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut value.0)
                .speed(0.001)
                .range(0.0..=100.0),
        );
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn celsius_field(ui: &mut Ui, label: &str, value: &mut Celsius, default: Celsius) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut value.0).speed(0.5));
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn degree_field(ui: &mut Ui, label: &str, value: &mut Degrees, default: Degrees) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut value.0).speed(0.1).range(0.0..=90.0));
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

fn land_generation_mode_field(
    ui: &mut Ui,
    mode: &mut LandGenerationMode,
    default: LandGenerationMode,
) {
    ui.label("Land generation mode");
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("land_generation_mode")
            .selected_text(land_generation_label(*mode))
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(mode, LandGenerationMode::TectonicBase, "TectonicBase");
                ui.selectable_value(mode, LandGenerationMode::LegacyMask, "LegacyMask");
            });
        if reset_button(ui) {
            *mode = default;
        }
    });
    ui.add_space(4.0);
}

fn land_generation_label(mode: LandGenerationMode) -> &'static str {
    match mode {
        LandGenerationMode::TectonicBase => "TectonicBase",
        LandGenerationMode::LegacyMask => "LegacyMask",
    }
}

fn bool_field(ui: &mut Ui, label: &str, value: &mut bool, default: bool) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.checkbox(value, "");
        if reset_button(ui) {
            *value = default;
        }
    });
    ui.add_space(4.0);
}

fn land_mask_method_field(ui: &mut Ui, method: &mut LandMaskMethod, default: LandMaskMethod) {
    ui.label("Land mask method");
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("land_mask_method")
            .selected_text(land_mask_label(*method))
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(method, LandMaskMethod::Hybrid, "Hybrid");
                ui.selectable_value(method, LandMaskMethod::Noise, "Noise");
                ui.selectable_value(method, LandMaskMethod::CellularAutomata, "CellularAutomata");
                ui.selectable_value(method, LandMaskMethod::DrunkardsWalk, "DrunkardsWalk");
            });
        if reset_button(ui) {
            *method = default;
        }
    });
    ui.add_space(4.0);
}

fn wind_direction_field(ui: &mut Ui, direction: &mut WindDirection, default: WindDirection) {
    ui.label("Wind direction");
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("wind_direction")
            .selected_text("WestToEast")
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(direction, WindDirection::WestToEast, "WestToEast");
            });
        if reset_button(ui) {
            *direction = default;
        }
    });
    ui.add_space(4.0);
}

fn land_mask_label(method: LandMaskMethod) -> &'static str {
    match method {
        LandMaskMethod::Hybrid => "Hybrid",
        LandMaskMethod::Noise => "Noise",
        LandMaskMethod::CellularAutomata => "CellularAutomata",
        LandMaskMethod::DrunkardsWalk => "DrunkardsWalk",
    }
}
