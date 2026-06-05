use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{LandMaskMethod, WorldGenConfig};
use super::land_mask;
use super::plates::{Plate, PlateData};
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

const LAND_UPLIFT: f32 = 0.04;
const LAND_COMPRESSION: f32 = 1.0;
const OCEAN_FLOOR: f32 = 0.06;
const TERRAIN_AMPLITUDE: f32 = 1.65;
const BOUNDARY_UPLIFT_SCALE: f32 = 2.0;

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
        let nx = x as f64 / f64::from(w as u32);
        let ny = y as f64 / f64::from(h as u32);

        let continent = self.continent.get([nx, ny]) as f32;
        let mountains = self.mountains.get([nx, ny]) as f32;
        let hills = self.hills.get([nx, ny]) as f32;

        let continent01 = (continent + 1.0) * 0.5;
        let mountains01 = ((mountains + 1.0) * 0.5).powi(2);
        let hills01 = (hills + 1.0) * 0.5;

        let detail =
            continent01 * 0.48 + hills01 * 0.32 + mountains01 * 0.22;
        normalize01((detail - 0.5) * TERRAIN_AMPLITUDE + 0.5)
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
    let drunkard_mask = config.land_mask_method == LandMaskMethod::DrunkardsWalk;
    let (mask_low, mask_high) = if drunkard_mask {
        (0.08, 0.24)
    } else {
        (0.30, 0.50)
    };

    let mut raw = vec![0.0f32; w * h];
    raw.par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 4 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    y as f32 / h as f32,
                    "Building elevation",
                );
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                let mask = smoothstep(mask_low, mask_high, land_mask[idx]);
                let detail = noise.sample_detail(w as f32, h as f32, x, y);
                let land_body =
                    normalize01(detail + boundary[idx] * BOUNDARY_UPLIFT_SCALE) * 0.94 + 0.12;
                let v = land_body * mask + (1.0 - mask) * OCEAN_FLOOR;
                *cell = v;
            }
        });

    normalize_land_elevation(&mut raw, &land_mask, w, h, mask_low, mask_high, config.sea_level);
    map.elevation.clone_from(&raw);
}

/// Spread elevation across the full land range without ocean depths compressing the scale.
fn normalize_land_elevation(
    raw: &mut [f32],
    land_mask: &[f32],
    w: usize,
    h: usize,
    mask_low: f32,
    mask_high: f32,
    _sea: f32,
) {
    let len = w * h;
    let mut land_min = f32::MAX;
    let mut land_max = f32::MIN;
    let mut land_count = 0usize;

    for idx in 0..len {
        let influence = smoothstep(mask_low, mask_high, land_mask[idx]);
        if influence < 0.15 {
            continue;
        }
        land_count += 1;
        land_min = land_min.min(raw[idx]);
        land_max = land_max.max(raw[idx]);
    }

    if land_count == 0 {
        return;
    }

    let span = (land_max - land_min).max(0.0001);
    let floor = OCEAN_FLOOR;
    let ceiling = 1.0;

    for idx in 0..len {
        let influence = smoothstep(mask_low, mask_high, land_mask[idx]);
        if influence < 0.15 {
            raw[idx] = floor;
            continue;
        }

        let t = ((raw[idx] - land_min) / span).clamp(0.0, 1.0);
        let stretched = t.powf(0.42);
        let elev = floor + stretched * (ceiling - floor);
        raw[idx] = (elev * LAND_COMPRESSION + LAND_UPLIFT).clamp(floor, ceiling);
    }
}
