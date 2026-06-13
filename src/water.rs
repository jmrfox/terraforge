use rayon::prelude::*;

use super::config::{ResolvedSimParams, WorldGenConfig};
use super::progress::{report, ProgressHandle};
use super::world::{Biome, WorldMap};

/// Classify water as ocean (edge-connected) or lake (enclosed depression).
pub fn generate_water(
    map: &mut WorldMap,
    _config: &WorldGenConfig,
    params: &ResolvedSimParams,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    report(progress, stage_start, "Classifying water");

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
    remove_small_inland_water(map, &ocean_mask, params.min_lake_cells);
    ocean_mask = flood_ocean_from_edges(map);

    for (idx, &is_ocean) in ocean_mask.iter().enumerate() {
        if map.water_mask[idx] {
            map.biome[idx] = if is_ocean { Biome::Ocean } else { Biome::Lake };
        }
    }

    report(progress, stage_end, "Water classified");
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
        generate_water(&mut map, &config, &params, &None, 0.0, 1.0);

        for y in 0..map.height {
            for x in 0..map.width {
                let idx = map.index(x, y);
                if map.biome[idx] == Biome::Ocean {
                    assert!(map.water_mask[idx]);
                }
            }
        }
    }
}
