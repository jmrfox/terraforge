use egui::Ui;
use terraforge::PriorSet;

pub fn draw_sampling(ui: &mut Ui, prior_set: &mut PriorSet) {
    ui.label("Sample numerical parameters from Earth-like priors centered on tuned defaults. Grid size, cell size, seed, enums, and boolean toggles are not sampled.");
    ui.horizontal(|ui| {
        if ui.button("Enable all").clicked() {
            prior_set.enable_all();
        }
        if ui.button("Disable all").clicked() {
            prior_set.disable_all();
        }
        if ui
            .button("Reset selection")
            .on_hover_text(
                "Restore default checkboxes for which parameters are included in sampling",
            )
            .clicked()
        {
            prior_set.reset_sampling_selection();
        }
    });
    ui.label(format!(
        "{} of {} parameters enabled for sampling",
        prior_set.enabled_count(),
        prior_set.params.len()
    ));
    ui.add_space(4.0);

    let mut current_category = "";
    for param in &mut prior_set.params {
        if param.category != current_category {
            current_category = param.category;
            ui.add_space(6.0);
            ui.label(egui::RichText::new(current_category).strong());
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut param.enabled, param.label);
            ui.label(egui::RichText::new(param.dist.summary()).small().weak());
        });
    }
}
