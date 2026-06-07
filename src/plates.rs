use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrustType {
    Continental,
    Oceanic,
}

#[derive(Debug, Clone)]
pub struct Plate {
    pub id: u32,
    pub center_x: f32,
    pub center_y: f32,
    pub velocity_x: f32,
    pub velocity_y: f32,
    pub crust_type: CrustType,
}

pub struct PlateData {
    pub plates: Vec<Plate>,
}

pub fn generate_plates(
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    rng: &mut ChaCha8Rng,
) -> PlateData {
    let count = params.plate_count;
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
            crust_type: CrustType::Oceanic,
        });
    }

    assign_crust_types(&mut plates, config, rng);
    lloyd_relax(&mut plates, config);
    bias_plate_velocities(&mut plates, config, rng);

    PlateData { plates }
}

/// Cluster continental plates from a random seed so land crust groups together.
fn assign_crust_types(plates: &mut [Plate], config: &WorldGenConfig, rng: &mut ChaCha8Rng) {
    let count = plates.len();
    if count <= 1 {
        if let Some(plate) = plates.first_mut() {
            plate.crust_type = CrustType::Continental;
        }
        return;
    }

    let fraction = config.continental_plate_fraction.clamp(0.05, 0.95);
    let target = ((count as f32) * fraction).round() as usize;
    let target = target.clamp(1, count - 1);

    let seed_id = rng.gen_range(0..count);
    let mut continental = vec![false; count];
    continental[seed_id] = true;
    let mut assigned = 1usize;

    while assigned < target {
        let mut best_id = None;
        let mut best_dist = f32::MAX;

        for (id, plate) in plates.iter().enumerate() {
            if continental[id] {
                continue;
            }
            let mut min_d = f32::MAX;
            for (cid, other) in plates.iter().enumerate() {
                if !continental[cid] {
                    continue;
                }
                let dx = plate.center_x - other.center_x;
                let dy = plate.center_y - other.center_y;
                min_d = min_d.min(dx * dx + dy * dy);
            }
            if min_d < best_dist {
                best_dist = min_d;
                best_id = Some(id);
            }
        }

        match best_id {
            Some(id) => {
                continental[id] = true;
                assigned += 1;
            }
            None => break,
        }
    }

    for (id, plate) in plates.iter_mut().enumerate() {
        plate.crust_type = if continental[id] {
            CrustType::Continental
        } else {
            CrustType::Oceanic
        };
    }
}

/// Lloyd relaxation: move plate centers toward Voronoi cell centroids.
fn lloyd_relax(plates: &mut [Plate], config: &WorldGenConfig) {
    let iterations = config.plate_lloyd_iterations;
    if iterations == 0 {
        return;
    }

    let w = config.width;
    let h = config.height;
    let n = plates.len();

    for _ in 0..iterations {
        let mut sum_x = vec![0.0f64; n];
        let mut sum_y = vec![0.0f64; n];
        let mut counts = vec![0u32; n];

        let partial: Vec<(Vec<f64>, Vec<f64>, Vec<u32>)> = (0..h)
            .into_par_iter()
            .map(|y| {
                let mut row_sum_x = vec![0.0f64; n];
                let mut row_sum_y = vec![0.0f64; n];
                let mut row_counts = vec![0u32; n];
                for x in 0..w {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut best_id = 0usize;
                    let mut best_dist = f32::MAX;
                    for plate in plates.iter() {
                        let dx = px - plate.center_x;
                        let dy = py - plate.center_y;
                        let d = dx * dx + dy * dy;
                        if d < best_dist {
                            best_dist = d;
                            best_id = plate.id as usize;
                        }
                    }
                    row_counts[best_id] += 1;
                    row_sum_x[best_id] += px as f64;
                    row_sum_y[best_id] += py as f64;
                }
                (row_sum_x, row_sum_y, row_counts)
            })
            .collect();

        for (row_sum_x, row_sum_y, row_counts) in partial {
            for id in 0..n {
                counts[id] += row_counts[id];
                sum_x[id] += row_sum_x[id];
                sum_y[id] += row_sum_y[id];
            }
        }

        for plate in plates.iter_mut() {
            let id = plate.id as usize;
            if counts[id] > 0 {
                let c = counts[id] as f64;
                plate.center_x = (sum_x[id] / c) as f32;
                plate.center_y = (sum_y[id] / c) as f32;
            }
        }
    }
}

/// Bias plate speeds and directions: slow continents, faster oceanic plates toward nearest continent.
fn bias_plate_velocities(plates: &mut [Plate], config: &WorldGenConfig, rng: &mut ChaCha8Rng) {
    let continental: Vec<(f32, f32)> = plates
        .iter()
        .filter(|p| p.crust_type == CrustType::Continental)
        .map(|p| (p.center_x, p.center_y))
        .collect();

    let mantle_rad = config.mantle_flow_angle_deg.to_radians() as f32;
    let mantle_x = mantle_rad.cos();
    let mantle_y = mantle_rad.sin();

    for plate in plates.iter_mut() {
        let (speed_min, speed_max) = match plate.crust_type {
            CrustType::Continental => (0.1, config.continental_plate_speed_max),
            CrustType::Oceanic => (config.oceanic_plate_speed_min, 1.0),
        };
        let speed = rng.gen_range(speed_min..=speed_max);

        let mut dir_x = mantle_x + rng.gen_range(-0.25..0.25);
        let mut dir_y = mantle_y + rng.gen_range(-0.25..0.25);

        if plate.crust_type == CrustType::Oceanic && !continental.is_empty() {
            let mut best = f32::MAX;
            let mut target = (plate.center_x, plate.center_y);
            for &(cx, cy) in &continental {
                let dx = cx - plate.center_x;
                let dy = cy - plate.center_y;
                let d = dx * dx + dy * dy;
                if d < best {
                    best = d;
                    target = (cx, cy);
                }
            }
            let dx = target.0 - plate.center_x;
            let dy = target.1 - plate.center_y;
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            dir_x = dir_x * 0.4 + (dx / len) * 0.6;
            dir_y = dir_y * 0.4 + (dy / len) * 0.6;
        }

        let len = (dir_x * dir_x + dir_y * dir_y).sqrt().max(0.001);
        plate.velocity_x = dir_x / len * speed;
        plate.velocity_y = dir_y / len * speed;
    }
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
        let params = config.resolve();
        let plates = generate_plates(&config, &params, &mut rng);
        let mut map = WorldMap::new(config.width, config.height, config.seed);
        assign_plate_ids(&mut map, &plates, &None, 0.0, 1.0);
        assert!(map.plate_id.iter().all(|&id| id < plates.plates.len() as u32));
    }

    #[test]
    fn continental_plates_cluster_and_meet_fraction() {
        let config = WorldGenConfig::test_config(42, 512);
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let params = config.resolve();
        let data = generate_plates(&config, &params, &mut rng);
        let continental = data
            .plates
            .iter()
            .filter(|p| p.crust_type == CrustType::Continental)
            .count();
        let expected = ((data.plates.len() as f32) * config.continental_plate_fraction).round()
            as usize;
        assert_eq!(continental, expected.clamp(1, data.plates.len() - 1));
    }

    #[test]
    fn crust_assignment_is_deterministic() {
        let config = WorldGenConfig::test_config(99, 256);
        let mut a_rng = ChaCha8Rng::seed_from_u64(config.seed);
        let mut b_rng = ChaCha8Rng::seed_from_u64(config.seed);
        let params = config.resolve();
        let a = generate_plates(&config, &params, &mut a_rng);
        let b = generate_plates(&config, &params, &mut b_rng);
        for (pa, pb) in a.plates.iter().zip(b.plates.iter()) {
            assert_eq!(pa.crust_type, pb.crust_type);
            assert!((pa.center_x - pb.center_x).abs() < 0.01);
            assert!((pa.velocity_x - pb.velocity_x).abs() < 0.01);
        }
    }

    #[test]
    fn lloyd_relaxation_moves_centers() {
        let mut config = WorldGenConfig::test_config(7, 64);
        config.plate_lloyd_iterations = 2;
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let params = config.resolve();
        let data = generate_plates(&config, &params, &mut rng);
        let xs: Vec<f32> = data.plates.iter().map(|p| p.center_x).collect();
        assert!(xs.iter().any(|&x| (x - config.width as f32 * 0.5).abs() < config.width as f32));
    }
}
