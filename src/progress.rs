use std::sync::{Arc, Mutex};

/// Thread-safe progress snapshot updated during `generate_world_with_progress`.
#[derive(Clone, Default)]
pub struct GenProgressReport {
    pub fraction: f32,
    pub label: String,
}

pub type ProgressHandle = Arc<Mutex<GenProgressReport>>;

pub fn new_progress_handle() -> ProgressHandle {
    Arc::new(Mutex::new(GenProgressReport::default()))
}

pub fn report(handle: &Option<ProgressHandle>, fraction: f32, label: &str) {
    let Some(handle) = handle else {
        return;
    };
    if let Ok(mut report) = handle.lock() {
        report.fraction = fraction.clamp(0.0, 1.0);
        report.label = label.to_string();
    }
}

/// Map sub-progress within a pipeline stage (sub in 0.0..1.0).
pub fn report_stage(
    handle: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
    sub: f32,
    label: &str,
) {
    let frac = stage_start + sub.clamp(0.0, 1.0) * (stage_end - stage_start);
    report(handle, frac, label);
}
