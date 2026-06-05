use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

use super::config::{LandMaskMethod, WorldGenConfig};

const LAND_MASK_BLUR_RADIUS: usize = 4;

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

fn min_land_component_cells(config: &WorldGenConfig) -> usize {
    let area = config.width * config.height;
    if area < 256 * 256 {
        return 0;
    }
    (area / 750).clamp(32, 8192)
}

fn max_land_masses(config: &WorldGenConfig) -> usize {
    let area = config.width * config.height;
    if area < 128 * 128 {
        return usize::MAX;
    }
    if area < 256 * 256 {
        return 6;
    }
    6
}

fn effective_coarse_factor(config: &WorldGenConfig) -> usize {
    let requested = config.ca_coarse_factor.clamp(1, 8) as usize;
    let min_dim = config.width.min(config.height);
    let max_factor = (min_dim / 32).max(1);
    requested.min(max_factor)
}

/// Remove tiny land specks so CA/hybrid produce fewer, larger landmasses.
fn cull_small_land_patches(mask: &mut [f32], w: usize, h: usize, min_cells: usize) {
    if min_cells <= 1 {
        return;
    }

    let mut land = vec![false; mask.len()];
    for (idx, cell) in land.iter_mut().enumerate() {
        *cell = mask[idx] >= 0.45;
    }

    let mut visited = vec![false; mask.len()];
    let mut stack = Vec::new();

    for start in 0..mask.len() {
        if !land[start] || visited[start] {
            continue;
        }

        stack.push(start);
        visited[start] = true;
        let mut component = Vec::new();

        while let Some(idx) = stack.pop() {
            component.push(idx);
            let x = idx % w;
            let y = idx / w;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let nidx = ny as usize * w + nx as usize;
                if land[nidx] && !visited[nidx] {
                    visited[nidx] = true;
                    stack.push(nidx);
                }
            }
        }

        if component.len() < min_cells {
            for idx in component {
                mask[idx] = 0.0;
            }
        }
    }
}

/// Keep only the largest N land components; zero out the rest.
fn keep_largest_land_components(mask: &mut [f32], w: usize, h: usize, keep: usize) {
    if keep == 0 {
        mask.fill(0.0);
        return;
    }

    let mut land = vec![false; mask.len()];
    for (idx, cell) in land.iter_mut().enumerate() {
        *cell = mask[idx] >= 0.45;
    }

    let mut visited = vec![false; mask.len()];
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut stack = Vec::new();

    for start in 0..mask.len() {
        if !land[start] || visited[start] {
            continue;
        }

        stack.push(start);
        visited[start] = true;
        let mut component = Vec::new();

        while let Some(idx) = stack.pop() {
            component.push(idx);
            let x = idx % w;
            let y = idx / w;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let nidx = ny as usize * w + nx as usize;
                if land[nidx] && !visited[nidx] {
                    visited[nidx] = true;
                    stack.push(nidx);
                }
            }
        }

        components.push(component);
    }

    components.sort_by(|a, b| b.len().cmp(&a.len()));
    let mut keep_set = vec![false; mask.len()];
    for component in components.into_iter().take(keep) {
        for idx in component {
            keep_set[idx] = true;
        }
    }

    for idx in 0..mask.len() {
        if !keep_set[idx] {
            mask[idx] = 0.0;
        }
    }
}

fn close_land_mask(mask: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 {
        return;
    }

    let mut land = vec![false; mask.len()];
    for (idx, cell) in land.iter_mut().enumerate() {
        *cell = mask[idx] >= 0.45;
    }

    for _ in 0..radius {
        let src = land.clone();
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                if src[idx] {
                    land[idx] = true;
                    continue;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    if src[ny as usize * w + nx as usize] {
                        land[idx] = true;
                        break;
                    }
                }
            }
        }
    }

    for idx in 0..mask.len() {
        if !land[idx] {
            mask[idx] = 0.0;
        }
    }
}

fn finalize_land_mask(mut mask: Vec<f32>, config: &WorldGenConfig) -> Vec<f32> {
    let w = config.width;
    let h = config.height;
    close_land_mask(&mut mask, w, h, 1);
    cull_small_land_patches(&mut mask, w, h, min_land_component_cells(config));
    keep_largest_land_components(&mut mask, w, h, max_land_masses(config));
    box_blur(&mask, w, h, LAND_MASK_BLUR_RADIUS)
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
            let n = fbm.get([
                nx * config.land_mask_scale,
                ny * config.land_mask_scale,
            ]) as f32;
            mask[y * w + x] = ((n + 1.0) * 0.5).powf(1.05);
        }
    }
    finalize_land_mask(mask, config)
}

fn generate_ca(config: &WorldGenConfig) -> Vec<f32> {
    let factor = effective_coarse_factor(config);
    let w = (config.width / factor).max(4);
    let h = (config.height / factor).max(4);
    let coarse = generate_ca_grid(config, w, h);
    if factor == 1 && w == config.width && h == config.height {
        return finalize_land_mask(coarse, config);
    }
    finalize_land_mask(upscale_nearest(&coarse, w, h, config.width, config.height), config)
}

fn generate_ca_grid(config: &WorldGenConfig, w: usize, h: usize) -> Vec<f32> {
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

fn upscale_nearest(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; dw * dh];
    for y in 0..dh {
        for x in 0..dw {
            let sx = x * sw / dw;
            let sy = y * sh / dh;
            out[y * dw + x] = src[sy * sw + sx];
        }
    }
    box_blur(&out, dw, dh, LAND_MASK_BLUR_RADIUS)
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
    let scale = (max_v * 0.72).max(0.001);
    let mut mask: Vec<f32> = accum
        .iter()
        .map(|v| (v / scale).clamp(0.0, 1.0))
        .collect();
    mask = box_blur(&mask, w, h, LAND_MASK_BLUR_RADIUS);
    for v in &mut mask {
        *v = v.powf(0.48);
    }
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
    let mask: Vec<f32> = ca
        .iter()
        .zip(noise.iter())
        .map(|(&c, &n)| (c * (1.0 - blend) + n * blend).clamp(0.0, 1.0))
        .collect();
    finalize_land_mask(mask, config)
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

fn bool_to_mask(grid: &[bool], _w: usize, _h: usize) -> Vec<f32> {
    grid.iter().map(|&c| if c { 1.0 } else { 0.0 }).collect()
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

    #[test]
    fn drunkard_mask_has_land_influence() {
        let mut config = WorldGenConfig::test_config(42, 64);
        config.land_mask_method = LandMaskMethod::DrunkardsWalk;
        let mask = generate(&config);
        let mut values: Vec<f32> = mask.to_vec();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = values[values.len() / 2];
        assert!(
            median > 0.12,
            "drunkard mask median {median} too low for land influence"
        );
    }
}
