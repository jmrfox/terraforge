# Terraforge Map Generation Order of Operations

Outline of the complete world generation pipeline, from seed to final biome map.

---

## Stage 0: Configuration Resolution
**Input:** `WorldGenConfig` (physical units)  
**Output:** `ResolvedSimParams` (normalized/cell units)

Converts all physical parameters (meters, degrees C, km²) to simulation units:
- `cell_size_m` → cell counts for distances
- `meters` → normalized elevation [0,1]
- `km²` → cell counts for areas
- Seed RNG initialized

---

## Stage 1: Tectonic Plates
**Module:** `plates.rs`  
**Progress:** 0% → 20%

1. **Generate plate centers** (Voronoi-style)
   - Distribute N points across map (N based on map area × plate density)
   - Default: 20-100 plates depending on map size

2. **Assign plate IDs** to each cell
   - Each cell gets nearest plate center ID
   - Stored in `map.plate_id`

3. **Lloyd relaxation** (optional, default: 2 iterations)
   - Smooths plate boundaries toward Voronoi centroids

4. **Assign crust types & velocities**
   - Continental plates: slower, clustered together
   - Oceanic plates: faster, fill remaining space
   - Store motion vectors (velocity) per plate

**Output:** `PlateData` struct with all plate properties

---

## Stage 2: Elevation (Tectonic Base)
**Module:** `elevation.rs`  
**Progress:** 20% → 38%

### 2a: Build Tectonic Base (20% → 35%)
**Input:** Plates + crust data  
**Output:** Base elevation + orogeny field

1. **Compute boundary influence**
   - For each cell, check 4 neighbors
   - If different plates: compute relative velocity
   - **Convergent:** uplift (mountain building)
   - **Divergent:** depression (rifts/basins)
   - **Transform:** minor effects
   - **Subduction:** trench + volcanic arc boost (our addition)

2. **Apply crust base elevation**
   - Continental crust: base height above sea level
   - Oceanic crust: abyssal floor depth
   - Add boundary uplift effects

3. **Apply subduction effects** (new)
   - Trench depression on oceanic side
   - Volcanic arc uplift on continental side

4. **Store orogeny field**
   - `map.orogeny` = normalized boundary uplift intensity
   - Used later for mountain classification & rain shadow

### 2b: Land Texture Overlay (35% → 45%)
**Purpose:** Break up uniform coastlines with irregular detail

1. **Generate texture mask** (CA / noise / hybrid / drunkard)
2. **Apply to tectonic base**
   - Add elevation deltas near coasts
   - Creates bays, peninsulas, islands
3. **Scale to final range**
   - Normalize land heights to [sea_level, max_elevation]

**Output:** `map.elevation` + `map.macro_land_mask` + `map.orogeny`

---

## Stage 3: Landscape Evolution (LEM)
**Module:** `landscape_evolution.rs`  
**Progress:** 38% → 45%

**Purpose:** Erode terrain based on tectonic uplift

1. **Compute steepest descent** for each cell
   - Find lowest neighbor (8-direction)
   - Store in `map.flow_downslope`

2. **Stream-power erosion loop**
   - Uplift rate from `map.orogeny` (tectonic forcing)
   - Erodibility: higher in plains, lower in orogeny belts
   - Iteratively carve valleys

3. **Flow accumulation**
   - Count upstream cells for each cell
   - Store in `map.flow_accumulation`
   - Reused by rivers later

**Output:** Modified `map.elevation` + cached flow data

---

## Stage 4: Oceans & Lakes
**Module:** `oceans.rs`  
**Progress:** 45% → 52%

1. **Apply sea level**
   - Cells below `sea_level_norm` → water candidates

2. **Flood fill from edges**
   - Edge-connected water = Ocean
   - Interior isolated water = Lake
   - Store in `map.water_mask`

3. **Continental shelf** (TectonicBase mode)
   - Macro-transition cells get gentle depth gradient
   - Nearshore land gets gentle slope

4. **Compute distance to water**
   - Chamfer distance for every cell to nearest water
   - Store in `map.dist_to_water`

**Output:** `map.water_mask` + `map.dist_to_water`

---

## Stage 5: Temperature
**Module:** `temperature.rs`  
**Progress:** 52% → 60%

1. **Latitude gradient**
   - Equator = hot, Poles = cold
   - Linear interpolation by Y position

2. **Elevation cooling** (lapse rate)
   - Higher elevation = colder
   - ~6.5°C per 1000m elevation

3. **Continentality** (distance to ocean)
   - Deep interior: more extreme temps (hotter/colder)
   - Scaled by distance from coast

4. **Add noise** for local variation

**Output:** `map.temperature` [0,1] normalized

---

## Stage 6: Rainfall
**Module:** `rainfall.rs`  
**Progress:** 60% → 68%

**Model:** Prevailing wind moisture transport

1. **Wind direction sweep** (default: West→East)
   - For each row, track moisture across map

2. **Moisture budget per cell:**
   - Start with ocean moisture
   - **Orographic lift:** Mountains (from `map.orogeny`) remove moisture → rain
   - **Interior drying:** Less moisture farther from ocean
   - Carry remainder to next cell

3. **Rain shadow effect**
   - Windward side of mountains: wet
   - Leeward side: dry (deserts)

**Output:** `map.rainfall` [0,1] normalized

---

## Stage 7: Climate-Driven Landscape Refinement (Optional)
**Module:** `landscape_evolution.rs` (second pass)  
**Progress:** 68% → 72%

**Triggered if:** `rainfall_erodibility_coupling > 0.001`

1. **Compute erodibility from rainfall**
   - Wetter areas: more erosion (softer rock/soil)
   - Drier areas: less erosion (harder rock)

2. **Short LEM pass**
   - Further refine valleys based on climate

**Output:** Refined `map.elevation`

---

## Stage 8: Rivers
**Module:** `rivers.rs`  
**Progress:** 72% → 88%

1. **Trace drainage networks**
   - Use cached `map.flow_downslope` from Stage 3
   - Accumulate upstream area

2. **Threshold for river formation**
   - Cells with `flow_accumulation > threshold` → river

3. **Carve river channels** (optional incision)
   - Lower elevation along river paths
   - Creates V-shaped valleys

4. **Validate connectivity**
   - All rivers must reach ocean or lake

**Output:** `map.river_mask` + modified `map.elevation`

---

## Stage 9: Mountain Classification
**Module:** `biomes.rs` (`compute_mountain_mask`)  
**Progress:** part of 88% → 100%

1. **Filter candidates:**
   - Must be land (not water)
   - Must be interior (coast buffer: 400m from water)
   - Must exceed elevation threshold

2. **Orogeny-driven mountains:**
   - Local orogeny peak > threshold
   - High slope OR strong orogeny belt

3. **Dilate mountain mask**
   - Expand slightly for visual coherence

**Output:** `map.mountain_mask`

---

## Stage 10: Biomes
**Module:** `biomes.rs` (`generate_biomes`)  
**Progress:** 88% → 100%

**Decision tree per cell:**

```
Water? → Ocean / Lake
Mountain? → Mountain
↓
Temperature bands:
  Cold (-30 to -10°C): Tundra (dry) / Taiga (wet)
  Temperate (0 to 20°C): Grassland (dry) / TemperateForest (wet)
  Hot (20 to 30°C): Desert (dry) / Savanna (med) / TropicalForest (wet)
```

**Factors:**
- Temperature + Rainfall → biome category
- Elevation → Mountain override
- Water status → Ocean/Lake

**Output:** `map.biome` (enum per cell)

---

## Summary Pipeline

```
Config → ResolvedSimParams
         ↓
    TECTONIC PLATES (voronoi + velocities)
         ↓
    ELEVATION (boundary uplift + subduction + texture)
         ↓
    LANDSCAPE EVOLUTION (erosion from orogeny)
         ↓
    OCEANS (sea level + flood fill + shelf)
         ↓
    TEMPERATURE (latitude + elevation + continentality)
         ↓
    RAINFALL (wind + orogeny shadow + drying)
         ↓
    LEM REFINE (optional climate-driven erosion)
         ↓
    RIVERS (drainage + accumulation + carving)
         ↓
    MOUNTAINS (classification from elevation + orogeny)
         ↓
    BIOMES (temp + rain + elevation rules)
         ↓
    WorldMap (complete)
```

---

## Key Data Flows

| Field | Created | Used By |
|-------|---------|---------|
| `plate_id` | Stage 1 | Stage 2 (boundaries) |
| `orogeny` | Stage 2 | Stage 3 (uplift), Stage 6 (rain shadow), Stage 9 (mountains) |
| `elevation` | Stage 2 | Stage 3, 4, 5, 7, 8 |
| `macro_land_mask` | Stage 2 | Stage 4 (shelf), Stage 6 (continentality) |
| `flow_downslope` | Stage 3 | Stage 8 (rivers) |
| `water_mask` | Stage 4 | Stage 5, 6, 8, 9 |
| `dist_to_water` | Stage 4 | Stage 5 (continentality), Stage 9 (coast buffer) |
| `temperature` | Stage 5 | Stage 10 |
| `rainfall` | Stage 6 | Stage 7 (optional), Stage 10 |
| `river_mask` | Stage 8 | Stage 10 |
| `mountain_mask` | Stage 9 | Stage 10 |
