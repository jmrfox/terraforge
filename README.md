# Terraforge

Deterministic procedural 2D world map generation for games and tools.

Pipeline: **plates → elevation → oceans → temperature → rainfall → rivers → biomes**

Design spec: [docs/design.md](docs/design.md)

## Install (Rust dependency)

```toml
terraforge = { git = "https://github.com/jmrfox/terraforge" }
```

```rust
use terraforge::{WorldGenConfig, generate_world};

let map = generate_world(&WorldGenConfig::default());
```

## Interactive GUI (`mapgui`)

Tweak all generation parameters, preview biome/elevation/temperature/rainfall layers, and export PNG or JSON presets:

```bash
cargo run --features gui --bin mapgui
cargo run --release --features gui --bin mapgui   # faster generation
```

The app auto-generates on startup (default 512×512). Use **Generate** after changing parameters. Load/save presets as JSON; export always writes the biome+rivers PNG.

## Headless map preview (`mapgen` CLI)

```bash
cargo run --bin mapgen -- -o out/map.png --width 512 --seed 42 --stats
cargo run --bin mapgen -- -o out/map.tiff --format tiff --width 512 --seed 42 --stats
cargo run --bin mapgen -- --batch mapgen_presets/example_batch.json --out-dir out/ --stats
cargo run --bin mapgen -- --batch mapgen_presets/example_batch.json --out-dir out/ --format tiff --stats
```

Multi-page TIFF output (use `--format tiff` or a `.tiff`/`.tif` extension). By default nine pages are written:

| Page | Layer | Encoding |
|------|-------|----------|
| 0 | Biome preview (rivers overlaid) | RGB8 |
| 1 | Elevation | 16-bit gray, `[0,1]` → full range |
| 2 | Temperature | 16-bit gray |
| 3 | Rainfall | 16-bit gray |
| 4 | Biome ID | 16-bit gray (0–10, legend in TIFF metadata) |
| 5 | Plate ID | 16-bit gray |
| 6 | Water mask | 8-bit gray |
| 7 | River mask | 8-bit gray |
| 8 | Mountain mask | 8-bit gray |

Use `--tiff-layers default` for the legacy two-page export (biomes + elevation only), or pick layers explicitly:

```bash
cargo run --bin mapgen -- -o out/climate.tiff --format tiff --tiff-layers elevation,temperature,rainfall
```

Release builds are faster for large maps:

```bash
cargo run --release --bin mapgen -- -o out/map.png --width 512 --seed 42
```

## Tests

```bash
cargo test
```

## Land-mask methods

| Method | Description |
|--------|-------------|
| `Hybrid` (default) | Cellular-automata continents blended with low-freq noise |
| `Noise` | Macro FBM continents |
| `CellularAutomata` | Cave-style organic blobs |
| `DrunkardsWalk` | Random-walk continent stamping |

Configure via `WorldGenConfig::land_mask_method` (JSON: `"Hybrid"`, `"Noise"`, etc.).
