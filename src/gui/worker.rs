use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use terraforge::{
    compute_map_stats, generate_world_with_progress, new_progress_handle, MapStats, ProgressHandle,
    WorldGenConfig, WorldMap,
};

pub struct GenResult {
    pub map: WorldMap,
    pub stats: MapStats,
    pub elapsed_ms: u64,
}

pub struct GenJob {
    pub progress: ProgressHandle,
    pub rx: mpsc::Receiver<Result<GenResult, String>>,
}

pub fn spawn_generation(config: WorldGenConfig) -> GenJob {
    let progress = new_progress_handle();
    let (tx, rx) = mpsc::channel();
    let progress_clone = progress.clone();

    thread::spawn(move || {
        let start = Instant::now();
        let map = generate_world_with_progress(&config, Some(progress_clone));
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let stats = compute_map_stats(&map, &config, elapsed_ms);
        let _ = tx.send(Ok(GenResult {
            map,
            stats,
            elapsed_ms,
        }));
    });

    GenJob { progress, rx }
}

pub fn poll_progress(progress: &ProgressHandle) -> (f32, String) {
    progress
        .lock()
        .map(|report| (report.fraction, report.label.clone()))
        .unwrap_or((0.0, String::new()))
}
