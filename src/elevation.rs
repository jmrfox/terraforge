use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::{LandGenerationMode, LandMaskMethod, ResolvedSimParams, WorldGenConfig};
use super::grid_ops::{box_blur, chamfer_distance};
use super::land_mask::{self, crust_macro_mask};
use super::plates::{CrustType, Plate, PlateData};
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

const LAND_UPLIFT: f32 = 0.04;
const LAND_COMPRESSION: f32 = 1.0;
const TERRAIN_AMPLITUDE: f32 = 1.65;
const TECTONIC_HILL_AMPLITUDE: f32 = 0.06;
const BOUNDARY_UPLIFT_SCALE: f32 = 2.0;
const MACRO_WATER_THRESHOLD: f32 = 0.5;
const MACRO_LAND_THRESHOLD: f32 = 0.5;

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
    fn new(config: &WorldGenConfig, params: &ResolvedSimParams) -> Self {
        let seed = config.seed as u32;
        Self {
            continent: Fbm::<Perlin>::new(seed)
                .set_octaves(4)
                .set_frequency(params.continent_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            mountains: Fbm::<Perlin>::new(seed.wrapping_add(1))
                .set_octaves(3)
                .set_frequency(params.mountain_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            hills: Fbm::<Perlin>::new(seed.wrapping_add(2))
                .set_octaves(2)
                .set_frequency(params.hill_noise_frequency)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
        }
    }

    fn sample_detail(
        &self,
        w: f32,
        h: f32,
        x: usize,
        y: usize,
        orogeny: f32,
        orogeny_only: bool,
        orogeny_threshold: f32,
        tectonic_mode: bool,
    ) -> f32 {
        let nx = x as f64 / f64::from(w as u32);
        let ny = y as f64 / f64::from(h as u32);

        let continent = self.continent.get([nx, ny]) as f32;
        let mountains = self.mountains.get([nx, ny]) as f32;
        let hills = self.hills.get([nx, ny]) as f32;

        let continent01 = (continent + 1.0) * 0.5;
        let mountains01 = ((mountains + 1.0) * 0.5).powi(2);
        let hills01 = (hills + 1.0) * 0.5;

        if tectonic_mode {
            let hill_only = hills01 * 0.7 + continent01 * 0.3;
            return (hill_only - 0.5) * TECTONIC_HILL_AMPLITUDE;
        }

        let mountain_weight = if orogeny_only {
            if orogeny > orogeny_threshold {
                0.08
            } else {
                0.0
            }
        } else {
            0.22
        };
        let continent_weight = if orogeny_only { 0.55 } else { 0.48 };
        let hills_weight = if orogeny_only { 0.37 } else { 0.32 };

        let detail = continent01 * continent_weight
            + hills01 * hills_weight
            + mountains01 * mountain_weight;
        normalize01((detail - 0.5) * TERRAIN_AMPLITUDE + 0.5)
    }
}

fn distance_to_macro_water(macro_mask: &[f32], w: usize, h: usize) -> Vec<u32> {
    chamfer_distance(w, h, |idx| macro_mask[idx] < MACRO_WATER_THRESHOLD)
}

fn distance_to_macro_land(macro_mask: &[f32], w: usize, h: usize) -> Vec<u32> {
    chamfer_distance(w, h, |idx| macro_mask[idx] >= MACRO_LAND_THRESHOLD)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryMotion {
    Convergent,
    Divergent,
    Transform,
}

fn crust_boundary_scale(
    my_crust: CrustType,
    neighbor_crust: CrustType,
    motion: BoundaryMotion,
    oceanic_factor: f32,
) -> f32 {
    match motion {
        BoundaryMotion::Convergent => match (my_crust, neighbor_crust) {
            (CrustType::Continental, CrustType::Continental) => 1.0,
            (CrustType::Continental, CrustType::Oceanic) => 0.2,
            (CrustType::Oceanic, CrustType::Continental) => 0.0,
            (CrustType::Oceanic, CrustType::Oceanic) => oceanic_factor,
        },
        BoundaryMotion::Divergent => match (my_crust, neighbor_crust) {
            (CrustType::Continental, CrustType::Continental) => 0.8,
            (CrustType::Continental, CrustType::Oceanic) => 0.5,
            (CrustType::Oceanic, CrustType::Continental) => 0.5,
            (CrustType::Oceanic, CrustType::Oceanic) => 1.0,
        },
        BoundaryMotion::Transform => 0.15,
    }
}

fn normalize_orogeny(boundary: &[f32]) -> Vec<f32> {
    let max_pos = boundary
        .iter()
        .cloned()
        .fold(0.0f32, f32::max)
        .max(0.0001);
    boundary
        .iter()
        .map(|v| (v.max(0.0) / max_pos).clamp(0.0, 1.0))
        .collect()
}

fn compute_boundary_influence(
    map: &WorldMap,
    plates: &PlateData,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    macro_mask: &[f32],
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

                    let (motion, base) = if my_toward > 0.0 && neighbor_toward > 0.0 {
                        (BoundaryMotion::Convergent, strength)
                    } else if my_toward < 0.0 && neighbor_toward < 0.0 {
                        (BoundaryMotion::Divergent, -strength * 0.6)
                    } else {
                        (BoundaryMotion::Transform, strength * 0.1)
                    };

                    let mut scale = crust_boundary_scale(
                        my.crust_type,
                        neighbor.crust_type,
                        motion,
                        config.oceanic_uplift_factor,
                    );
                    if matches!(motion, BoundaryMotion::Convergent) {
                        let emergent_collision = macro_mask[idx] >= MACRO_LAND_THRESHOLD
                            && macro_mask[nidx] >= MACRO_LAND_THRESHOLD;
                        if !emergent_collision {
                            scale *= 0.0;
                        }
                    }
                    total += base * scale;
                }

                *cell = total;
            }
        });

    let spread = params.mountain_spread_radius_cells as usize;
    let blurred = box_blur(&influence, w, h, spread);
    let weight = config.mountain_boundary_weight;
    blurred
        .into_iter()
        .map(|v| v * weight)
        .collect()
}

fn build_tectonic_base(
    raw: &mut [f32],
    macro_mask: &[f32],
    dist_to_macro_water: &[u32],
    boundary: &[f32],
    noise: &TerrainNoise,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    w: usize,
    h: usize,
) {
    let interior_buffer = params.orogeny_interior_min_dist_cells as f32;
    let uplift_scale = config.tectonic_uplift_scale;
    let continental_base = params.continental_base_norm;
    let abyssal_base = params.abyssal_base_norm;
    let ocean_floor = params.ocean_floor_norm;
    let ceiling = params.max_elev_norm;
    let orogeny_only = config.mountain_noise_orogeny_only;
    let orogeny_threshold = config.orogeny_mountain_threshold;

    raw.par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                let macro_land = macro_mask[idx] >= MACRO_WATER_THRESHOLD;
                let interior_w = smoothstep(
                    0.0,
                    interior_buffer,
                    dist_to_macro_water[idx] as f32,
                );
                let gated_boundary = if macro_land {
                    boundary[idx] * interior_w
                } else {
                    boundary[idx].min(0.0)
                };

                let norm_orogeny = (gated_boundary.max(0.0) / BOUNDARY_UPLIFT_SCALE).clamp(0.0, 1.0);
                let hill_delta = if macro_land {
                    noise.sample_detail(
                        w as f32,
                        h as f32,
                        x,
                        y,
                        norm_orogeny,
                        orogeny_only,
                        orogeny_threshold,
                        true,
                    )
                } else {
                    0.0
                };

                let crust_base =
                    continental_base * macro_mask[idx] + abyssal_base * (1.0 - macro_mask[idx]);
                let uplift = gated_boundary.max(0.0) * BOUNDARY_UPLIFT_SCALE * uplift_scale;
                let v = crust_base + uplift + hill_delta;
                *cell = v.clamp(ocean_floor, ceiling);
            }
        });
}

fn apply_land_texture(
    elevation: &mut [f32],
    texture: &[f32],
    macro_mask: &[f32],
    dist_to_water: &[u32],
    dist_to_land: &[u32],
    _config: &WorldGenConfig,
    params: &ResolvedSimParams,
) {
    let strength = params.land_texture_strength_norm;
    if strength <= 0.0001 {
        return;
    }

    let coast_band = params.land_texture_coast_band_cells.max(1) as f32;
    let island_zone = params.island_zone_cells.max(1) as f32;
    let floor = params.ocean_floor_norm;
    let ceiling = params.max_elev_norm;

    for idx in 0..elevation.len() {
        let macro_v = macro_mask[idx];
        let weight = if macro_v >= MACRO_LAND_THRESHOLD {
            1.0
        } else if macro_v > 0.2 {
            smoothstep(coast_band, 0.0, dist_to_water[idx] as f32)
        } else {
            let island_w =
                smoothstep(island_zone, 0.0, dist_to_land[idx] as f32) * 0.5;
            if texture[idx] > 0.55 {
                island_w
            } else {
                0.0
            }
        };

        if weight <= 0.001 {
            continue;
        }
        let delta = strength * (texture[idx] - 0.5) * weight;
        elevation[idx] = (elevation[idx] + delta).clamp(floor, ceiling);
    }
}

fn generate_legacy_elevation(
    raw: &mut [f32],
    land_mask: &[f32],
    macro_mask: &[f32],
    dist_to_macro_water: &[u32],
    boundary: &[f32],
    noise: &TerrainNoise,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    w: usize,
    h: usize,
    mask_low: f32,
    mask_high: f32,
) {
    let interior_buffer = params.orogeny_interior_min_dist_cells as f32;
    let orogeny_threshold = config.orogeny_mountain_threshold;
    let orogeny_only = config.mountain_noise_orogeny_only;
    let continental_base = params.continental_base_norm;
    let abyssal_base = params.abyssal_base_norm;
    let ocean_floor = params.ocean_floor_norm;

    raw.par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * w + x;
                let mask = smoothstep(mask_low, mask_high, land_mask[idx]);
                let macro_land = macro_mask[idx] >= MACRO_WATER_THRESHOLD;
                let interior_w = smoothstep(
                    0.0,
                    interior_buffer,
                    dist_to_macro_water[idx] as f32,
                );
                let gated_boundary = if macro_land {
                    boundary[idx] * interior_w
                } else {
                    0.0
                };

                let norm_orogeny = (gated_boundary / BOUNDARY_UPLIFT_SCALE).clamp(0.0, 1.0);
                let detail = noise.sample_detail(
                    w as f32,
                    h as f32,
                    x,
                    y,
                    norm_orogeny,
                    orogeny_only,
                    orogeny_threshold,
                    false,
                );

                let crust_base =
                    continental_base * macro_mask[idx] + abyssal_base * (1.0 - macro_mask[idx]);
                let land_body = normalize01(detail + gated_boundary * BOUNDARY_UPLIFT_SCALE)
                    * 0.94
                    + continental_base * 0.12;
                let v = land_body * mask + crust_base * (1.0 - mask);
                *cell = v.max(ocean_floor);
            }
        });

    normalize_land_elevation_per_mass(
        raw,
        land_mask,
        w,
        h,
        mask_low,
        mask_high,
        ocean_floor,
    );
}

pub fn generate_elevation(
    map: &mut WorldMap,
    plates: &PlateData,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let w = map.width;
    let h = map.height;
    let macro_mask = crust_macro_mask(map, plates, params);
    map.macro_land_mask.clone_from(&macro_mask);
    let dist_to_macro_water = distance_to_macro_water(&macro_mask, w, h);
    let dist_to_macro_land = distance_to_macro_land(&macro_mask, w, h);
    let boundary = compute_boundary_influence(map, plates, config, params, &macro_mask);
    let noise = TerrainNoise::new(config, params);

    let mut raw = vec![0.0f32; w * h];

    report_stage(progress, stage_start, stage_end, 0.0, "Building tectonic base");

    match config.land_generation {
        LandGenerationMode::TectonicBase => {
            build_tectonic_base(
                &mut raw,
                &macro_mask,
                &dist_to_macro_water,
                &boundary,
                &noise,
                config,
                params,
                w,
                h,
            );

            report_stage(progress, stage_start, stage_end, 0.45, "Applying land texture");
            let texture = land_mask::generate_texture(config, params, Some(map), Some(plates));
            apply_land_texture(
                &mut raw,
                &texture,
                &macro_mask,
                &dist_to_macro_water,
                &dist_to_macro_land,
                config,
                params,
            );
            scale_tectonic_land_range(&mut raw, &macro_mask, params);
        }
        LandGenerationMode::LegacyMask => {
            let land_mask = land_mask::generate(config, params, Some(map), Some(plates));
            let drunkard_mask = config.land_mask_method == LandMaskMethod::DrunkardsWalk;
            let (mask_low, mask_high) = if drunkard_mask {
                (0.08, 0.24)
            } else {
                (0.30, 0.50)
            };
            generate_legacy_elevation(
                &mut raw,
                &land_mask,
                &macro_mask,
                &dist_to_macro_water,
                &boundary,
                &noise,
                config,
                params,
                w,
                h,
                mask_low,
                mask_high,
            );
        }
    }

    map.elevation.clone_from(&raw);
    map.orogeny = normalize_orogeny(&boundary);
}

/// Stretch continental land heights into a usable range above sea level (global, not per-mass).
fn scale_tectonic_land_range(raw: &mut [f32], macro_mask: &[f32], params: &ResolvedSimParams) {
    let sea = params.sea_level_norm;
    let ceiling = params.max_elev_norm;
    let mut land_min = f32::MAX;
    let mut land_max = f32::MIN;

    for (idx, &elev) in raw.iter().enumerate() {
        if macro_mask[idx] >= MACRO_LAND_THRESHOLD && elev >= sea {
            land_min = land_min.min(elev);
            land_max = land_max.max(elev);
        }
    }

    if land_max <= land_min {
        return;
    }

    let span = (land_max - land_min).max(0.0001);
    for (idx, cell) in raw.iter_mut().enumerate() {
        if macro_mask[idx] >= MACRO_LAND_THRESHOLD && *cell >= sea {
            let t = ((*cell - land_min) / span).clamp(0.0, 1.0).powf(0.55);
            *cell = (sea + 0.04 + t * (ceiling - sea - 0.04)).clamp(sea, ceiling);
        }
    }
}

/// Normalize each connected landmass independently (legacy mask mode only).
fn normalize_land_elevation_per_mass(
    raw: &mut [f32],
    land_mask: &[f32],
    w: usize,
    h: usize,
    mask_low: f32,
    mask_high: f32,
    floor: f32,
) {
    let len = w * h;
    let mut visited = vec![false; len];
    let ceiling = 1.0f32;

    for start in 0..len {
        if visited[start] || smoothstep(mask_low, mask_high, land_mask[start]) < 0.15 {
            continue;
        }

        let mut stack = vec![start];
        let mut component = Vec::new();
        visited[start] = true;

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
                if visited[nidx] || smoothstep(mask_low, mask_high, land_mask[nidx]) < 0.15 {
                    continue;
                }
                visited[nidx] = true;
                stack.push(nidx);
            }
        }

        let mut land_min = f32::MAX;
        let mut land_max = f32::MIN;
        for &idx in &component {
            land_min = land_min.min(raw[idx]);
            land_max = land_max.max(raw[idx]);
        }
        let span = (land_max - land_min).max(0.0001);

        for idx in component {
            let t = ((raw[idx] - land_min) / span).clamp(0.0, 1.0);
            let stretched = t.powf(0.42);
            let elev = floor + stretched * (ceiling - floor);
            raw[idx] = (elev * LAND_COMPRESSION + LAND_UPLIFT).clamp(floor, ceiling);
        }
    }

    for idx in 0..len {
        if smoothstep(mask_low, mask_high, land_mask[idx]) < 0.15 {
            raw[idx] = floor;
        }
    }
}
