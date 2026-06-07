//! Interactive map parameter GUI — tweak config, generate, and preview layers.

#[path = "../gui/mod.rs"]
mod gui;

use eframe::egui;

/// Global UI scale (1.0 = 100%). Increase for larger text and controls.
const UI_ZOOM_FACTOR: f32 = 1.4;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Terraforge Map GUI",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_zoom_factor(UI_ZOOM_FACTOR);
            Ok(Box::new(gui::MapGuiApp::new(cc)))
        }),
    )
}
