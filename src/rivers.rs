use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rayon::prelude::*;

use super::config::WorldGenConfig;
use super::progress::{ProgressHandle, report, report_stage};
use super::world::{Biome, WorldMap};

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

struct MeanderField {
    noise: Fbm<Perlin>,
    strength: f32,
}

impl MeanderField {
    fn new(config: &WorldGenConfig) -> Self {
        Self {
            noise: Fbm::<Perlin>::new(config.seed as u32 + 6)
                .set_octaves(2)
                .set_frequency(0.08)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            strength: config.river_meander_strength,
        }
    }

    fn perturb_elevation(&self, x: usize, y: usize, elev: f32) -> f32 {
        if self.strength <= 0.001 {
            return elev;
        }
        let n = self.noise.get([x as f64, y as f64]) as f32;
        elev + n * self.strength
    }
}

/// Find steepest-descent neighbor index, or None if local minimum / water.
fn downslope_neighbor(map: &WorldMap, x: usize, y: usize, meander: &MeanderField) -> Option<usize> {
    let idx = map.index(x, y);
    if map.water_mask[idx] {
        return None;
    }
    let elev = meander.perturb_elevation(x, y, map.elevation[idx]);
    let mut best: Option<(usize, f32)> = None;

    for (dx, dy) in DIRS {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if !map.in_bounds(nx, ny) {
            continue;
        }
        let nidx = map.index(nx as usize, ny as usize);
        let neighbor_elev =
            meander.perturb_elevation(nx as usize, ny as usize, map.elevation[nidx]);
        if map.water_mask[nidx] {
            let drop = elev - neighbor_elev;
            if drop > 0.0 || neighbor_elev <= map.elevation[idx] {
                if best.map(|(_, d)| drop > d).unwrap_or(true) {
                    best = Some((nidx, drop.max(0.001)));
                }
            }
            continue;
        }
        let drop = elev - neighbor_elev;
        if drop > 0.0 {
            if best.map(|(_, d)| drop > d).unwrap_or(true) {
                best = Some((nidx, drop));
            }
        }
    }

    best.map(|(i, _)| i)
}

/// Hydrological river generation via flow accumulation.
pub fn generate_rivers(
    map: &mut WorldMap,
    config: &WorldGenConfig,
    progress: &Option<ProgressHandle>,
    stage_start: f32,
    stage_end: f32,
) {
    let len = map.width * map.height;
    map.river_mask.fill(false);

    report(progress, stage_start, "Carving river networks (terrain analysis)");

    let width = map.width;
    let height = map.height;
    let water_mask = map.water_mask.as_slice();
    let meander = MeanderField::new(config);
    let mut downslope = vec![None; len];
    let mut land_cells = Vec::new();

    downslope
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            if y % 8 == 0 {
                report_stage(
                    progress,
                    stage_start,
                    stage_end,
                    (y as f32 / height as f32) * 0.5,
                    "Carving river networks (terrain analysis)",
                );
            }
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * width + x;
                if water_mask[idx] {
                    continue;
                }
                *cell = downslope_neighbor(map, x, y, &meander);
            }
        });

    for idx in 0..len {
        if !water_mask[idx] {
            land_cells.push(idx);
        }
    }

    report(
        progress,
        stage_start + (stage_end - stage_start) * 0.55,
        "Carving river networks (flow accumulation)",
    );
    land_cells.sort_by(|&a, &b| {
        map.elevation[b]
            .partial_cmp(&map.elevation[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut flow = vec![1.0f32; len];
    let mut chain_length = vec![1u32; len];
    let land_len = land_cells.len().max(1);
    for (i, &idx) in land_cells.iter().enumerate() {
        if i % 256 == 0 {
            report_stage(
                progress,
                stage_start,
                stage_end,
                0.55 + (i as f32 / land_len as f32) * 0.45,
                "Carving river networks (flow accumulation)",
            );
        }
        if let Some(down) = downslope[idx] {
            flow[down] += flow[idx];
            chain_length[down] = chain_length[down].max(chain_length[idx] + 1);
        }
    }

    let min_length = config.river_min_length;
    for idx in 0..len {
        if !map.water_mask[idx]
            && flow[idx] >= config.river_flow_threshold
            && downslope[idx].is_some()
            && chain_length[idx] >= min_length
            && river_terminates_in_water(map, idx, config)
        {
            map.river_mask[idx] = true;
        }
    }
}

/// Trace a river cell downstream until ocean, lake, or dead end.
pub fn river_terminates_in_water(map: &WorldMap, start: usize, config: &WorldGenConfig) -> bool {
    let meander = MeanderField::new(config);
    let mut current = start;
    let mut visited = vec![false; map.width * map.height];
    for _ in 0..map.width * map.height {
        if visited[current] {
            return false;
        }
        visited[current] = true;

        if map.water_mask[current] {
            return map.biome[current] == Biome::Ocean || map.biome[current] == Biome::Lake;
        }

        let x = current % map.width;
        let y = current / map.width;
        match downslope_neighbor(map, x, y, &meander) {
            Some(next) => current = next,
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldGenConfig;
    use crate::oceans::generate_oceans;
    use crate::world::WorldMap;

    fn simple_slope_map(size: usize) -> WorldMap {
        let mut map = WorldMap::new(size, size, 1);
        for y in 0..size {
            for x in 0..size {
                let idx = map.index(x, y);
                map.elevation[idx] = 1.0 - (y as f32 / size as f32);
            }
        }
        map
    }

    #[test]
    fn rivers_flow_downhill() {
        let config = WorldGenConfig::test_config(2, 32);
        let mut map = simple_slope_map(32);
        let config = WorldGenConfig {
            river_flow_threshold: 5.0,
            river_min_length: 2,
            sea_level: 0.05,
            river_meander_strength: 0.0,
            ..config
        };
        generate_oceans(&mut map, &config);
        generate_rivers(&mut map, &config, &None, 0.0, 1.0);

        for y in 0..map.height {
            for x in 0..map.width {
                let idx = map.index(x, y);
                if map.river_mask[idx] {
                    let meander = MeanderField::new(&config);
                    let down = downslope_neighbor(&map, x, y, &meander);
                    assert!(down.is_some(), "river cell should have downslope path");
                }
            }
        }
    }
}
