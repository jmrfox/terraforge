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

---

# Ocean Determination

After elevation generation:

Choose a sea level:

```rust
sea_level = 0.50
```

initially configurable.

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
3. Mountains remove moisture.
4. Remaining moisture continues.

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

Create:

```rust
pub struct WorldGenConfig
```

containing:

```rust
seed
width
height

plate_count

sea_level

mountain_threshold

river_threshold

temperature_scale
rainfall_scale
```

All magic numbers must be configurable.

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
→ elevation
→ oceans
→ temperature
→ rainfall
→ rivers
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