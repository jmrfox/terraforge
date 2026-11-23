//! Surface hydrology: depression filling, flow routing, rivers, lakes, and basins.
//!
//! A priority flood from the sea fills terrain pits so every cell has a route out.
//! Runoff from the climate layers then accumulates downstream. Deep depressions hold
//! lakes. Outflowing rivers cut down through their sills, which lowers each lake and
//! drains shallow basins. A lake that evaporates faster than it is fed shrinks into a
//! terminal lake with no outlet.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::climate::normalized_rainfall;
use crate::config::{ResolvedSimParams, WorldGenConfig};
use crate::grid_ops::{chamfer_distance, chamfer_distance_water};
use crate::world::{Biome, WorldMap, FLOW_OFFSETS};

const NO_CELL: u32 = u32::MAX;
const SECONDS_PER_YEAR: f64 = 31_557_600.0;
/// Holdridge potential evapotranspiration, in mm/year per °C of biotemperature.
const PET_MM_PER_C: f64 = 58.93;
const MAX_BIOTEMPERATURE_C: f64 = 30.0;
/// Open water evaporates somewhat faster than vegetated land.
const OPEN_WATER_EVAPORATION_FACTOR: f64 = 1.1;

fn potential_evapotranspiration_mm(temperature_c: f64) -> f64 {
    PET_MM_PER_C * temperature_c.clamp(0.0, MAX_BIOTEMPERATURE_C)
}

/// Annual runoff from land using the Turc-Pike form of the Budyko water balance.
fn land_runoff_mm(rainfall_mm: f64, pet_mm: f64) -> f64 {
    if rainfall_mm <= 0.0 {
        return 0.0;
    }
    if pet_mm <= 0.0 {
        return rainfall_mm;
    }
    let ratio = rainfall_mm / pet_mm;
    (rainfall_mm - rainfall_mm / (1.0 + ratio * ratio).sqrt()).max(0.0)
}

fn neighbors(idx: usize, w: usize, h: usize) -> impl Iterator<Item = (usize, usize)> {
    let (x, y) = ((idx % w) as i32, (idx / w) as i32);
    FLOW_OFFSETS
        .iter()
        .enumerate()
        .filter_map(move |(dir, &(dx, dy))| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32)
                .then(|| (dir, ny as usize * w + nx as usize))
        })
}

fn step_length(dir: usize) -> f32 {
    if dir & 1 == 0 {
        1.0
    } else {
        std::f32::consts::SQRT_2
    }
}

/// Min-heap entry. Across a filled basin all heights tie, so the flood follows the lowest
/// original terrain. That makes drainage trace valley floors instead of straight lines.
#[derive(PartialEq)]
struct QueuedCell {
    height: f32,
    terrain: f32,
    sequence: u32,
    idx: u32,
}

impl Eq for QueuedCell {}

impl Ord for QueuedCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .height
            .total_cmp(&self.height)
            .then(other.terrain.total_cmp(&self.terrain))
            .then(other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for QueuedCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Flood {
    /// Entered cells, downstream first.
    order: Vec<u32>,
    /// Cell each entered cell was reached from.
    parent: Vec<u32>,
    /// Water surface after filling pits.
    filled: Vec<f32>,
}

/// Priority flood outward from `seeds`. `can_enter(from, to)` limits the flooded area.
fn priority_flood(
    elevation: &[f32],
    w: usize,
    h: usize,
    seeds: &[u32],
    can_enter: impl Fn(usize, usize) -> bool,
) -> Flood {
    let n = w * h;
    let mut filled = elevation.to_vec();
    let mut parent = vec![NO_CELL; n];
    let mut queued = vec![false; n];
    let mut order = Vec::new();
    let mut heap = BinaryHeap::with_capacity(seeds.len());
    let mut sequence = 0u32;

    for &seed in seeds {
        queued[seed as usize] = true;
        heap.push(QueuedCell {
            height: elevation[seed as usize],
            terrain: elevation[seed as usize],
            sequence,
            idx: seed,
        });
        sequence += 1;
    }

    while let Some(cell) = heap.pop() {
        let current = cell.idx as usize;
        order.push(cell.idx);
        for (_, next) in neighbors(current, w, h) {
            if queued[next] || !can_enter(current, next) {
                continue;
            }
            queued[next] = true;
            filled[next] = filled[next].max(filled[current]);
            parent[next] = cell.idx;
            heap.push(QueuedCell {
                height: filled[next],
                terrain: elevation[next],
                sequence,
                idx: next as u32,
            });
            sequence += 1;
        }
    }

    Flood {
        order,
        parent,
        filled,
    }
}

struct Lake {
    cells: Vec<u32>,
    /// Earliest flooded cell, which is the last to receive all of the lake's inflow.
    outlet: u32,
    level: f32,
    terminal: bool,
}

/// Thresholds for deciding which depressions hold lakes, in normalized elevation.
struct LakeRules {
    min_cells: usize,
    min_depth: f32,
    /// Sill incision at a flow of 10 m³/s.
    incision_at_10_m3s: f64,
}

/// Find lakes in depressions, with each surface lowered by its outlet's incision.
fn find_lakes(
    map: &WorldMap,
    filled: &[f32],
    ocean: &[bool],
    rank: &[u32],
    through_flow: &[f64],
    rules: &LakeRules,
) -> (Vec<Lake>, Vec<u32>) {
    let (w, h) = (map.width, map.height);
    let n = w * h;
    let mut lake_of = vec![NO_CELL; n];
    let mut depression_of = vec![NO_CELL; n];
    let mut in_pool = vec![false; n];
    let mut lakes = Vec::new();
    let mut stack = Vec::new();
    let mut depressions = 0u32;

    for start in 0..n {
        if depression_of[start] != NO_CELL || ocean[start] || filled[start] <= map.elevation[start]
        {
            continue;
        }
        let level = filled[start];
        let depression = depressions;
        depressions += 1;
        let mut cells = Vec::new();
        depression_of[start] = depression;
        stack.push(start);
        while let Some(idx) = stack.pop() {
            cells.push(idx as u32);
            for (_, next) in neighbors(idx, w, h) {
                if depression_of[next] == NO_CELL
                    && !ocean[next]
                    && filled[next].to_bits() == level.to_bits()
                    && filled[next] > map.elevation[next]
                {
                    depression_of[next] = depression;
                    stack.push(next);
                }
            }
        }

        let outflow = cells
            .iter()
            .map(|&c| through_flow[c as usize])
            .fold(0.0, f64::max);
        let lowered = level - (rules.incision_at_10_m3s * (outflow / 10.0).sqrt()) as f32;

        // Lowering the surface can split one depression into several pools.
        for &seed in &cells {
            let seed = seed as usize;
            if in_pool[seed] || map.elevation[seed] >= lowered {
                continue;
            }
            let mut pool = Vec::new();
            in_pool[seed] = true;
            stack.push(seed);
            while let Some(idx) = stack.pop() {
                pool.push(idx as u32);
                for (_, next) in neighbors(idx, w, h) {
                    if !in_pool[next]
                        && depression_of[next] == depression
                        && map.elevation[next] < lowered
                    {
                        in_pool[next] = true;
                        stack.push(next);
                    }
                }
            }
            let lowest = pool
                .iter()
                .map(|&c| map.elevation[c as usize])
                .fold(f32::INFINITY, f32::min);
            if pool.len() >= rules.min_cells && lowered - lowest >= rules.min_depth {
                let id = lakes.len() as u32;
                for &c in &pool {
                    lake_of[c as usize] = id;
                }
                let outlet = *pool.iter().min_by_key(|&&c| rank[c as usize]).unwrap();
                lakes.push(Lake {
                    cells: pool,
                    outlet,
                    level: lowered,
                    terminal: false,
                });
            }
        }
    }
    (lakes, lake_of)
}

/// Cells of a closed basin that stay flooded, lowest first, within its water budget.
fn terminal_lake_cells(
    map: &WorldMap,
    lake: &Lake,
    budget: f64,
    cost: impl Fn(usize) -> f64,
    min_cells: usize,
) -> Vec<u32> {
    let (w, h) = (map.width, map.height);
    let mut sorted = lake.cells.clone();
    sorted.sort_by(|&a, &b| {
        map.elevation[a as usize]
            .total_cmp(&map.elevation[b as usize])
            .then(a.cmp(&b))
    });

    let mut used = 0.0;
    let mut kept = Vec::new();
    for &c in &sorted {
        let i = c as usize;
        let cell_cost = cost(i);
        if used + cell_cost > budget {
            break;
        }
        used += cell_cost;
        kept.push(c);
    }

    // Drop pools too small to count as lakes.
    let kept_set: std::collections::HashSet<u32> = kept.iter().copied().collect();
    let mut visited = std::collections::HashSet::new();
    let mut result = Vec::new();
    for &start in &kept {
        if !visited.insert(start) {
            continue;
        }
        let mut pool = vec![start];
        let mut stack = vec![start];
        while let Some(c) = stack.pop() {
            for (_, next) in neighbors(c as usize, w, h) {
                let next = next as u32;
                if kept_set.contains(&next) && visited.insert(next) {
                    pool.push(next);
                    stack.push(next);
                }
            }
        }
        if pool.len() >= min_cells {
            result.extend(pool);
        }
    }
    result
}

/// Route water across the map and fill the hydrology layers.
pub fn generate(map: &mut WorldMap, config: &WorldGenConfig, params: &ResolvedSimParams) {
    let (w, h) = (map.width, map.height);
    let n = w * h;
    let span_m = (params.max_elevation_m - params.ocean_floor_m).max(1.0);
    let hydrology = &config.hydrology;

    let ocean: Vec<bool> = map.biome.iter().map(|&b| b == Biome::Ocean).collect();

    // Everything drains to the sea or off the map edge.
    let seeds: Vec<u32> = (0..n)
        .filter(|&i| {
            let on_edge = i % w == 0 || i % w == w - 1 || i / w == 0 || i / w == h - 1;
            if ocean[i] {
                neighbors(i, w, h).any(|(_, next)| !ocean[next])
            } else {
                on_edge
            }
        })
        .map(|i| i as u32)
        .collect();
    let mut flood = priority_flood(&map.elevation, w, h, &seeds, |_, next| !ocean[next]);
    flood.order.retain(|&c| !ocean[c as usize]);

    let mut rank = vec![u32::MAX; n];
    for (position, &c) in flood.order.iter().enumerate() {
        rank[c as usize] = position as u32;
    }

    // Steepest descent on the filled surface. Flats follow the flood back to their exit.
    let mut flow = flood.parent.clone();
    for &c in &flood.order {
        let i = c as usize;
        let mut best = 0.0f32;
        for (dir, next) in neighbors(i, w, h) {
            let drop = (flood.filled[i] - flood.filled[next]) / step_length(dir);
            if drop > best {
                best = drop;
                flow[i] = next as u32;
            }
        }
    }

    let mm_to_flow = params.cell_size_m * params.cell_size_m / 1000.0 / SECONDS_PER_YEAR;
    let pet: Vec<f64> = map
        .temperature_c
        .iter()
        .map(|&t| potential_evapotranspiration_mm(f64::from(t)))
        .collect();
    let rain: Vec<f64> = map.rainfall_mm.iter().map(|&p| f64::from(p)).collect();
    let runoff = |i: usize| land_runoff_mm(rain[i], pet[i]) * mm_to_flow;
    let net_evaporation =
        |i: usize| (pet[i] * OPEN_WATER_EVAPORATION_FACTOR - rain[i]) * mm_to_flow;

    // A first pass without lakes estimates how much water reaches each depression's sill.
    let mut through_flow = vec![0.0f64; n];
    for &c in flood.order.iter().rev() {
        let i = c as usize;
        through_flow[i] += runoff(i);
        let next = flow[i];
        if next != NO_CELL && !ocean[next as usize] {
            through_flow[next as usize] += through_flow[i];
        }
    }

    let (mut lakes, lake_of) = find_lakes(
        map,
        &flood.filled,
        &ocean,
        &rank,
        &through_flow,
        &LakeRules {
            min_cells: params.min_lake_cells,
            min_depth: (hydrology.min_lake_depth_m / span_m) as f32,
            incision_at_10_m3s: hydrology.outlet_incision_m / span_m,
        },
    );

    let mut inflow = vec![0.0f64; n];
    let mut discharge = vec![0.0f32; n];
    let mut lake_balance = vec![0.0f64; lakes.len()];
    let mut lake_water = vec![false; n];
    let mut surface_norm = map.elevation.clone();
    let mut terminal_seeds = Vec::new();

    for &c in flood.order.iter().rev() {
        let i = c as usize;
        let lake_id = lake_of[i];
        if lake_id == NO_CELL {
            let total = inflow[i] + runoff(i);
            discharge[i] = total as f32;
            let next = flow[i];
            if next != NO_CELL && !ocean[next as usize] {
                inflow[next as usize] += total;
            }
            continue;
        }

        let l = lake_id as usize;
        lake_balance[l] += inflow[i] - net_evaporation(i);
        if c != lakes[l].outlet {
            continue;
        }

        let balance = lake_balance[l];
        if balance >= 0.0 {
            // Enough water to fill to the outlet and overflow.
            let level = lakes[l].level;
            for &cell in &lakes[l].cells {
                let cell = cell as usize;
                lake_water[cell] = true;
                surface_norm[cell] = level;
                discharge[cell] = balance as f32;
            }
            let next = flow[i];
            if next != NO_CELL && !ocean[next as usize] {
                inflow[next as usize] += balance;
            }
        } else {
            lakes[l].terminal = true;
            let lake = &lakes[l];
            let external: f64 = balance
                + lake
                    .cells
                    .iter()
                    .map(|&cell| net_evaporation(cell as usize))
                    .sum::<f64>();
            let land_runoff: f64 = lake.cells.iter().map(|&cell| runoff(cell as usize)).sum();
            let kept = terminal_lake_cells(
                map,
                lake,
                external + land_runoff,
                |cell| net_evaporation(cell) + runoff(cell),
                params.min_lake_cells,
            );
            if kept.is_empty() {
                // A dry sink: water soaks in or evaporates at the lowest point.
                let lowest = *lake
                    .cells
                    .iter()
                    .min_by(|&&a, &&b| {
                        map.elevation[a as usize]
                            .total_cmp(&map.elevation[b as usize])
                            .then(a.cmp(&b))
                    })
                    .unwrap();
                flow[lowest as usize] = NO_CELL;
                terminal_seeds.push(lowest);
            } else {
                let level = kept
                    .iter()
                    .map(|&cell| map.elevation[cell as usize])
                    .fold(f32::NEG_INFINITY, f32::max);
                for &cell in &kept {
                    let cell = cell as usize;
                    lake_water[cell] = true;
                    surface_norm[cell] = level;
                    flow[cell] = NO_CELL;
                }
                terminal_seeds.extend(kept);
            }
        }
    }

    // Exposed floors of closed basins drain inward to their terminal lake or sink.
    let exposed: Vec<bool> = (0..n)
        .map(|i| {
            lake_of[i] != NO_CELL
                && lakes[lake_of[i] as usize].terminal
                && !lake_water[i]
                && flow[i] != NO_CELL
        })
        .collect();
    let basin_floor = priority_flood(&map.elevation, w, h, &terminal_seeds, |from, next| {
        exposed[next] && lake_of[next] == lake_of[from]
    });
    for &c in basin_floor.order.iter().rev() {
        let i = c as usize;
        if !exposed[i] {
            continue;
        }
        let total = inflow[i] + runoff(i);
        discharge[i] = total as f32;
        let next = basin_floor.parent[i];
        flow[i] = next;
        if next != NO_CELL && exposed[next as usize] {
            inflow[next as usize] += total;
        }
    }

    // Each outlet to the sea, map edge, or closed basin defines a drainage basin.
    let mut basin_id = vec![0u32; n];
    let mut lake_basin = vec![0u32; lakes.len()];
    let mut next_basin = 1u32;
    let mut path = Vec::new();
    for start in 0..n {
        if ocean[start] || basin_id[start] != 0 {
            continue;
        }
        path.clear();
        let mut current = start;
        let id = loop {
            path.push(current);
            let next = flow[current];
            if next == NO_CELL || ocean[next as usize] {
                let closed = lake_of[current] != NO_CELL
                    && lakes[lake_of[current] as usize].terminal
                    && next == NO_CELL;
                if closed {
                    let slot = &mut lake_basin[lake_of[current] as usize];
                    if *slot == 0 {
                        *slot = next_basin;
                        next_basin += 1;
                    }
                    break *slot;
                }
                let id = next_basin;
                next_basin += 1;
                break id;
            }
            if basin_id[next as usize] != 0 {
                break basin_id[next as usize];
            }
            current = next as usize;
        };
        for &cell in &path {
            basin_id[cell] = id;
        }
    }

    for i in 0..n {
        let is_water = ocean[i] || lake_water[i];
        map.water_mask[i] = is_water;
        if is_water && !ocean[i] {
            map.biome[i] = Biome::Lake;
        }
        map.rainfall[i] = normalized_rainfall(map.rainfall_mm[i], is_water);
        map.water_surface_m[i] = if ocean[i] {
            params.sea_level_m as f32
        } else {
            (params.ocean_floor_m + f64::from(surface_norm[i]) * span_m) as f32
        };
        map.discharge_m3s[i] = if ocean[i] { 0.0 } else { discharge[i] };
        map.flow_direction[i] = if ocean[i] || flow[i] == NO_CELL {
            0
        } else {
            neighbors(i, w, h)
                .find(|&(_, next)| next == flow[i] as usize)
                .map_or(0, |(dir, _)| dir as u8 + 1)
        };
        map.river_mask[i] =
            !is_water && f64::from(discharge[i]) >= hydrology.river_min_discharge_m3s;
    }
    map.basin_id = basin_id;
    map.dist_to_water = chamfer_distance_water(w, h, &map.water_mask);
    let rivers = &map.river_mask;
    map.dist_to_river = chamfer_distance(w, h, |i| rivers[i]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{add_hydrology, generate_world, water};

    /// Map with terrain from `height_m(x, y)` in meters and uniform climate.
    fn synthetic(
        w: usize,
        h: usize,
        temperature_c: f32,
        rainfall_mm: f32,
        height_m: impl Fn(usize, usize) -> f64,
    ) -> (WorldMap, WorldGenConfig) {
        let config = WorldGenConfig {
            width: w,
            height: h,
            hydrology: crate::HydrologyConfig {
                min_lake_depth_m: 20.0,
                outlet_incision_m: 0.0,
                ..Default::default()
            },
            ..WorldGenConfig::default()
        };
        let params = config.resolve();
        let span = params.max_elevation_m - params.ocean_floor_m;
        let mut map = WorldMap::from_config(&config);
        for y in 0..h {
            for x in 0..w {
                let idx = map.index(x, y);
                map.elevation[idx] = ((height_m(x, y) - params.ocean_floor_m) / span) as f32;
            }
        }
        water::generate(&mut map, &params);
        map.temperature_c.fill(temperature_c);
        map.rainfall_mm.fill(rainfall_mm);
        (map, config)
    }

    fn route(map: &mut WorldMap, config: &WorldGenConfig) {
        generate(map, config, &config.resolve());
    }

    /// Slope down to a southern ocean, with a bowl centered at (20, 15).
    fn bowl_on_slope(x: usize, y: usize) -> f64 {
        let slope = (39.0 - y as f64) * 12.0 - 10.0;
        let r2 = (x as f64 - 20.0).powi(2) + (y as f64 - 15.0).powi(2);
        slope - 220.0 * (-r2 / 30.0).exp()
    }

    #[test]
    fn budyko_runoff_behaves_at_its_limits() {
        assert_eq!(land_runoff_mm(0.0, 800.0), 0.0);
        assert_eq!(land_runoff_mm(500.0, 0.0), 500.0);
        assert!(
            land_runoff_mm(100.0, 2000.0) < 5.0,
            "arid land keeps its rain"
        );
        let wet = land_runoff_mm(4000.0, 800.0);
        assert!(
            (wet - 3200.0).abs() < 100.0,
            "wet land loses about PET: {wet}"
        );
    }

    #[test]
    fn all_runoff_reaches_the_sea() {
        let (mut map, config) = synthetic(40, 30, 10.0, 1000.0, |x, y| {
            if y >= 28 {
                -50.0
            } else {
                (28 - y) as f64 * 15.0 + (x as f64 - 20.0).abs() * 2.0
            }
        });
        route(&mut map, &config);

        let params = config.resolve();
        let per_cell = land_runoff_mm(1000.0, potential_evapotranspiration_mm(10.0))
            * params.cell_size_m
            * params.cell_size_m
            / 1000.0
            / SECONDS_PER_YEAR;
        let land = map.water_mask.iter().filter(|w| !**w).count() as f64;
        let mut delivered = 0.0f64;
        for i in 0..map.biome.len() {
            if map.water_mask[i] {
                continue;
            }
            let exits = match map.downstream(i) {
                Some(next) => map.water_mask[next],
                None => true,
            };
            if exits {
                delivered += f64::from(map.discharge_m3s[i]);
            }
        }
        let expected = per_cell * land;
        assert!(
            (delivered - expected).abs() / expected < 1e-3,
            "{delivered} vs {expected}"
        );
    }

    #[test]
    fn wet_depression_fills_to_its_spill_point_and_overflows() {
        let (mut map, config) = synthetic(40, 40, 8.0, 1600.0, bowl_on_slope);
        route(&mut map, &config);

        let center = map.index(20, 15);
        assert_eq!(map.biome[center], Biome::Lake);
        assert!(map.water_surface_m[center] > map.elevation_m(center) as f32);
        assert!(
            map.water_surface_m[center] > 0.0,
            "lake sits above sea level"
        );

        let surface = map.water_surface_m[center];
        let mut current = center;
        let mut steps = 0;
        while map.water_mask[current] && map.biome[current] == Biome::Lake {
            assert_eq!(map.water_surface_m[current], surface);
            current = map.downstream(current).expect("full lake has an outlet");
            steps += 1;
            assert!(steps < 1600, "flow must not cycle");
        }
        assert!(
            map.discharge_m3s[current] > 0.0,
            "lake overflows downstream"
        );
    }

    #[test]
    fn dry_depression_becomes_a_smaller_closed_basin() {
        let lake_cells = |map: &WorldMap| map.biome.iter().filter(|b| **b == Biome::Lake).count();
        let (mut wet, config) = synthetic(40, 40, 8.0, 1600.0, bowl_on_slope);
        route(&mut wet, &config);
        let (mut dry, config) = synthetic(40, 40, 26.0, 180.0, bowl_on_slope);
        route(&mut dry, &config);

        assert!(lake_cells(&dry) < lake_cells(&wet));
        let rim = dry.index(20, 11);
        let mut current = rim;
        let mut steps = 0;
        while let Some(next) = dry.downstream(current) {
            assert!(!dry.water_mask[next] || dry.biome[next] == Biome::Lake);
            current = next;
            steps += 1;
            assert!(steps < 1600, "flow must not cycle");
        }
        assert_ne!(
            dry.basin_id[dry.index(20, 15)],
            dry.basin_id[dry.index(20, 36)],
            "closed basin is separate from the slope to the sea"
        );
    }

    #[test]
    fn outlet_incision_lowers_and_can_drain_lakes() {
        let lake_cells = |map: &WorldMap| map.biome.iter().filter(|b| **b == Biome::Lake).count();
        let run = |incision: f64| {
            let (mut map, mut config) = synthetic(40, 40, 8.0, 1600.0, bowl_on_slope);
            config.hydrology.outlet_incision_m = incision;
            route(&mut map, &config);
            map
        };
        let (full, lowered, drained) = (run(0.0), run(100.0), run(5000.0));
        let center = full.index(20, 15);
        assert!(lake_cells(&lowered) < lake_cells(&full));
        assert!(lowered.water_surface_m[center] < full.water_surface_m[center]);
        assert_eq!(lake_cells(&drained), 0);
        assert!(drained.river_mask.contains(&true) || drained.discharge_m3s[center] > 0.0);
    }

    #[test]
    fn map_without_ocean_drains_off_its_edges() {
        let (mut map, config) = synthetic(24, 24, 10.0, 1000.0, |x, y| {
            300.0 - (x as f64 - 12.0).abs() * 4.0 - (y as f64 - 12.0).abs() * 4.0
        });
        assert!(!map.biome.contains(&Biome::Ocean));
        route(&mut map, &config);
        for i in 0..map.biome.len() {
            let mut current = i;
            let mut steps = 0;
            while let Some(next) = map.downstream(current) {
                current = next;
                steps += 1;
                assert!(steps < 600);
            }
            let (x, y) = (current % 24, current / 24);
            assert!(x == 0 || y == 0 || x == 23 || y == 23);
        }
    }

    #[test]
    fn generated_flow_is_acyclic_and_consistent() {
        for seed in [42, 7, 99] {
            let config = WorldGenConfig::test_config(seed, 192);
            let map = generate_world(&config);
            for i in 0..map.biome.len() {
                let mut current = i;
                let mut steps = 0;
                while let Some(next) = map.downstream(current) {
                    if !map.water_mask[current] && !map.water_mask[next] {
                        assert_eq!(map.basin_id[current], map.basin_id[next]);
                    }
                    current = next;
                    steps += 1;
                    assert!(steps <= map.biome.len(), "flow cycle from {i}");
                    if map.biome[current] == Biome::Ocean {
                        break;
                    }
                }
                if map.river_mask[i] {
                    assert!(!map.water_mask[i]);
                    assert!(
                        f64::from(map.discharge_m3s[i]) >= config.hydrology.river_min_discharge_m3s
                    );
                }
                assert!(map.discharge_m3s[i].is_finite() && map.discharge_m3s[i] >= 0.0);
                assert_eq!(map.basin_id[i] == 0, map.biome[i] == Biome::Ocean);
            }
        }
    }

    #[test]
    fn rivers_are_stable_across_resolutions() {
        let delivered = |m: &WorldMap| -> f64 {
            (0..m.biome.len())
                .filter(|&i| m.biome[i] != Biome::Ocean)
                .filter(|&i| {
                    m.downstream(i)
                        .is_none_or(|next| m.biome[next] == Biome::Ocean)
                })
                .map(|i| f64::from(m.discharge_m3s[i]))
                .sum()
        };
        for seed in [42, 99] {
            let coarse = WorldGenConfig {
                seed,
                width: 256,
                height: 256,
                ..WorldGenConfig::default()
            };
            let fine = WorldGenConfig {
                width: 512,
                height: 512,
                cell_size_m: crate::Meters(coarse.cell_size_m.0 / 2.0),
                ..coarse.clone()
            };
            let (a, b) = (generate_world(&coarse), generate_world(&fine));

            let (sea_a, sea_b) = (delivered(&a), delivered(&b));
            assert!((sea_a - sea_b).abs() / sea_a < 0.05, "{sea_a} vs {sea_b}");

            let rivers: Vec<usize> = (0..a.biome.len()).filter(|&i| a.river_mask[i]).collect();
            assert!(!rivers.is_empty());
            let matched = rivers
                .iter()
                .filter(|&&i| b.dist_to_river[b.index(2 * (i % a.width), 2 * (i / a.width))] <= 6)
                .count();
            let overlap = matched as f32 / rivers.len() as f32;
            assert!(overlap > 0.8, "seed {seed}: river overlap {overlap}");
        }
    }

    #[test]
    fn hydrology_can_be_rebuilt_on_an_existing_map() {
        let config = WorldGenConfig::test_config(42, 128);
        let original = generate_world(&config);
        let mut rebuilt = original.clone();
        rebuilt.discharge_m3s.fill(0.0);
        rebuilt.flow_direction.fill(0);
        rebuilt.basin_id.fill(0);
        rebuilt.river_mask.fill(false);
        add_hydrology(&mut rebuilt, &config);

        assert_eq!(original.discharge_m3s, rebuilt.discharge_m3s);
        assert_eq!(original.flow_direction, rebuilt.flow_direction);
        assert_eq!(original.basin_id, rebuilt.basin_id);
        assert_eq!(original.river_mask, rebuilt.river_mask);
        assert_eq!(original.water_mask, rebuilt.water_mask);
        assert_eq!(original.biome, rebuilt.biome);
    }
}
