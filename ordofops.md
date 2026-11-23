# Order of operations

Reference for the current Terraforge generation pipeline. Entry point: `generate_world` / `generate_world_with_progress` in `src/lib.rs`.

## Binaries

| Binary | Feature | Role |
|--------|---------|------|
| `mapgen` | default | Headless PNG, TIFF, and `.tfmap` export, plus batch presets |
| `mapgui` | `gui` | Interactive parameter editor and preview |
| `mapanalyze` | default | Batch diagnostics to stdout |

Configuration is a `WorldGenConfig` (JSON-serializable) in physical units. `WorldGenConfig::resolve()` derives the per-grid values in `ResolvedSimParams`.

## Pipeline stages

Order: **elevation → sea and coast → climate → hydrology → biomes**. Coasts come before climate so airflow and humidity see them, and hydrology follows climate because rivers need rainfall.

### 1. Elevation (`elevation.rs`)

- Sample FBM Perlin noise at three scales: continent, detail, and ridge. Sampling uses cell centers in world meters, so non-square maps are isotropic and different resolutions sample the same terrain.
- Each noise layer gets its own seed hashed from the full 64-bit map seed (`seeds.rs`).
- **Base blend:** weighted average of continent and detail. Detail can be modulated by an optional spatial envelope.
- **Ridge overlay:** `w_ridge * ridge_noise * ridge_envelope` is added on top. The ridge envelope is on by default.
- `edge_ocean_bias` lowers elevation within `edge_ocean_margin_m` of the map edge.
- Sea level for `target_land_fraction` is placed from a fixed 256×256 world-space lattice, so it does not change with resolution. By default the lattice covers the map. Set `land_reference_extent_m` to keep coastlines fixed across different map sizes.
- Output: `WorldMap.elevation` in `[0, 1]` and `WorldMap.ridge_influence`.

### 2. Sea and coast (`water.rs`)

- Cells below sea level become water. Edge-connected water is ocean, and enclosed water is a lake.
- Inland water smaller than `min_lake_area_m2` is raised to sea level and becomes ordinary land.
- `dist_to_water` is the Manhattan distance in cells to the nearest water cell, measured after ponds are filled.

### 3. Climate (`climate.rs`)

- **Temperature:** sea-level temperature from latitude, plus `temperature_offset_c` and local noise, minus the lapse rate over height above sea level.
- **Rainfall:** a steady upwind sweep carries moisture along `wind_from_deg`. Air recharges over water and rains as it travels. Rising terrain cools air and lowers its saturation humidity, so excess moisture condenses on the windward side and the lee side is drier.
- A subtropical dry belt near 27° latitude reduces rainfall by up to `subtropical_drying`.
- Outputs `temperature_c` and `rainfall_mm`, plus the normalized `temperature` (−15 to 35 °C) and `rainfall` (`1 - exp(-mm / 1500)`, zero on water) layers. That curve stays close to `mm / 2000` below 1000 mm and does not saturate in wet climates.
- Transcendental math uses `libm` so results match across platforms.

### 4. Hydrology (`hydrology.rs`)

- A priority flood from the ocean and map edges fills terrain pits so every cell has a route out. Across filled basins it follows the lowest original terrain, so drainage traces valley floors.
- Land runoff uses the Turc-Pike form of the Budyko water balance, with Holdridge potential evapotranspiration of `58.93 × biotemperature` mm/year.
- Runoff accumulates downstream into `discharge_m3s`. Cells at or above `river_min_discharge_m3s` become rivers, which remain land in `water_mask`.
- Depressions deeper than `min_lake_depth_m` and larger than `min_lake_area_m2` can hold lakes above sea level. Outflowing rivers cut down through their sills by `outlet_incision_m × √(Q / 10)`, which lowers lakes and drains shallow basins.
- A lake that evaporates faster than it is fed has no outlet. It shrinks to the level its water budget supports, and its basin floor drains inward.
- Each outlet to the sea, map edge, or closed basin defines a `basin_id`.
- `add_hydrology` reruns this stage and biome assignment on an existing map.

### 5. Biomes (`biomes.rs`)

- **Mountain:** elevation at or above `mountain_min_elevation_m`, `ridge_influence` at or above `mountain_min_ridge_influence`, and enough slope on smoothed terrain. The required slope starts at `mountain_min_slope_deg` and fades to zero 1000 m above the minimum elevation, so flat summits still count while low plateaus do not.
- Other land is classified from mean annual °C and annual rainfall, with the Köppen desert limit `10 × T + 140` mm.

## `WorldMap` fields

| Field | Type | Set by |
|-------|------|--------|
| `elevation` | `Vec<f32>` | Elevation, adjusted by water |
| `ridge_influence` | `Vec<f32>` | Elevation |
| `water_mask` | `Vec<bool>` | Water |
| `dist_to_water` | `Vec<u32>` | Water, updated by hydrology |
| `discharge_m3s`, `water_surface_m` | `Vec<f32>` | Hydrology |
| `flow_direction` | `Vec<u8>` | Hydrology |
| `basin_id`, `dist_to_river` | `Vec<u32>` | Hydrology |
| `river_mask` | `Vec<bool>` | Hydrology |
| `temperature`, `temperature_c` | `Vec<f32>` | Climate |
| `rainfall`, `rainfall_mm` | `Vec<f32>` | Climate |
| `biome` | `Vec<Biome>` | Water and biomes |
| `width`, `height`, `seed` | metadata | construction |
| `generator_version`, `cell_size_m`, `ocean_floor_m`, `max_elevation_m`, `sea_level_m` | metadata | construction |

## Export and analysis

- **Preview / export:** `preview.rs` — RGB biome map, multi-page TIFF layers, JSON stats sidecar.
- **Map storage:** `map_file.rs` — lossless compressed `.tfmap` import and export. The header holds a magic number, format version, compressed length, and checksum. The zlib payload holds map metadata, the generation config, and delta-coded layers. Layers that can be rebuilt exactly are omitted.
- **High-level controls:** `controls.rs` — world size, ruggedness, climate, aridity, and wind. Each one reads and writes ordinary config fields and runs before generation, so it does not affect `GENERATOR_VERSION`.
- **Parameter sampling:** `priors.rs` — random parameter exploration from prior distributions (GUI, CLI `--sample`, library).

## Generator version

`GENERATOR_VERSION` is recorded on every `WorldMap`. Bump it when a change alters output for an existing config, and update the table in `tests/golden_maps.rs`, which detects unintended changes.

See `README.md` for CLI usage and `cargo test` for pipeline invariants (determinism, value ranges, edge ocean connectivity).
