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

## Headless map preview (`mapgen` CLI)

```bash
cargo run --bin mapgen -- -o out/map.png --width 512 --seed 42 --stats
cargo run --bin mapgen -- --batch mapgen_presets/example_batch.json --out-dir out/ --stats
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
