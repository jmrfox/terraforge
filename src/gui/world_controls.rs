use egui::{Align, Align2, FontId, Layout, Pos2, Response, Sense, Stroke, Ui, Vec2};
use terraforge::{WorldControl, WorldGenConfig};

const COMPASS_SIZE: f32 = 72.0;
const COMPASS_POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
const CUSTOM_HINT: &str =
    "Advanced values no longer match this control. Moving it replaces the values it drives";

/// Draw the high-level controls. Returns true when the map should regenerate.
pub fn draw_world_controls(
    ui: &mut Ui,
    config: &mut WorldGenConfig,
    rescale_cell_size_m: &mut f64,
    auto_generate: &mut bool,
) -> bool {
    let cell_size_before = config.cell_size_m.0;
    let mut finished_edit = false;

    ui.strong("World");
    for control in WorldControl::ALL {
        finished_edit |= draw_header(ui, config, control);
        finished_edit |= match control {
            WorldControl::Wind => draw_wind(ui, config),
            _ => draw_slider(ui, config, control),
        };
        ui.add_space(4.0);
    }
    ui.checkbox(auto_generate, "Update map after each change")
        .on_hover_text("Regenerate when a control is released. Turn off for very large maps");

    if config.cell_size_m.0 != cell_size_before {
        *rescale_cell_size_m = config.cell_size_m.0;
    }
    finished_edit && *auto_generate
}

/// Label row with a custom marker and reset button. Returns true on reset.
fn draw_header(ui: &mut Ui, config: &mut WorldGenConfig, control: WorldControl) -> bool {
    let reset = ui
        .horizontal(|ui| {
            ui.label(control.label());
            if control.is_custom(config) {
                ui.weak("custom").on_hover_text(CUSTOM_HINT);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.small_button("↺")
                    .on_hover_text("Reset to default")
                    .clicked()
            })
            .inner
        })
        .inner;
    if reset {
        control.apply(config, control.default_value(config));
    }
    reset
}

fn draw_slider(ui: &mut Ui, config: &mut WorldGenConfig, control: WorldControl) -> bool {
    let mut value = control.value(config);
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(48.0);
            // Formatters only change the display. Decimal limits would round the value
            // and write it back into the config on an idle frame.
            let slider = egui::Slider::new(&mut value, control.range());
            let slider = if control == WorldControl::Scale {
                slider
                    .logarithmic(true)
                    .suffix(" km")
                    .custom_formatter(|v, _| format!("{v:.0}"))
            } else {
                slider.custom_formatter(|v, _| format!("{v:.2}"))
            };
            ui.add(slider)
        })
        .inner;
    if response.changed() {
        control.apply(config, value);
    }

    let (low, high) = control.end_labels();
    ui.horizontal(|ui| {
        ui.small(low);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(60.0);
            ui.small(high);
        });
    });
    if control == WorldControl::Scale {
        ui.small(format!("{:.0} m per cell", config.cell_size_m.0));
    }
    edit_finished(&response)
}

fn draw_wind(ui: &mut Ui, config: &mut WorldGenConfig) -> bool {
    let control = WorldControl::Wind;
    let mut bearing = control.value(config);
    let mut finished = false;
    ui.horizontal(|ui| {
        let compass = wind_compass(ui, &mut bearing);
        finished |= compass.drag_stopped() || compass.clicked();
        let mut changed = compass.changed();
        ui.vertical(|ui| {
            let field = ui.add(
                egui::DragValue::new(&mut bearing)
                    .speed(1.0)
                    .range(control.range())
                    .custom_formatter(|v, _| format!("{v:.0}"))
                    .suffix("°"),
            );
            changed |= field.changed();
            finished |= edit_finished(&field);
            ui.small(compass_point(bearing));
        });
        if changed {
            control.apply(config, bearing);
        }
    });
    finished
}

/// A continuous drag finishes on release. Clicks, keys, and typed values finish at once.
fn edit_finished(response: &Response) -> bool {
    response.drag_stopped() || (response.changed() && !response.dragged())
}

fn compass_point(bearing: f64) -> &'static str {
    let sector = ((bearing + 22.5).rem_euclid(360.0) / 45.0) as usize;
    COMPASS_POINTS[sector.min(COMPASS_POINTS.len() - 1)]
}

/// Offset from the compass center toward a bearing, with north up.
fn bearing_offset(bearing_deg: f32, radius: f32) -> Vec2 {
    let angle = bearing_deg.to_radians();
    Vec2::new(angle.sin(), -angle.cos()) * radius
}

fn bearing_from_offset(offset: Vec2) -> f64 {
    f64::from(offset.x.atan2(-offset.y).to_degrees())
        .round()
        .rem_euclid(360.0)
}

/// Click or drag toward the direction the wind comes from.
fn wind_compass(ui: &mut Ui, bearing: &mut f64) -> Response {
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::splat(COMPASS_SIZE), Sense::click_and_drag());
    let center: Pos2 = rect.center();
    if let Some(pointer) = response.interact_pointer_pos() {
        let offset = pointer - center;
        if offset.length() > 3.0 {
            let picked = bearing_from_offset(offset);
            if picked != *bearing {
                *bearing = picked;
                response.mark_changed();
            }
        }
    }

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let radius = COMPASS_SIZE * 0.5 - 1.0;
        let painter = ui.painter();
        painter.circle(center, radius, visuals.bg_fill, visuals.fg_stroke);
        for (label, deg) in [("N", 0.0f32), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
            painter.text(
                center + bearing_offset(deg, radius - 8.0),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                ui.visuals().text_color(),
            );
        }
        // The arrow starts on the upwind side and points the way the air moves.
        let upwind = bearing_offset(*bearing as f32, radius * 0.62);
        painter.arrow(
            center + upwind,
            -upwind * 2.0,
            Stroke::new(2.0f32, ui.visuals().selection.stroke.color),
        );
    }
    response.on_hover_text("Direction the prevailing wind blows from")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compass_points_match_bearings() {
        assert_eq!(compass_point(0.0), "N");
        assert_eq!(compass_point(359.0), "N");
        assert_eq!(compass_point(90.0), "E");
        assert_eq!(compass_point(225.0), "SW");
        assert_eq!(compass_point(270.0), "W");
    }

    #[test]
    fn pointer_offsets_map_to_compass_bearings() {
        assert_eq!(bearing_from_offset(Vec2::new(0.0, -10.0)), 0.0);
        assert_eq!(bearing_from_offset(Vec2::new(10.0, 0.0)), 90.0);
        assert_eq!(bearing_from_offset(Vec2::new(0.0, 10.0)), 180.0);
        assert_eq!(bearing_from_offset(Vec2::new(-10.0, 0.0)), 270.0);
    }
}
