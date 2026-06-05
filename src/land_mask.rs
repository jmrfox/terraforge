use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

use super::config::{LandMaskMethod, WorldGenConfig};

const MACRO_CONTINENT_SCALE: f64 = 2.4;
const LAND_MASK_BLUR_RADIUS: usize = 2;

/// Generate a macro land influence mask in `[0, 1]` (1 = land, 0 = open ocean).
pub fn generate(config: &WorldGenConfig) -> Vec<f32> {
    let len = config.width * config.height;
    let mask = match config.land_mask_method {
        LandMaskMethod::Noise => generate_noise_macro(config),
        LandMaskMethod::CellularAutomata => generate_ca(config),
        LandMaskMethod::DrunkardsWalk => generate_drunkard(config),
        LandMaskMethod::Hybrid => generate_hybrid(config),
    };
    debug_assert_eq!(mask.len(), len);
    mask
}

fn generate_noise_macro(config: &WorldGenConfig) -> Vec<f32> {
    let w = config.width;
    let h = config.height;
    let fbm = Fbm::<Perlin>::new(config.seed as u32 + 3)
        .set_octaves(3)
        .set_frequency(1.0)
        .set_lacunarity(2.0)
        .set_persistence(0.5);

    let mut mask = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let nx = x as f64 / w as f64;
            let ny = y as f64 / h as f64;
            let n = fbm.get([nx * MACRO_CONTINENT_SCALE, ny * MACRO_CONTINENT_SCALE]) as f32;
            mask[y * w + x] = ((n + 1.0) * 0.5).powf(1.15);
        }
    }
    box_blur(&mask, w, h, LAND_MASK_BLUR_RADIUS)
}

fn generate_ca(config: &WorldGenConfig) -> Vec<f32> {
    let w = config.width;
    let h = config.height;
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed.wrapping_add(20));

    let mut grid = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            grid[y * w + x] = if edge {
                false
            } else {
                rng.gen::<f32>() < config.ca_fill_probability
            };
        }
    }

    for _ in 0..config.ca_iterations {
        grid = ca_step(&grid, w, h);
    }
    for _ in 0..config.ca_smoothing_passes {
        grid = smooth_step(&grid, w, h);
    }

    bool_to_mask(&grid, w, h)
}

fn generate_drunkard(config: &WorldGenConfig) -> Vec<f32> {
    let w = config.width;
    let h = config.height;
    let len = w * h;
    let mut accum = vec![0.0f32; len];
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed.wrapping_add(30));
    let steps = config.drunkard_steps_for_map();
    let radius = config.drunkard_brush_radius.max(1) as i32;

    for _ in 0..config.drunkard_walkers.max(1) {
        let mut x = rng.gen_range(1..w.saturating_sub(1).max(2)) as i32;
        let mut y = rng.gen_range(1..h.saturating_sub(1).max(2)) as i32;
        for _ in 0..steps {
            stamp_disk(&mut accum, w, h, x, y, radius, 1.0);
            match rng.gen_range(0..4) {
                0 => x += 1,
                1 => x -= 1,
                2 => y += 1,
                _ => y -= 1,
            }
            x = x.clamp(1, w as i32 - 2);
            y = y.clamp(1, h as i32 - 2);
        }
    }

    let max_v = accum.iter().cloned().fold(0.0f32, f32::max).max(0.001);
    let mut mask: Vec<f32> = accum.iter().map(|v| (v / max_v).clamp(0.0, 1.0)).collect();
    mask = box_blur(&mask, w, h, LAND_MASK_BLUR_RADIUS);
    for y in 0..h {
        for x in 0..w {
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                mask[y * w + x] = 0.0;
            }
        }
    }
    mask
}

fn generate_hybrid(config: &WorldGenConfig) -> Vec<f32> {
    let ca = generate_ca(config);
    let noise = generate_noise_macro(config);
    let blend = config.hybrid_noise_blend.clamp(0.0, 1.0);
    ca.iter()
        .zip(noise.iter())
        .map(|(&c, &n)| (c * (1.0 - blend) + n * blend).clamp(0.0, 1.0))
        .collect()
}

fn ca_step(grid: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut next = vec![false; grid.len()];
    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let neighbors = count_land_neighbors(grid, w, h, x, y);
            next[idx] = if grid[idx] {
                neighbors >= 4
            } else {
                neighbors >= 5
            };
        }
    }
    next
}

fn smooth_step(grid: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut next = vec![false; grid.len()];
    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let neighbors = count_land_neighbors(grid, w, h, x, y);
            let total = neighbors + if grid[idx] { 1 } else { 0 };
            next[idx] = total >= 5;
        }
    }
    next
}

fn count_land_neighbors(grid: &[bool], w: usize, h: usize, x: usize, y: usize) -> u32 {
    let mut count = 0u32;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            if grid[ny as usize * w + nx as usize] {
                count += 1;
            }
        }
    }
    count
}

fn bool_to_mask(grid: &[bool], w: usize, h: usize) -> Vec<f32> {
    let mask: Vec<f32> = grid.iter().map(|&c| if c { 1.0 } else { 0.0 }).collect();
    box_blur(&mask, w, h, LAND_MASK_BLUR_RADIUS)
}

fn stamp_disk(grid: &mut [f32], w: usize, h: usize, cx: i32, cy: i32, radius: i32, value: f32) {
    let r2 = (radius * radius) as f32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if (dx * dx + dy * dy) as f32 > r2 {
                continue;
            }
            let x = cx + dx;
            let y = cy + dy;
            if x <= 0 || y <= 0 || x >= w as i32 - 1 || y >= h as i32 - 1 {
                continue;
            }
            let idx = y as usize * w + x as usize;
            grid[idx] += value;
        }
    }
}

fn box_blur(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0.0f32; src.len()];
    let mut out = vec![0.0f32; src.len()];
    let r = radius as i32;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LandMaskMethod;

    #[test]
    fn land_mask_determinism() {
        for method in [
            LandMaskMethod::Noise,
            LandMaskMethod::CellularAutomata,
            LandMaskMethod::DrunkardsWalk,
            LandMaskMethod::Hybrid,
        ] {
            let mut a = WorldGenConfig::test_config(99, 64);
            a.land_mask_method = method;
            let b = a.clone();
            assert_eq!(generate(&a), generate(&b));
        }
    }

    #[test]
    fn land_mask_correct_length() {
        let config = WorldGenConfig::test_config(1, 32);
        assert_eq!(generate(&config).len(), 32 * 32);
    }
}
