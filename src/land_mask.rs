use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use rayon::prelude::*;

use super::config::{LandMaskMethod, ResolvedSimParams, WorldGenConfig};
use super::grid_ops::box_blur;
use super::plates::{CrustType, PlateData};
use super::world::WorldMap;

const MACRO_COAST_LOW: f32 = 0.15;
const MACRO_COAST_HIGH: f32 = 0.85;

/// Config and dimensions for land-shape generators at a fixed physical feature scale.
fn land_shape_grid(
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
) -> (WorldGenConfig, usize, usize) {
    let factor = params.land_shape_factor.max(1) as usize;
    let sw = (config.width / factor).max(4);
    let sh = (config.height / factor).max(4);
    let mut shape = config.clone();
    shape.width = sw;
    shape.height = sh;
    (shape, sw, sh)
}

fn upscale_land_shape(
    mask: &[f32],
    sw: usize,
    sh: usize,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
) -> Vec<f32> {
    if sw == config.width && sh == config.height {
        return mask.to_vec();
    }
    upscale_nearest(
        mask,
        sw,
        sh,
        config.width,
        config.height,
        params.land_mask_blur_cells.max(1) as usize,
    )
}

fn hybrid_detail_raw(config: &WorldGenConfig, params: &ResolvedSimParams) -> Vec<f32> {
    let ca = generate_ca_raw(config, params);
    let noise = generate_noise_raw(config, params);
    let blend = config.hybrid_noise_blend.clamp(0.0, 1.0);
    ca.iter()
        .zip(noise.iter())
        .map(|(&c, &n)| (c * (1.0 - blend) + n * blend).clamp(0.0, 1.0))
        .collect()
}

/// Raw texture mask for the land-texture overlay pass (no finalize cleanup).
pub fn generate_texture(
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    _map: Option<&WorldMap>,
    _plates: Option<&PlateData>,
) -> Vec<f32> {
    let len = config.width * config.height;
    let (shape_cfg, sw, sh) = land_shape_grid(config, params);
    let mask = match config.land_mask_method {
        LandMaskMethod::Noise => upscale_land_shape(
            &generate_noise_raw(&shape_cfg, params),
            sw,
            sh,
            config,
            params,
        ),
        LandMaskMethod::CellularAutomata => upscale_land_shape(
            &generate_ca_raw(&shape_cfg, params),
            sw,
            sh,
            config,
            params,
        ),
        LandMaskMethod::DrunkardsWalk => upscale_land_shape(
            &generate_drunkard(&shape_cfg, params),
            sw,
            sh,
            config,
            params,
        ),
        LandMaskMethod::Hybrid => upscale_land_shape(
            &hybrid_detail_raw(&shape_cfg, params),
            sw,
            sh,
            config,
            params,
        ),
    };
    debug_assert_eq!(mask.len(), len);
    mask
}

/// Generate a macro land influence mask in `[0, 1]` (1 = land, 0 = open ocean).
pub fn generate(
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    map: Option<&WorldMap>,
    plates: Option<&PlateData>,
) -> Vec<f32> {
    let len = config.width * config.height;
    let (shape_cfg, sw, sh) = land_shape_grid(config, params);
    let mask = match config.land_mask_method {
        LandMaskMethod::Noise => finalize_land_mask(
            upscale_land_shape(&generate_noise_raw(&shape_cfg, params), sw, sh, config, params),
            config,
            params,
        ),
        LandMaskMethod::CellularAutomata => finalize_land_mask(
            upscale_land_shape(&generate_ca_raw(&shape_cfg, params), sw, sh, config, params),
            config,
            params,
        ),
        LandMaskMethod::DrunkardsWalk => upscale_land_shape(
            &generate_drunkard(&shape_cfg, params),
            sw,
            sh,
            config,
            params,
        ),
        LandMaskMethod::Hybrid => {
            if config.use_plate_macro_mask {
                if let (Some(map), Some(plates)) = (map, plates) {
                    generate_hybrid_with_crust(map, plates, config, params, &shape_cfg, sw, sh)
                } else {
                    finalize_land_mask(
                        upscale_land_shape(
                            &hybrid_detail_raw(&shape_cfg, params),
                            sw,
                            sh,
                            config,
                            params,
                        ),
                        config,
                        params,
                    )
                }
            } else {
                finalize_land_mask(
                    upscale_land_shape(
                        &hybrid_detail_raw(&shape_cfg, params),
                        sw,
                        sh,
                        config,
                        params,
                    ),
                    config,
                    params,
                )
            }
        }
    };
    debug_assert_eq!(mask.len(), len);
    mask
}

fn land_threshold() -> f32 {
    0.45
}

fn is_land(mask: &[f32], idx: usize) -> bool {
    mask[idx] >= land_threshold()
}

/// Macro continent mask from continental crust plate cells, blurred at margins.
pub fn crust_macro_mask(
    map: &WorldMap,
    plates: &PlateData,
    params: &ResolvedSimParams,
) -> Vec<f32> {
    let w = map.width;
    let h = map.height;
    let len = w * h;
    let mut crust_by_id = vec![CrustType::Oceanic; plates.plates.len()];
    for plate in &plates.plates {
        crust_by_id[plate.id as usize] = plate.crust_type;
    }

    let mut mask = vec![0.0f32; len];
    for (idx, &plate_id) in map.plate_id.iter().enumerate() {
        if crust_by_id[plate_id as usize] == CrustType::Continental {
            mask[idx] = 1.0;
        }
    }

    box_blur(
        &mask,
        w,
        h,
        params.continental_blur_radius_cells.max(1) as usize,
    )
}

fn combine_macro_and_coast_detail(macro_mask: &[f32], detail: &[f32]) -> Vec<f32> {
    macro_mask
        .iter()
        .zip(detail.iter())
        .map(|(&m, &d)| {
            if m >= MACRO_COAST_HIGH || m <= MACRO_COAST_LOW {
                m
            } else {
                (m * (0.55 + 0.45 * d)).clamp(0.0, 1.0)
            }
        })
        .collect()
}

/// Remove tiny land specks so CA/hybrid produce fewer, larger landmasses.
fn cull_small_land_patches(mask: &mut [f32], w: usize, h: usize, min_cells: usize) {
    if min_cells <= 1 {
        return;
    }

    let mut land = vec![false; mask.len()];
    for (idx, cell) in land.iter_mut().enumerate() {
        *cell = is_land(mask, idx);
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
        *cell = is_land(mask, idx);
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

/// Erosion pass — removes land cells with at least one water neighbor.
fn erode_land_mask(mask: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 {
        return;
    }

    for _ in 0..radius {
        let src: Vec<f32> = mask.to_vec();
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                if !is_land(&src, idx) {
                    continue;
                }
                let mut touches_water = false;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        touches_water = true;
                        break;
                    }
                    if !is_land(&src, ny as usize * w + nx as usize) {
                        touches_water = true;
                        break;
                    }
                }
                if touches_water {
                    mask[idx] = 0.0;
                }
            }
        }
    }
}

fn close_land_mask(mask: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 {
        return;
    }

    let mut land = vec![false; mask.len()];
    for (idx, cell) in land.iter_mut().enumerate() {
        *cell = is_land(mask, idx);
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

/// Morphological open — erosion then dilation — to break narrow isthmuses.
fn open_land_mask(mask: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 {
        return;
    }
    erode_land_mask(mask, w, h, radius);
    close_land_mask(mask, w, h, radius);
}

fn component_perimeter(component: &[usize], land: &[bool], w: usize, h: usize) -> usize {
    let mut perimeter = 0usize;
    for &idx in component {
        let x = idx % w;
        let y = idx / w;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                perimeter += 1;
                continue;
            }
            if !land[ny as usize * w + nx as usize] {
                perimeter += 1;
            }
        }
    }
    perimeter
}

/// Remove land components that are too elongated (high perimeter²/area).
fn cull_non_compact_components(mask: &mut [f32], w: usize, h: usize, max_compactness: f32) {
    if max_compactness <= 0.0 {
        return;
    }

    let mut land = vec![false; mask.len()];
    for (idx, cell) in land.iter_mut().enumerate() {
        *cell = is_land(mask, idx);
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

        let area = component.len().max(1);
        let perimeter = component_perimeter(&component, &land, w, h).max(1);
        let score = (perimeter * perimeter) as f32 / area as f32;
        if score > max_compactness {
            for idx in component {
                mask[idx] = 0.0;
            }
        }
    }
}

fn finalize_land_mask(
    mut mask: Vec<f32>,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
) -> Vec<f32> {
    let w = config.width;
    let h = config.height;
    let open_radius = (params.min_isthmus_width_cells / 2).max(1) as usize;
    open_land_mask(&mut mask, w, h, open_radius);
    close_land_mask(
        &mut mask,
        w,
        h,
        params.land_mask_close_cells.max(1) as usize,
    );
    cull_non_compact_components(&mut mask, w, h, config.max_landmass_compactness);
    cull_small_land_patches(&mut mask, w, h, params.min_landmass_cells);
    keep_largest_land_components(&mut mask, w, h, params.max_landmasses);
    box_blur(
        &mask,
        w,
        h,
        params.land_mask_blur_cells.max(1) as usize,
    )
}

fn generate_noise_raw(config: &WorldGenConfig, params: &ResolvedSimParams) -> Vec<f32> {
    let w = config.width;
    let h = config.height;
    let fbm = Fbm::<Perlin>::new(config.seed as u32 + 3)
        .set_octaves(3)
        .set_frequency(1.0)
        .set_lacunarity(2.0)
        .set_persistence(0.5);

    let mut mask = vec![0.0f32; w * h];
    let freq = params.land_mask_noise_frequency;
    mask.par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let nx = x as f64 / w as f64;
                let ny = y as f64 / h as f64;
                let n = fbm.get([nx * freq, ny * freq]) as f32;
                *cell = ((n + 1.0) * 0.5).powf(1.05);
            }
        });
    mask
}

fn generate_ca_raw(config: &WorldGenConfig, params: &ResolvedSimParams) -> Vec<f32> {
    let factor = params.ca_coarse_factor.max(1) as usize;
    let w = (config.width / factor).max(4);
    let h = (config.height / factor).max(4);
    let coarse = generate_ca_grid(config, w, h);
    if factor == 1 && w == config.width && h == config.height {
        return coarse;
    }
    upscale_nearest(
        &coarse,
        w,
        h,
        config.width,
        config.height,
        params.land_mask_blur_cells.max(1) as usize,
    )
}

fn generate_ca_grid(config: &WorldGenConfig, w: usize, h: usize) -> Vec<f32> {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed.wrapping_add(20));
    const REF_COARSE_DIM: f64 = 128.0;
    let coarse_scale = ((w * h) as f64 / (REF_COARSE_DIM * REF_COARSE_DIM)).sqrt().max(1.0);
    let iterations = (config.ca_iterations as f64 * coarse_scale)
        .round()
        .clamp(1.0, 64.0) as u32;
    let smoothing = (config.ca_smoothing_passes as f64 * coarse_scale.sqrt())
        .round()
        .clamp(0.0, 32.0) as u32;

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

    for _ in 0..iterations {
        grid = ca_step(&grid, w, h);
    }
    for _ in 0..smoothing {
        grid = smooth_step(&grid, w, h);
    }

    bool_to_mask(&grid, w, h)
}

fn upscale_nearest(
    src: &[f32],
    sw: usize,
    sh: usize,
    dw: usize,
    dh: usize,
    blur_radius: usize,
) -> Vec<f32> {
    let mut out = vec![0.0f32; dw * dh];
    for y in 0..dh {
        for x in 0..dw {
            let sx = x * sw / dw;
            let sy = y * sh / dh;
            out[y * dw + x] = src[sy * sw + sx];
        }
    }
    box_blur(&out, dw, dh, blur_radius.max(1))
}

fn generate_drunkard(config: &WorldGenConfig, params: &ResolvedSimParams) -> Vec<f32> {
    let w = config.width;
    let h = config.height;
    let len = w * h;
    let mut accum = vec![0.0f32; len];
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed.wrapping_add(30));
    let steps = params.drunkard_steps_per_walker;
    let radius = params.drunkard_brush_radius_cells.max(1) as i32;
    let blur = params.land_mask_blur_cells.max(1) as usize;

    for _ in 0..params.drunkard_walkers.max(1) {
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
    mask = box_blur(&mask, w, h, blur);
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

fn generate_hybrid_with_crust(
    map: &WorldMap,
    plates: &PlateData,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    shape_cfg: &WorldGenConfig,
    sw: usize,
    sh: usize,
) -> Vec<f32> {
    let macro_mask = crust_macro_mask(map, plates, params);
    let detail = upscale_land_shape(
        &hybrid_detail_raw(shape_cfg, params),
        sw,
        sh,
        config,
        params,
    );
    let combined = combine_macro_and_coast_detail(&macro_mask, &detail);
    finalize_land_mask(combined, config, params)
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LandMaskMethod;
    use crate::plates::{assign_plate_ids, generate_plates};
    use rand_chacha::rand_core::SeedableRng;

    fn mask_for_method(config: &WorldGenConfig, method: LandMaskMethod) -> Vec<f32> {
        let mut cfg = config.clone();
        cfg.land_mask_method = method;
        if method == LandMaskMethod::Hybrid && cfg.use_plate_macro_mask {
            let mut rng = ChaCha8Rng::seed_from_u64(cfg.seed);
            let params = cfg.resolve();
            let plates = generate_plates(&cfg, &params, &mut rng);
            let mut map = WorldMap::new(cfg.width, cfg.height, cfg.seed);
            assign_plate_ids(&mut map, &plates, &None, 0.0, 1.0);
            let params = cfg.resolve();
            generate(&cfg, &params, Some(&map), Some(&plates))
        } else {
            let params = cfg.resolve();
            generate(&cfg, &params, None, None)
        }
    }

    #[test]
    fn land_mask_determinism() {
        for method in [
            LandMaskMethod::Noise,
            LandMaskMethod::CellularAutomata,
            LandMaskMethod::DrunkardsWalk,
            LandMaskMethod::Hybrid,
        ] {
            let a = WorldGenConfig::test_config(99, 64);
            let b = a.clone();
            assert_eq!(
                mask_for_method(&a, method),
                mask_for_method(&b, method)
            );
        }
    }

    #[test]
    fn land_mask_correct_length() {
        let config = WorldGenConfig::test_config(1, 32);
        let params = config.resolve();
        assert_eq!(generate(&config, &params, None, None).len(), 32 * 32);
    }

    #[test]
    fn drunkard_mask_has_land_influence() {
        let mut config = WorldGenConfig::test_config(42, 64);
        config.land_mask_method = LandMaskMethod::DrunkardsWalk;
        let params = config.resolve();
        let mask = generate(&config, &params, None, None);
        let mut values: Vec<f32> = mask.to_vec();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = values[values.len() / 2];
        assert!(
            median > 0.12,
            "drunkard mask median {median} too low for land influence"
        );
    }

    #[test]
    fn land_mask_shape_scales_with_cell_size() {
        let mut fine = WorldGenConfig::test_config(77, 128);
        fine.cell_size_m = crate::units::Meters(50.0);
        fine.land_shape_cell_size_m = crate::units::Meters(50.0);
        fine.land_mask_method = LandMaskMethod::DrunkardsWalk;
        fine.use_plate_macro_mask = false;

        let mut coarse = fine.clone();
        coarse.cell_size_m = crate::units::Meters(100.0);

        assert_eq!(fine.resolve().land_shape_factor, 1);
        assert_eq!(coarse.resolve().land_shape_factor, 2);

        let fine_params = fine.resolve();
        let coarse_params = coarse.resolve();
        assert!(coarse_params.drunkard_walkers > fine_params.drunkard_walkers);

        let fine_mask = generate_texture(&fine, &fine_params, None, None);
        let coarse_mask = generate_texture(&coarse, &coarse_params, None, None);
        assert_ne!(
            fine_mask, coarse_mask,
            "texture masks should differ when cell size doubles at fixed shape scale"
        );
    }

    #[test]
    fn crust_hybrid_differs_from_legacy_hybrid() {
        let mut config = WorldGenConfig::test_config(42, 128);
        config.land_mask_method = LandMaskMethod::Hybrid;
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let params = config.resolve();
        let plates = generate_plates(&config, &params, &mut rng);
        let mut map = WorldMap::new(config.width, config.height, config.seed);
        assign_plate_ids(&mut map, &plates, &None, 0.0, 1.0);

        config.use_plate_macro_mask = true;
        let params = config.resolve();
        let crust = generate(&config, &params, Some(&map), Some(&plates));
        config.use_plate_macro_mask = false;
        let legacy = generate(&config, &params, None, None);
        assert_ne!(crust, legacy, "crust macro hybrid should differ from legacy");
    }
}
