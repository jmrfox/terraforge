# Order of operations

Reference for the current Terraforge generation pipeline. Entry point: `generate_world` / `generate_world_with_progress` in `src/lib.rs`.

## Binaries

| Binary | Feature | Role |
|--------|---------|------|
| `mapgen` | default | Headless PNG/TIFF export and batch presets |
| `mapgui` | `gui` | Interactive parameter editor and preview |
| `mapanalyze` | default | Batch diagnostics to stdout |

Configuration is a `WorldGenConfig` (JSON-serializable). Physical units (meters, °C) are converted to simulation-normalized values via `WorldGenConfig::resolve()` → `ResolvedSimParams`.

## Pipeline stages

### 1. Elevation (`elevation.rs`)

- Sample FBM Perlin noise at three scales: continent, detail, ridge.
- **Base blend:** weighted average of continent + detail; detail can be modulated by an optional spatial envelope.
- **Ridge overlay:** `w_ridge * ridge_noise * ridge_envelope` added on top (additive, not blended into the base).
- Ridge envelope is on by default (broad mountain belts); detail envelope is off by default.
- Optional edge falloff (`edge_ocean_bias`) lowers elevation near map borders.
- Optional `target_land_fraction`: shift the field so the configured sea level yields that land share.
- Output: `WorldMap.elevation` in `[0, 1]` and `WorldMap.ridge_influence` (`ridge_noise × ridge_envelope`, per cell).

Note: additive ridge composition differs from the older three-way weighted average; re-tune `elevation_ridge_weight` / envelope params when migrating presets.

### 2. Temperature (`temperature.rs`)

- Base latitudinal gradient (poles cold, equator warm).
- Elevation lapse cooling from resolved `elevation_cooling_factor`.
- Low-amplitude spatial noise for variation.
- Output: `WorldMap.temperature` in `[0, 1]`.

### 3. Rainfall (`rainfall.rs`)

- FBM noise field scaled by `rainfall_scale`.
- Modulated by distance to water (coast proximity / continentality) and elevation (orographic boost).
- Box-blurred for spatial coherence.
- Output: `WorldMap.rainfall` in `[0, 1]`.

### 4. Water (`water.rs`)

- Cells below `sea_level_norm` become water (`water_mask`).
- Chamfer distance to nearest water cell (`dist_to_water`).
- Edge-connected flood fill → ocean; enclosed depressions → lakes.
- Small inland water bodies below `min_lake_area_m2` are removed.
- Water cells get provisional `Biome::Ocean` or `Biome::Lake`.

### 5. Biomes (`biomes.rs`)

- Rule-based assignment from elevation, temperature, rainfall, water mask, slope, and ridge influence.
- **Mountain:** normalized elevation ≥ `mountain_elev_norm`, neighbor slope ≥ `mountain_slope_norm` (physical degrees converted to per-cell normalized delta), and `ridge_influence` ≥ `mountain_min_ridge_influence`.
- Land biomes: ice, tundra, taiga, grassland, temperate forest, desert, savanna, tropical forest.
- Output: final `WorldMap.biome` per cell.

## `WorldMap` fields

| Field | Type | Set by |
|-------|------|--------|
| `elevation` | `Vec<f32>` | Stage 1 |
| `ridge_influence` | `Vec<f32>` | Stage 1 |
| `temperature` | `Vec<f32>` | Stage 2 |
| `rainfall` | `Vec<f32>` | Stage 3 |
| `water_mask` | `Vec<bool>` | Stage 4 |
| `dist_to_water` | `Vec<u32>` | Stage 4 |
| `biome` | `Vec<Biome>` | Stages 4–5 |
| `width`, `height`, `seed` | metadata | construction |

## Export and analysis

- **Preview / export:** `preview.rs` — RGB biome map, multi-page TIFF layers, JSON stats sidecar.
- **Parameter sampling:** `priors.rs` — random parameter exploration from prior distributions (GUI, CLI `--sample`, library).

See `README.md` for CLI usage and `cargo test` for pipeline invariants (determinism, value ranges, edge ocean connectivity).
