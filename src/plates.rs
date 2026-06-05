use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;

use super::config::WorldGenConfig;
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

#[derive(Debug, Clone)]
pub struct Plate {
    pub id: u32,
    pub center_x: f32,
    pub center_y: f32,
    pub velocity_x: f32,
    pub velocity_y: f32,
}

pub struct PlateData {
    pub plates: Vec<Plate>,
}

/// Scale plate count with map area (design: 20–100 depending on size).
pub fn plate_count_for_map(config: &WorldGenConfig) -> u32 {
    let area = (config.width * config.height) as f32;
    let scaled = (area.sqrt() / 16.0) as u32;
    scaled.clamp(20, 100).min(config.plate_count.max(8))
}

pub fn generate_plates(config: &WorldGenConfig, rng: &mut ChaCha8Rng) -> PlateData {
    let count = plate_count_for_map(config);
    let mut plates = Vec::with_capacity(count as usize);

    for id in 0..count {
        let center_x = rng.gen_range(0.0..config.width as f32);
        let center_y = rng.gen_range(0.0..config.height as f32);
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let speed = rng.gen_range(0.2..1.0);
        plates.push(Plate {
            id,
            center_x,
            center_y,
            velocity_x: angle.cos() * speed,
            velocity_y: angle.sin() * speed,
        });
    }

    PlateData { plates }
}

/// Assign every cell to the nearest plate center (Voronoi-style partition).
pub fn assign_plate_ids(
    map: &mut WorldMap,
    plates: &PlateData,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;

    map.plate_id
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 4 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / h as f32,
                    "Assigning plate regions",
                );
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let mut best_id = 0u32;
                let mut best_dist = f32::MAX;

                for plate in &plates.plates {
                    let dx = x as f32 + 0.5 - plate.center_x;
                    let dy = y as f32 + 0.5 - plate.center_y;
                    let dist = dx * dx + dy * dy;
                    if dist < best_dist {
                        best_dist = dist;
                        best_id = plate.id;
                    }
                }

                *cell = best_id;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    #[test]
    fn plate_assignment_covers_map() {
        let config = WorldGenConfig::test_config(1, 32);
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let plates = generate_plates(&config, &mut rng);
        let mut map = WorldMap::new(config.width, config.height, config.seed);
        assign_plate_ids(&mut map, &plates, &None, 0.0, 1.0);
        assert!(map.plate_id.iter().all(|&id| id < plates.plates.len() as u32));
    }
}
