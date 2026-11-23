//! Water classification before climate, so filled ponds become ordinary land.

use crate::config::ResolvedSimParams;
use crate::grid_ops::chamfer_distance_water;
use crate::world::{Biome, WorldMap};

/// Mark ocean and lakes, fill small ponds to sea level, and measure distance to water.
pub fn generate(map: &mut WorldMap, params: &ResolvedSimParams) {
    let sea = params.sea_level_norm;
    for (is_water, &elevation) in map.water_mask.iter_mut().zip(&map.elevation) {
        *is_water = elevation < sea;
    }

    let ocean = flood_ocean_from_edges(map);
    remove_small_inland_water(map, &ocean, params.min_lake_cells);
    for (elevation, &is_water) in map.elevation.iter_mut().zip(&map.water_mask) {
        if !is_water && *elevation < sea {
            *elevation = sea;
        }
    }

    let ocean = flood_ocean_from_edges(map);
    for (idx, &is_ocean) in ocean.iter().enumerate() {
        if map.water_mask[idx] {
            map.biome[idx] = if is_ocean { Biome::Ocean } else { Biome::Lake };
        }
    }
    map.dist_to_water = chamfer_distance_water(map.width, map.height, &map.water_mask);
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

    #[test]
    fn classifies_ocean_lakes_and_fills_ponds() {
        let config = WorldGenConfig {
            width: 16,
            height: 16,
            min_lake_area_m2: crate::SquareMeters(4.0 * 400.0 * 400.0),
            ..WorldGenConfig::default()
        };
        let params = config.resolve();
        let sea = params.sea_level_norm;
        let mut map = WorldMap::from_config(&config);
        map.elevation.fill(sea + 0.1);
        for x in 0..16 {
            let idx = map.index(x, 15);
            map.elevation[idx] = sea - 0.2;
        }
        for (x, y) in [(4, 4), (5, 4), (4, 5), (5, 5)] {
            let idx = map.index(x, y);
            map.elevation[idx] = sea - 0.1;
        }
        let pond = map.index(10, 8);
        map.elevation[pond] = sea - 0.1;

        generate(&mut map, &params);

        assert_eq!(map.biome[map.index(3, 15)], Biome::Ocean);
        assert_eq!(map.biome[map.index(4, 4)], Biome::Lake);
        assert!(!map.water_mask[pond]);
        assert_eq!(map.elevation[pond], sea);
        assert!(map.dist_to_water[pond] > 0);
    }
}
