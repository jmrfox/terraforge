use std::fs;

use eframe::egui;
use rand::Rng;
use terraforge::{
    LEGEND_ENTRIES, PreviewLayer, PriorSet, RIVER_RGBA, WorldGenConfig, WorldMap, biome_rgba,
    write_map_png,
};

use super::params::draw_params;
use super::sampling::draw_sampling;
use super::preview::{MapTexture, upload_map_texture};
use super::worker::{GenJob, GenResult, poll_progress, spawn_generation};

const MAP_ZOOM_MIN: f32 = 0.25;
const MAP_ZOOM_MAX: f32 = 16.0;

enum GenState {
    Idle,
    Running(GenJob),
    Done {
        map: WorldMap,
        stats: terraforge::MapStats,
        elapsed_ms: u64,
    },
    Error(String),
}

pub struct MapGuiApp {
    config: WorldGenConfig,
    prior_set: PriorSet,
    calibrate_land_target: f32,
    gen_state: GenState,
    layer: PreviewLayer,
    rivers_overlay: bool,
    texture: Option<MapTexture>,
    map_zoom: f32,
    map_zoom_fit: bool,
    status_message: String,
    pending_load: Option<String>,
    pending_save_config: bool,
    pending_export_png: bool,
}

impl MapGuiApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = WorldGenConfig::default();
        let mut app = Self {
            config,
            prior_set: PriorSet::default_priors(),
            calibrate_land_target: 0.30,
            gen_state: GenState::Idle,
            layer: PreviewLayer::Biomes,
            rivers_overlay: true,
            texture: None,
            map_zoom: 1.0,
            map_zoom_fit: false,
            status_message: "Generating initial map…".into(),
            pending_load: None,
            pending_save_config: false,
            pending_export_png: false,
        };
        app.start_generation();
        app
    }

    /// Start a new generation, abandoning any in-progress run (its result is discarded).
    fn start_generation(&mut self) {
        let config = self.config.clone();
        self.gen_state = GenState::Running(spawn_generation(config));
        self.status_message = "Generating…".into();
    }

    fn sample_and_generate(&mut self) {
        let mut rng = rand::thread_rng();
        self.prior_set.sample_into(&mut self.config, &mut rng);
        self.config.seed = rand_seed();
        self.status_message = format!(
            "Sampled {} parameters, seed {}",
            self.prior_set.enabled_count(),
            self.config.seed
        );
        self.start_generation();
    }

    fn poll_generation(&mut self, ctx: &egui::Context) {
        if let GenState::Running(job) = &self.gen_state {
            let (fraction, label) = poll_progress(&job.progress);
            self.status_message = if label.is_empty() {
                format!("Generating… {:.0}%", fraction * 100.0)
            } else {
                format!("{label} ({:.0}%)", fraction * 100.0)
            };

            if let Ok(result) = job.rx.try_recv() {
                match result {
                    Ok(gen) => {
                        self.on_generation_complete(ctx, gen);
                    }
                    Err(msg) => {
                        self.gen_state = GenState::Error(msg.clone());
                        self.status_message = msg;
                    }
                }
            } else {
                ctx.request_repaint();
            }
        }
    }

    fn on_generation_complete(&mut self, ctx: &egui::Context, gen: GenResult) {
        self.texture = Some(upload_map_texture(
            ctx,
            &gen.map,
            self.layer,
            self.rivers_overlay,
            None,
        ));
        self.status_message = format!("Done in {} ms", gen.elapsed_ms);
        self.gen_state = GenState::Done {
            map: gen.map,
            stats: gen.stats,
            elapsed_ms: gen.elapsed_ms,
        };
    }

    fn refresh_texture(&mut self, ctx: &egui::Context) {
        if let GenState::Done { map, .. } = &self.gen_state {
            let map = map.clone();
            self.texture = Some(upload_map_texture(
                ctx,
                &map,
                self.layer,
                self.rivers_overlay,
                Some("map_preview"),
            ));
        }
    }

    fn is_generating(&self) -> bool {
        matches!(self.gen_state, GenState::Running(_))
    }

    fn clamp_map_zoom(zoom: f32) -> f32 {
        zoom.clamp(MAP_ZOOM_MIN, MAP_ZOOM_MAX)
    }

    fn apply_zoom_scroll(&mut self, scroll_y: f32) {
        if scroll_y == 0.0 {
            return;
        }
        self.map_zoom = Self::clamp_map_zoom(self.map_zoom * 1.1f32.powf(scroll_y / 40.0));
    }

    fn draw_zoom_controls(&mut self, ui: &mut egui::Ui) {
        ui.label("Zoom:");
        if ui.small_button("−").clicked() {
            self.map_zoom = Self::clamp_map_zoom(self.map_zoom / 1.25);
        }
        if ui.small_button("+").clicked() {
            self.map_zoom = Self::clamp_map_zoom(self.map_zoom * 1.25);
        }
        if ui.small_button("100%").clicked() {
            self.map_zoom = 1.0;
        }
        if ui.small_button("Fit").clicked() {
            self.map_zoom_fit = true;
        }
        ui.add(
            egui::DragValue::new(&mut self.map_zoom)
                .speed(0.02)
                .range(MAP_ZOOM_MIN..=MAP_ZOOM_MAX)
                .suffix("×"),
        );
        ui.label(egui::RichText::new("Scroll over map to zoom").weak().small());
    }

    fn current_map(&self) -> Option<&WorldMap> {
        match &self.gen_state {
            GenState::Done { map, .. } => Some(map),
            _ => None,
        }
    }

    fn handle_file_dialogs(&mut self) {
        if self.pending_save_config {
            self.pending_save_config = false;
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name("preset.json")
                .save_file()
            {
                match serde_json::to_string_pretty(&self.config) {
                    Ok(json) => match fs::write(&path, json) {
                        Ok(()) => self.status_message = format!("Saved preset to {}", path.display()),
                        Err(e) => self.status_message = format!("Save failed: {e}"),
                    },
                    Err(e) => self.status_message = format!("Serialize failed: {e}"),
                }
            }
        }

        if self.pending_export_png {
            self.pending_export_png = false;
            if let Some(map) = self.current_map().cloned() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .set_file_name("map.png")
                    .save_file()
                {
                    match write_map_png(&map, &path) {
                        Ok(()) => self.status_message = format!("Exported PNG to {}", path.display()),
                        Err(e) => self.status_message = format!("Export failed: {e}"),
                    }
                }
            }
        }

        if let Some(text) = self.pending_load.take() {
            match serde_json::from_str::<WorldGenConfig>(&text) {
                Ok(loaded) => {
                    self.config = loaded;
                    self.status_message = "Preset loaded".into();
                }
                Err(e) => self.status_message = format!("Invalid preset: {e}"),
            }
        }
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .button("Generate")
                .on_hover_text("Start generation; replaces any in-progress run")
                .clicked()
            {
                self.start_generation();
            }
            if ui.button("Randomize seed").clicked() {
                self.config.seed = rand_seed();
            }
            if ui
                .add_enabled(
                    self.prior_set.enabled_count() > 0,
                    egui::Button::new("Sample & generate"),
                )
                .on_hover_text(
                    "Draw enabled parameters from priors, randomize seed, and generate (replaces any in-progress run)",
                )
                .clicked()
            {
                self.sample_and_generate();
            }
            if ui.button("Load preset").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()
                {
                    match fs::read_to_string(&path) {
                        Ok(text) => self.pending_load = Some(text),
                        Err(e) => self.status_message = format!("Read failed: {e}"),
                    }
                }
            }
            if ui.button("Save preset").clicked() {
                self.pending_save_config = true;
            }
            if ui
                .add_enabled(self.current_map().is_some(), egui::Button::new("Export PNG"))
                .clicked()
            {
                self.pending_export_png = true;
            }
        });
    }

    fn draw_preview_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut layer_changed = false;
        let mut overlay_changed = false;

        ui.horizontal(|ui| {
            ui.label("Layer:");
            overlay_changed = ui
                .checkbox(&mut self.rivers_overlay, "Rivers overlay")
                .changed();
        });
        ui.horizontal_wrapped(|ui| {
            for layer in PreviewLayer::ALL {
                if ui
                    .selectable_label(self.layer == layer, layer.label())
                    .clicked()
                {
                    self.layer = layer;
                    layer_changed = true;
                }
            }
        });

        ui.horizontal(|ui| {
            self.draw_zoom_controls(ui);
        });

        if layer_changed || overlay_changed {
            self.refresh_texture(ctx);
        }

        if self.is_generating() {
            if let GenState::Running(job) = &self.gen_state {
                let (fraction, _) = poll_progress(&job.progress);
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
            }
        }

        if let GenState::Error(msg) = &self.gen_state {
            ui.colored_label(egui::Color32::RED, msg);
        } else {
            ui.label(&self.status_message);
        }
    }

    fn draw_preview_image(&mut self, ui: &mut egui::Ui) {
        let panel_rect = ui.max_rect();
        let (wheel, pointer_over_map, ctrl_held) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.pointer
                    .hover_pos()
                    .is_some_and(|pos| panel_rect.contains(pos)),
                i.modifiers.ctrl,
            )
        });
        let zooming = wheel != 0.0 && (pointer_over_map || ctrl_held);
        if zooming {
            self.apply_zoom_scroll(wheel);
        }

        if let Some(tex) = &self.texture {
            if self.map_zoom_fit {
                let avail = ui.available_size();
                self.map_zoom = Self::clamp_map_zoom(
                    (avail.x / tex.width as f32).min(avail.y / tex.height as f32),
                );
                self.map_zoom_fit = false;
            }

            let size = egui::vec2(tex.width as f32, tex.height as f32) * self.map_zoom;
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .enable_scrolling(!zooming)
                .show(ui, |ui| {
                    ui.image((tex.handle.id(), size));
                });
        } else {
            ui.label("No map yet — click Generate.");
        }
    }

    fn draw_legend(&self, ui: &mut egui::Ui) {
        if let Some(hint) = self.layer.legend_hint() {
            ui.heading("Legend");
            ui.label(hint);
            return;
        }
        ui.heading("Legend");
        egui::Grid::new("biome_legend")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for (name, biome) in LEGEND_ENTRIES {
                    let rgba = biome_rgba(*biome);
                    let color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                    ui.colored_label(color, "■");
                    ui.label(*name);
                    ui.end_row();
                }
                let river_color = egui::Color32::from_rgba_unmultiplied(
                    RIVER_RGBA[0],
                    RIVER_RGBA[1],
                    RIVER_RGBA[2],
                    RIVER_RGBA[3],
                );
                ui.colored_label(river_color, "■");
                ui.label("River");
                ui.end_row();
            });
    }

    fn draw_stats(&mut self, ui: &mut egui::Ui) {
        let (stats, elapsed_ms, map) = match &self.gen_state {
            GenState::Done { stats, elapsed_ms, map, .. } => (stats, elapsed_ms, map),
            _ => return,
        };
        ui.heading("Stats");
        ui.label(format!(
            "Extent: {:.2} × {:.2} km",
            stats.map_width_m / 1000.0,
            stats.map_height_m / 1000.0,
        ));
        ui.label(format!("Cell size: {:.0} m", stats.cell_size_m));
        ui.label(format!("Generated in {elapsed_ms} ms"));
        ui.label(format!("Land: {:.1}%", stats.land_fraction * 100.0));
        ui.label(format!("Ocean: {:.1}%", stats.ocean_fraction * 100.0));
        ui.label(format!("Sea level: {:.0} m", stats.sea_level_m));
        ui.add_space(6.0);
        ui.label("Calibrate sea level (editor)");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.calibrate_land_target)
                    .speed(0.01)
                    .range(0.05..=0.95),
            );
            if ui.button("Apply").clicked() {
                let suggested = self.config.suggest_sea_level_m_for_fraction(
                    &map.elevation,
                    self.calibrate_land_target,
                );
                self.config.sea_level_m = suggested;
                self.status_message = format!(
                    "Sea level set to {:.0} m for ~{:.0}% land (re-generate to apply)",
                    suggested.0,
                    self.calibrate_land_target * 100.0
                );
            }
        });
        ui.add_space(4.0);
        ui.label("Biomes");
        egui::Grid::new("biome_stats")
            .num_columns(2)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                let mut entries: Vec<_> = stats.biomes.iter().collect();
                entries.sort_by(|a, b| b.1.cmp(a.1));
                for (name, count) in entries {
                    ui.label(name.as_str());
                    ui.label(count.to_string());
                    ui.end_row();
                }
            });
    }
}

impl eframe::App for MapGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_generation(ctx);
        self.handle_file_dialogs();

        egui::SidePanel::left("params_panel")
            .resizable(true)
            .default_width(340.0)
            .min_width(280.0)
            .show(ctx, |ui| {
                self.draw_toolbar(ui);
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::CollapsingHeader::new("Parameter sampling")
                            .default_open(false)
                            .show(ui, |ui| {
                                draw_sampling(ui, &mut self.prior_set);
                            });
                        ui.separator();
                        draw_params(ui, &mut self.config);
                    });
            });

        egui::SidePanel::right("info_panel")
            .resizable(true)
            .default_width(240.0)
            .min_width(180.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.draw_legend(ui);
                        if self.layer == PreviewLayer::Biomes
                            && matches!(self.gen_state, GenState::Done { .. })
                        {
                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);
                        }
                        self.draw_stats(ui);
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_preview_controls(ui, ctx);
            ui.separator();
            self.draw_preview_image(ui);
        });
    }
}

fn rand_seed() -> u64 {
    rand::thread_rng().gen()
}
