use super::config::{LandGenerationMode, ResolvedSimParams, WorldGenConfig};
use super::world::WorldMap;

/// Collapse the ambiguous elevation band around sea level before water classification.
pub fn sharpen_elevation(map: &mut WorldMap, config: &WorldGenConfig, params: &ResolvedSimParams) {
    let sharpening = config.effective_coast_sharpening();
    if sharpening <= 0.001 {
        return;
    }

    let sea = params.sea_level_norm;
    let band_width = (0.12 * (1.0 - sharpening * 0.85)).max(0.015);
    let steepness = 2.0 + sharpening * 6.0;

    for v in &mut map.elevation {
        let dist_from_sea = (*v - sea).abs();
        if dist_from_sea > band_width * 2.5 {
            *v = v.clamp(0.0, 1.0);
            continue;
        }

        let t = (*v - sea) / band_width;
        *v = (sea + t.tanh() * steepness * band_width * 0.5).clamp(0.0, 1.0);
    }
}

/// Remove orphan land specks and tiny coastal water flecks after initial water classification.
pub fn cleanup_coastal_specks(
    map: &mut WorldMap,
    ocean_mask: &[bool],
    config: &WorldGenConfig,
    params: &ResolvedSimParams,
) {
    let w = map.width;
    let h = map.height;
    let passes = if config.land_generation == LandGenerationMode::TectonicBase
        && !config.legacy_coast_cleanup
    {
        1
    } else {
        config.coast_cleanup_passes
    };

    for _ in 0..passes {
        let water = map.water_mask.clone();
        let mut next = map.water_mask.clone();

        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                if water[idx] {
                    if !ocean_mask[idx] && land_neighbor_count(&water, w, h, x, y) >= 6 {
                        next[idx] = false;
                    }
                    continue;
                }

                let land_neighbors = land_neighbor_count(&water, w, h, x, y);
                if land_neighbors < 3
                    && near_ocean(
                        ocean_mask,
                        w,
                        h,
                        x,
                        y,
                        params.coast_proximity_cells,
                    )
                {
                    next[idx] = true;
                }
            }
        }
        map.water_mask = next;
    }
}

fn land_neighbor_count(water: &[bool], w: usize, h: usize, x: usize, y: usize) -> u32 {
    let mut count = 0u32;
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
            continue;
        }
        if !water[ny as usize * w + nx as usize] {
            count += 1;
        }
    }
    count
}

fn near_ocean(ocean: &[bool], w: usize, h: usize, x: usize, y: usize, radius: usize) -> bool {
    for dy in 0..=radius {
        for dx in 0..=radius {
            let nx = x as i32 + dx as i32 - radius as i32;
            let ny = y as i32 + dy as i32 - radius as i32;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            if ocean[ny as usize * w + nx as usize] {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldGenConfig;

    #[test]
    fn coast_cleanup_removes_orphan_land() {
        use crate::world::WorldMap;

        let config = WorldGenConfig::test_config(5, 16);
        let mut map = WorldMap::new(16, 16, 5);
        map.water_mask.fill(true);
        // Orphan land speck adjacent to the top-left ocean corner.
        map.water_mask[1 * 16 + 1] = false;

        let mut ocean = vec![false; 16 * 16];
        for x in 0..16 {
            ocean[x] = true;
            ocean[15 * 16 + x] = true;
        }
        for y in 0..16 {
            ocean[y * 16] = true;
            ocean[y * 16 + 15] = true;
        }

        let params = config.resolve();
        cleanup_coastal_specks(&mut map, &ocean, &config, &params);
        assert!(
            map.water_mask[1 * 16 + 1],
            "isolated land speck near ocean should become water"
        );
    }
}
