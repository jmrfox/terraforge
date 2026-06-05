use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::WorldGenConfig;
use super::land_mask;
use super::plates::{Plate, PlateData};
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

const LAND_UPLIFT: f32 = 0.05;
const LAND_COMPRESSION: f32 = 0.92;
const OCEAN_FLOOR: f32 = 0.08;

fn normalize01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 >= edge1 {
        return if x >= edge0 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct TerrainNoise {
    continent: Fbm<Perlin>,
    mountains: Fbm<Perlin>,
    hills: Fbm<Perlin>,
}

impl TerrainNoise {
    fn new(config: &WorldGenConfig) -> Self {
        let seed = config.seed as u32;
        Self {
            continent: Fbm::<Perlin>::new(seed)
                .set_octaves(4)
                .set_frequency(config.continent_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            mountains: Fbm::<Perlin>::new(seed.wrapping_add(1))
                .set_octaves(3)
                .set_frequency(config.mountain_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            hills: Fbm::<Perlin>::new(seed.wrapping_add(2))
                .set_octaves(2)
                .set_frequency(config.hill_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
        }
    }

    fn sample_detail(&self, w: f32, h: f32, x: usize, y: usize) -> f32 {
        let wx = x as f32 / w * w;
        let hy = y as f32 / h * h;

        let continent = self.continent.get([wx as f64, hy as f64]) as f32;
        let mountains = self.mountains.get([wx as f64, hy as f64]) as f32;
        let hills = self.hills.get([wx as f64, hy as f64]) as f32;

        let continent01 = (continent + 1.0) * 0.5;
        let mountains01 = ((mountains + 1.0) * 0.5).powi(2);
        let hills01 = (hills + 1.0) * 0.5;

        normalize01(continent01 * 0.55 + hills01 * 0.28 + mountains01 * 0.12)
    }
}

fn box_blur(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return src.to_vec();
    }
    let r = radius as i32;
    let mut tmp = vec![0.0f32; src.len()];
    let mut out = vec![0.0f32; src.len()];

    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for dx in -r..=r {
                let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                sum += src[y * w + nx];
                count += 1;
            }
            tmp[y * w + x] = sum / count as f32;
        }
    }

    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for dy in -r..=r {
                let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                sum += tmp[ny * w + x];
                count += 1;
            }
            out[y * w + x] = sum / count as f32;
        }
    }
    out
}

/// Per-cell plate boundary uplift (each cell accumulates only its own contribution).
fn compute_boundary_influence(
    map: &WorldMap,
    plates: &PlateData,
    config: &WorldGenConfig,
) -> Vec<f32> {
    let w = map.width;
    let h = map.height;
    let plate_by_id: Vec<&Plate> = {
        let mut v = vec![None; plates.plates.len()];
        for p in &plates.plates {
            v[p.id as usize] = Some(p);
        }
        v.into_iter().map(|p| p.unwrap()).collect()
    };

    let plate_id = map.plate_id.as_slice();
    let mut influence = vec![0.0f32; w * h];
    let strength = config.plate_boundary_strength;

    influence
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                let my_plate = plate_id[idx] as usize;
                let my = plate_by_id[my_plate];
                let mut total = 0.0f32;

                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let nidx = ny as usize * w + nx as usize;
                    let neighbor_plate = plate_id[nidx] as usize;
                    if neighbor_plate == my_plate {
                        continue;
                    }
                    let neighbor = plate_by_id[neighbor_plate];

                    let bx = neighbor.center_x - my.center_x;
                    let by = neighbor.center_y - my.center_y;
                    let len_b = (bx * bx + by * by).sqrt().max(0.001);
                    let ux = bx / len_b;
                    let uy = by / len_b;

                    let my_toward = my.velocity_x * ux + my.velocity_y * uy;
                    let neighbor_toward =
                        neighbor.velocity_x * (-ux) + neighbor.velocity_y * (-uy);

                    if my_toward > 0.0 && neighbor_toward > 0.0 {
                        total += strength;
                    } else if my_toward < 0.0 && neighbor_toward < 0.0 {
                        total -= strength * 0.6;
                    } else {
                        total += strength * 0.1;
                    }
                }

                *cell = total;
            }
        });

    let spread = config.mountain_spread_radius as usize;
    let blurred = box_blur(&influence, w, h, spread);
    let weight = config.mountain_boundary_weight;
    blurred
        .into_iter()
        .map(|v| v * weight)
        .collect()
}

pub fn generate_elevation(
    map: &mut WorldMap,
    plates: &PlateData,
    config: &WorldGenConfig,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;
    let land_mask = land_mask::generate(config);
    let boundary = compute_boundary_influence(map, plates, config);
    let noise = TerrainNoise::new(config);

    let mut raw = vec![0.0f32; w * h];
    let (min_v, max_v) = raw
        .par_chunks_mut(w)
        .enumerate()
        .map(|(y, row)| {
            if y % 4 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / h as f32,
                    "Building elevation",
                );
            }
            let mut row_min = f32::MAX;
            let mut row_max = f32::MIN;
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                let mask = smoothstep(0.30, 0.50, land_mask[idx]);
                let detail = noise.sample_detail(w as f32, h as f32, x, y);
                let land_body = normalize01(detail + boundary[idx]) * 0.88 + 0.22;
                let v = land_body * mask + (1.0 - mask) * OCEAN_FLOOR;
                *cell = v;
                row_min = row_min.min(v);
                row_max = row_max.max(v);
            }
            (row_min, row_max)
        })
        .reduce(
            || (f32::MAX, f32::MIN),
            |(a_min, a_max), (b_min, b_max)| (a_min.min(b_min), a_max.max(b_max)),
        );

    let range = (max_v - min_v).max(0.0001);
    raw.par_iter_mut().for_each(|v| {
        *v = (*v - min_v) / range;
        *v = (*v * LAND_COMPRESSION + LAND_UPLIFT).clamp(0.0, 1.0);
    });

    map.elevation.clone_from(&raw);
}
