//! Golden fingerprints for generator version 1.
//!
//! Saved maps store only their config and regenerate on load, so any change to these
//! hashes changes existing worlds. Algorithm changes must ship as a new
//! `GENERATOR_VERSION` while version 1 keeps producing exactly these outputs.

use terraforge::{biome_to_id, generate_world, WorldGenConfig, WorldMap};

const LAYERS: [&str; 7] = [
    "elevation",
    "ridge_influence",
    "temperature",
    "rainfall",
    "water_mask",
    "dist_to_water",
    "biome",
];

const GOLDEN: &[(&str, [u64; 7])] = &[
    (
        "default_256",
        [
            0xd75f50576db1999e,
            0xa16fb221fb0e5780,
            0x7ab5e793185b8802,
            0x590a7ab6fb16ac5d,
            0x3fed7c311acac9a1,
            0x80feed34b6a6dd08,
            0x4f96cb311191c096,
        ],
    ),
    (
        "seed_7_128",
        [
            0x1ea2a201c2fb6b5e,
            0xf87674b616130e9f,
            0x179cb195b6e35573,
            0xbe9b39936ff66538,
            0x19e68c96dce76fca,
            0xa5c7b898dedd9e19,
            0xe994bb1061e2b80d,
        ],
    ),
    (
        "seed_99_128",
        [
            0x815acc7bcbed17cc,
            0x43eee14036b31a82,
            0x10fea10d5fc7ca52,
            0xcea734d89a1ba328,
            0xe6fe79b1dc2f3e9e,
            0xc8e2b29dce9cae67,
            0xd3a7aac151d88a65,
        ],
    ),
    (
        "non_square_200x96",
        [
            0x93735332dc6e11e7,
            0xf20ad0e444446dff,
            0xe85ed791306dcc65,
            0x7f9c407f47d11f9c,
            0x9957227a2952632a,
            0x471fd01492d4f236,
            0xe8329f337cf1a832,
        ],
    ),
    (
        "game_land_070",
        [
            0xdf7628707b766ba8,
            0x8e35fb5a3d79bcc9,
            0xc3d3e0a10e874f94,
            0x5052ee04fc490617,
            0x8c7239edd07df235,
            0xabb5c75998712236,
            0x177580e437e917f1,
        ],
    ),
    (
        "no_land_target_no_edge",
        [
            0x1d6bfa604468b14d,
            0xb03bff8415832f0e,
            0x3913782aa7cd7d31,
            0x720f47c57988ef28,
            0x470d06ff3af4976d,
            0x3280a4afb21c543d,
            0x196f720b6b321c2c,
        ],
    ),
    (
        "detail_envelope",
        [
            0xf2a4ec7c555cd817,
            0xf87674b616130e9f,
            0x7b670d619bca6caf,
            0xa8595ee78b79a547,
            0xe4a3b8f84adb7b7a,
            0x71142dd018b960fc,
            0x869c4214d0256cf4,
        ],
    ),
    (
        "saved_map_v1",
        [
            0xdf7628707b766ba8,
            0x8e35fb5a3d79bcc9,
            0xc3d3e0a10e874f94,
            0x5052ee04fc490617,
            0x8c7239edd07df235,
            0xabb5c75998712236,
            0x177580e437e917f1,
        ],
    ),
];

fn fnv1a(bytes: impl Iterator<Item = u8>) -> u64 {
    bytes.fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn hash_f32(values: &[f32]) -> u64 {
    fnv1a(values.iter().flat_map(|v| v.to_bits().to_le_bytes()))
}

fn fingerprint(map: &WorldMap) -> [u64; 7] {
    [
        hash_f32(&map.elevation),
        hash_f32(&map.ridge_influence),
        hash_f32(&map.temperature),
        hash_f32(&map.rainfall),
        fnv1a(map.water_mask.iter().map(|&w| u8::from(w))),
        fnv1a(map.dist_to_water.iter().flat_map(|d| d.to_le_bytes())),
        fnv1a(map.biome.iter().flat_map(|&b| biome_to_id(b).to_le_bytes())),
    ]
}

fn sized(seed: u64, width: usize, height: usize) -> WorldGenConfig {
    WorldGenConfig {
        seed,
        width,
        height,
        ..WorldGenConfig::default()
    }
}

fn cases() -> Vec<(&'static str, WorldGenConfig)> {
    let mut detail_envelope = sized(7, 128, 128);
    detail_envelope.elevation_detail_envelope.enabled = true;

    vec![
        ("default_256", sized(42, 256, 256)),
        ("seed_7_128", sized(7, 128, 128)),
        ("seed_99_128", sized(99, 128, 128)),
        ("non_square_200x96", sized(3, 200, 96)),
        (
            "game_land_070",
            WorldGenConfig {
                target_land_fraction: Some(0.7),
                ..sized(2103, 128, 128)
            },
        ),
        (
            "no_land_target_no_edge",
            WorldGenConfig {
                target_land_fraction: None,
                edge_ocean_bias: 0.0,
                ..sized(5, 128, 128)
            },
        ),
        ("detail_envelope", detail_envelope),
        (
            "saved_map_v1",
            serde_json::from_str(include_str!("fixtures/saved_map_v1.json"))
                .expect("saved v1 map must keep loading"),
        ),
    ]
}

#[test]
fn generator_v1_output_is_unchanged() {
    let actual: Vec<_> = cases()
        .into_iter()
        .map(|(name, config)| (name, fingerprint(&generate_world(&config))))
        .collect();

    let mut table = String::new();
    for (name, hashes) in &actual {
        let values: Vec<String> = hashes.iter().map(|h| format!("0x{h:016x}")).collect();
        table.push_str(&format!("    (\"{name}\", [{}]),\n", values.join(", ")));
    }

    assert_eq!(actual.len(), GOLDEN.len(), "golden table:\n{table}");
    for ((name, hashes), (golden_name, golden)) in actual.iter().zip(GOLDEN) {
        assert_eq!(name, golden_name, "golden table:\n{table}");
        for (layer, (got, want)) in LAYERS.iter().zip(hashes.iter().zip(golden)) {
            assert_eq!(got, want, "{name}: layer `{layer}` changed");
        }
    }
}
