use std::cmp::Ordering;

use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
use super::grid_ops::{downsample_avg, upsample_bilinear};
use super::progress::{ProgressHandle, report_stage};
use super::world::WorldMap;

const DIRS: [(i32, i32); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

struct HydroGrid {
    elevation: Vec<f32>,
    macro_land_mask: Vec<f32>,
    orogeny: Vec<f32>,
    rainfall: Option<Vec<f32>>,
    width: usize,
    height: usize,
}

/// Grid-based stream-power landscape evolution (fastlem-inspired, no external dependency).
pub fn evolve_landscape(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
    rainfall: Option<&[f32]>,
) {
    if !config.landscape_evolution_enabled {
        return;
    }

    let factor = config.coarse_hydro_factor.max(1) as usize;
    let w = map.width;
    let h = map.height;
    let use_coarse = factor > 1 && w >= factor * 2 && h >= factor * 2;

    if use_coarse {
        let iterations = rainfall_adjusted_iterations(config, rainfall.is_some());
        let stage_coarse_end = if config.landscape_evolution_full_res_passes > 0 {
            stage_start + (stage_end - stage_start) * 0.85
        } else {
            stage_end
        };
        evolve_on_grid(
            map,
            config,
            params,
            progress,
            stage_start,
            stage_coarse_end,
            rainfall,
            factor,
            iterations,
        );

        if config.landscape_evolution_full_res_passes > 0 {
            evolve_on_grid(
                map,
                config,
                params,
                progress,
                stage_coarse_end,
                stage_end,
                rainfall,
                1,
                config.landscape_evolution_full_res_passes,
            );
        }
    } else {
        let iterations = rainfall_adjusted_iterations(config, rainfall.is_some());
        evolve_on_grid(
            map,
            config,
            params,
            progress,
            stage_start,
            stage_end,
            rainfall,
            1,
            iterations,
        );
    }
}

fn rainfall_adjusted_iterations(config: &WorldGenConfig, has_rainfall: bool) -> u32 {
    if has_rainfall {
        config.landscape_evolution_iterations.min(4).max(1)
    } else {
        config.landscape_evolution_iterations.max(1)
    }
}

fn evolve_on_grid(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
    rainfall: Option<&[f32]>,
    factor: usize,
    iterations: u32,
) {
    let full_w = map.width;
    let full_h = map.height;
    let mut grid = build_hydro_grid(map, rainfall, factor);
    let elev_start = grid.elevation.clone();
    let sea = params.sea_level_norm;
    let floor = params.ocean_floor_norm;
    let ceiling = params.max_elev_norm;
    let macro_thresh = 0.5f32;
    let k_erode = config.landscape_erosion_factor;
    let k_uplift = config.landscape_uplift_factor;

    let erodibility = build_erodibility(&grid, config, macro_thresh);

    let mut last_flow = vec![1.0f32; grid.width * grid.height];
    let mut last_downslope = vec![None; grid.width * grid.height];

    for iter in 0..iterations {
        if iter % 2 == 0 {
            report_stage(
                progress,
                stage_start,
                stage_end,
                iter as f32 / iterations as f32,
                "Evolving landscape",
            );
        }

        pin_outlets_grid(
            &mut grid.elevation,
            &grid.macro_land_mask,
            grid.width,
            grid.height,
            sea,
            floor,
            macro_thresh,
        );

        let (downslope, flow) = compute_flow_grid(&grid, sea);
        last_downslope = downslope;
        last_flow = flow;

        if factor == 1 {
            map.flow_downslope.clone_from(&last_downslope);
            map.flow_accumulation.clone_from(&last_flow);
        }

        let uplift = grid.orogeny.clone();
        let erode_delta: Vec<f32> = (0..grid.elevation.len())
            .into_par_iter()
            .map(|idx| {
                if grid.macro_land_mask[idx] < macro_thresh || grid.elevation[idx] < sea {
                    return 0.0;
                }
                let f = last_flow[idx].sqrt();
                k_erode * f / erodibility[idx]
            })
            .collect();

        grid.elevation
            .par_iter_mut()
            .enumerate()
            .for_each(|(idx, elev)| {
                if grid.macro_land_mask[idx] < macro_thresh {
                    *elev = (*elev).min(sea - 0.001).max(floor);
                    return;
                }
                *elev += k_uplift * uplift[idx];
                *elev -= erode_delta[idx];
                *elev = (*elev).clamp(floor, ceiling);
            });
    }

    pin_outlets_grid(
        &mut grid.elevation,
        &grid.macro_land_mask,
        grid.width,
        grid.height,
        sea,
        floor,
        macro_thresh,
    );

    if factor > 1 {
        let delta: Vec<f32> = grid
            .elevation
            .iter()
            .zip(elev_start.iter())
            .map(|(a, b)| a - b)
            .collect();
        let delta_full = upsample_bilinear(&delta, grid.width, grid.height, full_w, full_h);
        map.elevation
            .par_iter_mut()
            .zip(delta_full.par_iter())
            .for_each(|(elev, &d)| {
                *elev = (*elev + d).clamp(floor, ceiling);
            });
        map.flow_accumulation =
            upsample_bilinear(&last_flow, grid.width, grid.height, full_w, full_h);
    } else {
        map.flow_downslope = last_downslope;
        map.flow_accumulation = last_flow;
    }
}

fn build_hydro_grid(map: &WorldMap, rainfall: Option<&[f32]>, factor: usize) -> HydroGrid {
    let w = map.width;
    let h = map.height;
    if factor <= 1 {
        return HydroGrid {
            elevation: map.elevation.clone(),
            macro_land_mask: map.macro_land_mask.clone(),
            orogeny: map.orogeny.clone(),
            rainfall: rainfall.map(|r| r.to_vec()),
            width: w,
            height: h,
        };
    }

    let (elevation, cw, ch) = downsample_avg(&map.elevation, w, h, factor);
    let (macro_land_mask, _, _) = downsample_avg(&map.macro_land_mask, w, h, factor);
    let (orogeny, _, _) = downsample_avg(&map.orogeny, w, h, factor);
    let rainfall_ds = rainfall.map(|r| downsample_avg(r, w, h, factor).0);

    HydroGrid {
        elevation,
        macro_land_mask,
        orogeny,
        rainfall: rainfall_ds,
        width: cw,
        height: ch,
    }
}

fn build_erodibility(grid: &HydroGrid, config: &WorldGenConfig, macro_thresh: f32) -> Vec<f32> {
    let len = grid.width * grid.height;
    (0..len)
        .map(|idx| {
            if grid.macro_land_mask[idx] < macro_thresh {
                return 0.0;
            }
            let base = if grid.orogeny[idx] > config.orogeny_mountain_threshold {
                config.erodibility_mountains
            } else {
                config.erodibility_plains
            };
            let rain_mod = grid.rainfall.as_ref().map(|r| {
                let interior = r[idx];
                1.0 + config.rainfall_erodibility_coupling * (interior - 0.5)
            }).unwrap_or(1.0);
            (base * rain_mod).max(0.1)
        })
        .collect()
}

fn pin_outlets_grid(
    elevation: &mut [f32],
    macro_mask: &[f32],
    w: usize,
    h: usize,
    sea: f32,
    floor: f32,
    macro_thresh: f32,
) {
    elevation
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, elev) in row.iter_mut().enumerate() {
                if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                    *elev = floor;
                    continue;
                }
                let idx = y * w + x;
                if macro_mask[idx] < macro_thresh {
                    *elev = (*elev).min(sea - 0.001).max(floor);
                }
            }
        });
}

fn compute_flow_grid(grid: &HydroGrid, sea: f32) -> (Vec<Option<usize>>, Vec<f32>) {
    let w = grid.width;
    let h = grid.height;
    let len = w * h;
    let mut downslope = vec![None; len];
    let elevation = grid.elevation.as_slice();

    downslope.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, cell) in row.iter_mut().enumerate() {
            let idx = y * w + x;
            if elevation[idx] < sea {
                continue;
            }
            let elev = elevation[idx];
            let mut best: Option<(usize, f32)> = None;
            for (dx, dy) in DIRS {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let nidx = ny as usize * w + nx as usize;
                let drop = elev - elevation[nidx];
                if drop > 0.0 {
                    if best.map(|(_, d)| drop > d).unwrap_or(true) {
                        best = Some((nidx, drop));
                    }
                } else if elevation[nidx] < sea {
                    let sink_drop = elev - sea;
                    if sink_drop > 0.0 && best.is_none() {
                        best = Some((nidx, sink_drop));
                    }
                }
            }
            *cell = best.map(|(i, _)| i);
        }
    });

    let mut land_cells: Vec<usize> = (0..len)
        .filter(|&idx| elevation[idx] >= sea)
        .collect();
    land_cells.par_sort_by(|&a, &b| {
        elevation[b]
            .partial_cmp(&elevation[a])
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });

    let mut flow = vec![1.0f32; len];
    for &idx in &land_cells {
        if let Some(down) = downslope[idx] {
            flow[down] += flow[idx];
        }
    }

    (downslope, flow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldGenConfig;

    #[test]
    fn drainage_elevation_mostly_decreases_downstream() {
        let mut config = WorldGenConfig::test_config(8, 32);
        config.coarse_hydro_factor = 1;
        let params = config.resolve();
        let mut map = WorldMap::new(32, 32, 8);
        for idx in 0..32 * 32 {
            map.macro_land_mask[idx] = 0.9;
            map.elevation[idx] = 0.55 + (idx % 32) as f32 * 0.01;
            map.orogeny[idx] = 0.15;
        }
        evolve_landscape(&mut map, &config, &params, &None, 0.0, 1.0, None);

        let mut violations = 0usize;
        let mut checked = 0usize;
        for idx in 0..32 * 32 {
            if let Some(down) = map.flow_downslope[idx] {
                if map.elevation[idx] < params.sea_level_norm {
                    continue;
                }
                checked += 1;
                if map.elevation[idx] < map.elevation[down] - 0.001 {
                    violations += 1;
                }
            }
        }
        if checked > 0 {
            let rate = violations as f32 / checked as f32;
            assert!(rate < 0.15, "too many upstream elevation violations: {rate}");
        }
    }

    #[test]
    fn evolution_preserves_determinism() {
        let config = WorldGenConfig::test_config(5, 32);
        let params = config.resolve();
        let mut a = WorldMap::new(32, 32, 5);
        let mut b = WorldMap::new(32, 32, 5);
        for idx in 0..32 * 32 {
            a.macro_land_mask[idx] = if idx % 3 == 0 { 0.9 } else { 0.1 };
            b.macro_land_mask[idx] = a.macro_land_mask[idx];
            a.elevation[idx] = 0.5;
            b.elevation[idx] = 0.5;
            a.orogeny[idx] = 0.2;
            b.orogeny[idx] = 0.2;
        }
        evolve_landscape(&mut a, &config, &params, &None, 0.0, 1.0, None);
        evolve_landscape(&mut b, &config, &params, &None, 0.0, 1.0, None);
        assert_eq!(a.elevation, b.elevation);
    }

    #[test]
    fn coarse_hydro_runs_without_panic() {
        let mut config = WorldGenConfig::test_config(3, 64);
        config.coarse_hydro_factor = 4;
        let params = config.resolve();
        let mut map = WorldMap::new(64, 64, 3);
        for idx in 0..64 * 64 {
            map.macro_land_mask[idx] = 0.8;
            map.elevation[idx] = 0.55;
            map.orogeny[idx] = 0.1;
        }
        evolve_landscape(&mut map, &config, &params, &None, 0.0, 1.0, None);
        assert!(map.flow_accumulation.iter().any(|&f| f > 1.0));
    }
}
