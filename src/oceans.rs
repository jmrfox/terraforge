use rayon::prelude::*;

use super::coast;
use super::grid_ops::chamfer_distance;
use super::config::{ResolvedSimParams, WorldGenConfig};
use super::world::{Biome, WorldMap};

const MACRO_DEEP_OCEAN: f32 = 0.15;

/// Classify water as ocean (edge-connected) or lake (enclosed depression).
pub fn generate_oceans(map: &mut WorldMap, config: &WorldGenConfig, params: &ResolvedSimParams) {
    apply_continental_shelf(map, config, params);
    coast::sharpen_elevation(map, config, params);

    let len = map.width * map.height;
    let sea = params.sea_level_norm;
    let elevation = map.elevation.as_slice();

    map.water_mask
        .par_iter_mut()
        .enumerate()
        .for_each(|(idx, cell)| {
            *cell = elevation[idx] < sea;
        });

    map.dist_to_water =
        super::grid_ops::chamfer_distance_water(map.width, map.height, &map.water_mask);

    let mut ocean_mask = flood_ocean_from_edges(map);

    coast::cleanup_coastal_specks(map, &ocean_mask, config, params);

    ocean_mask = flood_ocean_from_edges(map);

    remove_small_inland_water(map, &ocean_mask, params.min_lake_cells);

    ocean_mask = flood_ocean_from_edges(map);

    for idx in 0..len {
        if map.water_mask[idx] {
            map.biome[idx] = if ocean_mask[idx] {
                Biome::Ocean
            } else {
                Biome::Lake
            };
        }
    }
}

/// Shelf bathymetry in macro transition zones and gentle nearshore land slopes.
fn apply_continental_shelf(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
) {
    if map.macro_land_mask.is_empty() {
        return;
    }

    let w = map.width;
    let h = map.height;
    let sea = params.sea_level_norm;
    let shelf_floor = (sea - params.shelf_depth_norm).max(params.abyssal_base_norm);
    let width = params.shelf_width_cells.max(1) as f32;
    let macro_mask = map.macro_land_mask.as_slice();
    let dist_ocean = chamfer_distance(w, h, |idx| macro_mask[idx] <= MACRO_DEEP_OCEAN);
    let dist_land = chamfer_distance(w, h, |idx| macro_mask[idx] >= 0.85);
    let nearshore_band = (params.shelf_width_cells * 2).max(1) as f32;

    for idx in 0..w * h {
        let macro_v = map.macro_land_mask[idx];
        let elev = map.elevation[idx];

        // Underwater shelf in macro transition
        if macro_v > MACRO_DEEP_OCEAN && macro_v < 0.85 && elev < sea {
            let t = (dist_ocean[idx] as f32 / width).clamp(0.0, 1.0);
            let shelf_elev = sea - (sea - shelf_floor) * (1.0 - t);
            map.elevation[idx] = elev
                .max(shelf_elev)
                .max(params.abyssal_base_norm);
        }

        // Nearshore land: gentle slope toward sea
        if macro_v >= 0.5 && elev >= sea {
            let coast_prox = dist_land[idx] as f32;
            if coast_prox < nearshore_band {
                let t = (coast_prox / nearshore_band).clamp(0.0, 1.0);
                let min_land = sea + (params.continental_base_norm - sea) * t * 0.35;
                if map.elevation[idx] < min_land {
                    map.elevation[idx] = min_land;
                }
            }
        }
    }
}

fn flood_ocean_from_edges(map: &WorldMap) -> Vec<bool> {
    let len = map.width * map.height;
    let mut ocean_mask = vec![false; len];
    let mut stack = Vec::new();
    let w = map.width;
    let h = map.height;

    for x in 0..w {
        for &y in &[0, h - 1] {
            let idx = map.index(x, y);
            if map.water_mask[idx] && !ocean_mask[idx] {
                stack.push(idx);
                ocean_mask[idx] = true;
            }
        }
    }
    for y in 0..h {
        for &x in &[0, w - 1] {
            let idx = map.index(x, y);
            if map.water_mask[idx] && !ocean_mask[idx] {
                stack.push(idx);
                ocean_mask[idx] = true;
            }
        }
    }

    while let Some(idx) = stack.pop() {
        let x = idx % w;
        let y = idx / w;
        let neighbors = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];
        for (nx, ny) in neighbors {
            if nx >= w || ny >= h {
                continue;
            }
            let nidx = map.index(nx, ny);
            if map.water_mask[nidx] && !ocean_mask[nidx] {
                ocean_mask[nidx] = true;
                stack.push(nidx);
            }
        }
    }

    ocean_mask
}

/// Fill tiny enclosed depressions so elevation noise does not pepper the map with lakes.
fn remove_small_inland_water(map: &mut WorldMap, ocean_mask: &[bool], min_cells: usize) {
    let w = map.width;
    let h = map.height;
    let len = w * h;
    let mut visited = vec![false; len];

    for start in 0..len {
        if visited[start] || !map.water_mask[start] || ocean_mask[start] {
            continue;
        }

        let mut stack = vec![start];
        let mut region = Vec::new();
        visited[start] = true;

        while let Some(idx) = stack.pop() {
            region.push(idx);
            let x = idx % w;
            let y = idx / w;
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx >= w || ny >= h {
                    continue;
                }
                let nidx = ny * w + nx;
                if visited[nidx] || !map.water_mask[nidx] || ocean_mask[nidx] {
                    continue;
                }
                visited[nidx] = true;
                stack.push(nidx);
            }
        }

        if region.len() < min_cells {
            for idx in region {
                map.water_mask[idx] = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldGenConfig;
    use crate::world::WorldMap;

    #[test]
    fn shelf_elevation_shallows_toward_coast() {
        let config = WorldGenConfig::default();
        let params = config.resolve();
        let w = 32usize;
        let h = 32usize;
        let mut map = WorldMap::new(w, h, 1);
        let sea = params.sea_level_norm;

        for x in 0..w {
            for y in 0..h {
                let idx = map.index(x, y);
                map.macro_land_mask[idx] = if x < 8 {
                    0.1
                } else if x > 22 {
                    0.9
                } else {
                    0.5
                };
                map.elevation[idx] = params.abyssal_base_norm.min(sea - 0.05);
            }
        }

        generate_oceans(&mut map, &config, &params);

        for y in 8..24 {
            let near_coast = map.elevation[map.index(20, y)];
            let near_abyss = map.elevation[map.index(10, y)];
            assert!(
                near_coast >= near_abyss,
                "shelf should shallow toward macro coast (coast {near_coast}, abyss {near_abyss})"
            );
        }
    }

    #[test]
    fn ocean_cells_connect_to_edge() {
        let config = WorldGenConfig::test_config(5, 16);
        let mut map = WorldMap::new(config.width, config.height, config.seed);
        for y in 0..map.height {
            for x in 0..map.width {
                let idx = map.index(x, y);
                map.elevation[idx] = if y == map.height - 1 { 0.1 } else { 0.9 };
            }
        }
        let params = config.resolve();
        generate_oceans(&mut map, &config, &params);

        let w = map.width;
        let h = map.height;
        for y in 0..h {
            for x in 0..w {
                let idx = map.index(x, y);
                if map.biome[idx] == Biome::Ocean {
                    assert!(map.water_mask[idx]);
                }
            }
        }
    }
}
