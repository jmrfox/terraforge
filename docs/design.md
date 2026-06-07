# Task: Implement a Modular 2D World Generation System in Rust

Create a new Rust module responsible for generating large-scale procedural world maps.

The goal is NOT a game-specific map generator. Instead, build a reusable world-generation library capable of generating:

- oceans
- continents
- islands
- mountain ranges
- hills
- rivers
- lakes
- temperature maps
- rainfall maps
- biome maps

The implementation should prioritize realism, determinism, and extensibility over raw speed.

---

# Design Requirements

## Deterministic

Generation must be fully deterministic.

The same:

- seed
- map dimensions
- generation parameters

must always produce identical output.

All randomness should originate from a single seeded RNG.

Use:

```rust
rand
rand_chacha
```

---

# Overall Pipeline

Implement the following stages:

```text
Generate Plates
    ↓
Generate Elevation
    ↓
Generate Oceans
    ↓
Generate Temperature
    ↓
Generate Rainfall
    ↓
Generate Rivers
    ↓
Generate Biomes
```

Each stage should be implemented as a separate module.

Avoid one giant function.

---

# World Representation

Create:

```rust
pub struct WorldMap
```

containing:

```rust
width: usize
height: usize

elevation: Vec<f32>
temperature: Vec<f32>
rainfall: Vec<f32>

water_mask: Vec<bool>
river_mask: Vec<bool>

biome: Vec<Biome>
plate_id: Vec<u32>
```

Store all grids as flattened arrays.

Provide:

```rust
fn index(x: usize, y: usize) -> usize
```

helper.

---

# Biomes

Create:

```rust
pub enum Biome
{
    Ocean,
    Lake,

    Ice,
    Tundra,
    Taiga,

    Grassland,
    TemperateForest,

    Desert,
    Savanna,

    TropicalForest,

    Mountain
}
```

Do not hardcode colors.

Rendering is not part of this task.

---

# Plate Generation

Use Voronoi-style tectonic plates.

Dependency:

```rust
voronoice
```

Generate:

```rust
20-100 plates
```

depending on map size.

Each plate should have:

```rust
id
center
motion vector
```

Store:

```rust
struct Plate
{
    id: u32,
    center: Vec2,
    velocity: Vec2,
}
```

Assign every map cell to the nearest plate center.

Store plate IDs in the world map.

---

# Elevation Generation

Elevation should be the combination of:

1. Large-scale continental structure
2. Plate interactions
3. Fractal terrain noise

Use:

```rust
noise
```

Implement:

```rust
base_continent_noise
mountain_noise
hill_noise
```

and combine them.

Normalize final elevation into:

```rust
0.0 .. 1.0
```

range.

---

# Plate Boundary Effects

For neighboring cells belonging to different plates:

Compute:

```rust
relative_velocity
```

between plates.

Approximate:

### Convergent Boundary

If plates move toward each other:

Increase elevation.

Creates mountain ranges.

### Divergent Boundary

If plates move apart:

Decrease elevation.

Creates rifts and ocean basins.

### Transform Boundary

Minor elevation effect.

Store boundary influence in a separate temporary field before applying.

Do NOT directly modify noise values.

## Plate geometry (Tier B)

After crust assignment, optional **Lloyd relaxation** (`plate_lloyd_iterations`,
default 2) moves plate centers toward Voronoi centroids for smoother boundaries.

**Velocity biasing** slows continental plates (`continental_plate_speed_max`) and
raises oceanic plate speeds (`oceanic_plate_speed_min`) with a slight bias toward the
nearest continental center. Optional `mantle_flow_angle_deg` rotates all velocities.

---

# Process-driven elevation (default: `TectonicBase`)

Bulk geography is assembled in layers:

1. **Tectonic base** — crust macro mask + plate-boundary uplift + minimal hill noise.
   Continents emerge from physical crust heights and convergent boundaries, not a soft land mask.
2. **Land texture overlay** — CA / noise / hybrid / drunkard methods add coastline irregularity
   as an elevation *delta* on and around macro land (`land_texture_strength_m`, coast band,
   optional `island_zone_m`). No `finalize_land_mask` cleanup in tectonic mode.
3. **Landscape evolution** — grid stream-power erosion/uplift loop (concepts inspired by
   [fastlem](https://github.com/TadaTeruki/fastlem) Salève model; no external dependency).
   Per-cell `uplift_rate` from `map.orogeny`; `erodibility` from orogeny belts vs plains.
4. **Optional climate refine** — short second LEM pass after rainfall when
   `rainfall_erodibility_coupling` > 0.

`LandGenerationMode::LegacyMask` preserves the older mask-primary blend + per-landmass normalize
for regression comparison.

The macro land mask is stored on `WorldMap.macro_land_mask`. Cached `flow_downslope` and
`flow_accumulation` from landscape evolution are reused by rivers.

---

# Ocean Determination

After elevation + landscape evolution:

Choose a sea level from physical datum (`sea_level_m`, default `0.0` m). At generation
start, `WorldGenConfig::resolve()` converts it to normalized `sea_level_norm` using
`max_elevation_m` and `ocean_floor_m`.

**Sea-level calibration** is editor-only: `WorldGenConfig::suggest_sea_level_m_for_fraction()`
and `calibrate_sea_level_norm()` — not applied during `generate_world`. The mapgui
"Calibrate sea level" button sets `sea_level_m` from a preview elevation field.

**Continental shelf:** macro-transition cells below sea level receive a gentle depth gradient
(`shelf_width_m`, `shelf_depth_m`); nearshore land gets a gentle slope in `TectonicBase` mode.
Coast sharpening is **disabled** in `TectonicBase` (shelf defines coasts); `LegacyMask` may
still use `coast_sharpening`.

Cells below sea level become water.

Then perform flood fill from map edges.

Only edge-connected water becomes ocean.

Interior depressions become lakes.

Create:

```rust
Ocean
Lake
```

distinction.

---

# Temperature Simulation

Generate temperature from:

## Latitude

Hot at equator.

Cold at poles.

Assume:

```rust
equator = map_height / 2
```

Use smooth interpolation.

## Elevation Cooling

Higher elevation reduces temperature.

Approximate atmospheric lapse rate.

Example:

```rust
temperature -= elevation * cooling_factor
```

**Continentality:** distance to ocean (BFS on `water_mask`) cools deep-interior land
beyond `continentality_ocean_range_m`, scaled by `continentality_strength`.

Normalize result:

```rust
0.0 .. 1.0
```

---

# Rainfall Simulation

Implement a simplified prevailing-wind model.

Assume configurable wind direction:

```rust
WestToEast
```

initially.

Algorithm:

For each row:

1. Moisture enters from ocean.
2. Moisture travels inland.
3. Orogeny belts remove moisture (`orographic_orogeny_weight` on `map.orogeny`, not raw elevation).
4. Interior drying reduces moisture with distance from ocean (`interior_drying_factor`).
5. Remaining moisture continues.

This should naturally create:

- wet coasts
- rain shadows
- interior deserts

Store rainfall:

```rust
0.0 .. 1.0
```

---

# River Generation

Do NOT generate random rivers.

Use hydrology.

For every land cell:

Determine:

```rust
downslope neighbor
```

using steepest descent.

Construct a drainage graph.

Compute flow accumulation.

Pseudo:

```text
all cells contribute 1 unit

flow accumulates downstream
```

Cells whose accumulation exceeds a threshold become rivers.

Requirements:

- rivers flow downhill
- rivers merge
- rivers end in ocean or lake

Store:

```rust
river_mask
```

---

# Mountain Classification

Create a mountain mask.

A cell becomes mountain if:

```rust
elevation > mountain_threshold
```

and local slope exceeds threshold.

Avoid labeling plateaus as mountains.

Implement local gradient estimation.

---

# Biome Assignment

Assign biomes from:

```rust
temperature
rainfall
elevation
water status
mountain status
```

Example rules:

```text
water + ocean -> Ocean
water + lake -> Lake

high elevation -> Mountain

cold + dry -> Tundra
cold + wet -> Taiga

temperate + dry -> Grassland
temperate + wet -> TemperateForest

hot + dry -> Desert
hot + medium -> Savanna
hot + wet -> TropicalForest
```

Keep thresholds configurable.

---

# Configuration

`WorldGenConfig` exposes **physical units** where applicable. Internal simulation
still uses normalized grids (`elevation` 0–1, cell indices). A single `resolve()` step
maps physical config → `ResolvedSimParams` at generation start.

**Anchor:** `cell_size_m` (default **20 m**). Horizontal distances in cells =
`physical_m / cell_size_m`. At 512×512 and 20 m/cell the map extent is **10.24 km × 10.24 km**.

| Category | Example fields | Default |
|----------|----------------|---------|
| Grid / datum | `cell_size_m`, `max_elevation_m`, `sea_level_m`, `ocean_floor_m` | 20 m, 9000 m, 0 m, −6000 m |
| Horizontal | `continental_margin_m`, `min_isthmus_width_m`, `mountain_belt_width_m`, `mountain_coast_buffer_m` | 200, 120, 60, 120 m |
| Areas | `min_lake_area_m2`, `river_min_drainage_area_km2` | 9600 m², 0.01536 km² |
| Elevation / slope | `mountain_min_elevation_m`, `mountain_min_slope_deg` | 4635 m, ~4.6° |
| Climate | `equator_mean_temp_c`, `pole_mean_temp_c`, `lapse_rate_c_per_km` | 30 °C, −30 °C, 6.5 °C/km |
| Noise | `continent_wavelength_m`, `hill_wavelength_m`, … | calibrated to prior frequencies |
| Elevation realism | `orogeny_interior_min_dist_m`, `mountain_noise_orogeny_only` | 120 m, true |
| Process geography | `land_generation`, `tectonic_uplift_scale`, `land_texture_strength_m` | TectonicBase, 1.0, 400 m |
| Landscape evolution | `landscape_evolution_enabled`, `landscape_erosion_factor`, `erodibility_plains` | true, 0.002, 4.0 |
| Oceans | `shelf_width_m`, `shelf_depth_m` | 80 m, 200 m |
| Rivers | `river_incision_enabled`, `river_incision_factor` | true, 0.003 |
| Plates (Tier B) | `plate_lloyd_iterations`, `continental_plate_speed_max`, `oceanic_plate_speed_min` | 2, 0.4, 0.4 |
| Climate realism | `orographic_orogeny_weight`, `interior_drying_factor`, `continentality_strength`, `continentality_ocean_range_m` | 0.65, 0.08, 0.12, 8000 m |

Dimensionless knobs (`seed`, `plate_count`, CA probabilities, `coast_sharpening` default **0.15**, etc.)
remain on `WorldGenConfig` unchanged.

---

# Public API

Primary entry point:

```rust
pub fn generate_world(
    config: &WorldGenConfig
) -> WorldMap
```

Pipeline:

```rust
plates
→ elevation (tectonic base + land texture)
→ landscape_evolution
→ oceans
→ temperature
→ rainfall
→ landscape_evolution (optional climate refine)
→ rivers (+ optional incision)
→ biomes
```

---

# Performance

Target:

```text
2048 × 2048 maps
```

without excessive memory allocations.

Guidelines:

- preallocate buffers
- reuse temporary arrays
- avoid HashMaps in inner loops
- avoid recursion

Favor cache-friendly iteration.

---

# Testing

Add tests for:

## Determinism

Same seed produces identical world.

## Ocean Connectivity

All ocean cells must connect to map edge.

## River Validity

Every river eventually reaches:

- lake
- ocean

## Range Validation

Elevation, temperature, rainfall remain:

```rust
0.0 .. 1.0
```

---

# Code Organization

Suggested layout:

```text
worldgen/

    mod.rs

    config.rs

    plates.rs

    elevation.rs

    oceans.rs

    temperature.rs

    rainfall.rs

    rivers.rs

    biomes.rs

    world.rs
```

Each module should expose a clean API and avoid circular dependencies.

Focus on correctness and maintainability first. Optimize only after the complete pipeline is functional.