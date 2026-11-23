//! Golden fingerprints for the generator.
//!
//! These detect accidental changes to generated maps. When a change is intended, bump
//! `GENERATOR_VERSION` and update the table from the failure message.

use terraforge::{biome_to_id, generate_world, Meters, WorldGenConfig, WorldMap};

const LAYERS: [&str; 15] = [
    "elevation",
    "ridge_influence",
    "temperature",
    "rainfall",
    "water_mask",
    "dist_to_water",
    "biome",
    "temperature_c",
    "rainfall_mm",
    "discharge_m3s",
    "flow_direction",
    "basin_id",
    "river_mask",
    "dist_to_river",
    "water_surface_m",
];

const GOLDEN: &[(&str, [u64; 15])] = &[
    (
        "default_256",
        [
            0xf8ac4f5aa16ede5c,
            0x02afffe2cd22cfca,
            0x9281b22828d72818,
            0x02f0f2afe82153dc,
            0x79149d6405402f53,
            0x181f79145c07d019,
            0x4864ebd1ee4bed9f,
            0x16ba2d0c35a68ade,
            0xf0bad6a6b056db24,
            0x9328003a90fd126a,
            0xabea68e7a39e243e,
            0xebf51ff42982defa,
            0x56a347d4599b2844,
            0x5955a16b88b4b03d,
            0x9f3a8ba1faab9f58,
        ],
    ),
    (
        "non_square_200x96",
        [
            0x06a88811d8a9132b,
            0xc5162ad21fa061ed,
            0x3759d578539a1a0a,
            0xde5c1912acb6f0cc,
            0x441a21fed0f81365,
            0xb737bfd75f60d0a3,
            0x9ff4f91a1bf56cda,
            0x279e94d037c5baa1,
            0x5e2ed22732a8dae6,
            0x7672fb489e4a2e0b,
            0x0c511c8a9f26e575,
            0x57f4ea32abf21659,
            0xcf66fa0f4a4a37ce,
            0x9ab803622aac0a05,
            0x0396a9f239ed1566,
        ],
    ),
    (
        "tropical_easterly",
        [
            0x7beea321c6312560,
            0xcfc9e0f0a5ce3570,
            0x1df7a34fee457143,
            0x1b0ae7dfdb244259,
            0x0eab68784455dea5,
            0x6484f4e39aa89681,
            0x1979c1c91100c01a,
            0x73b9b97f19969cc5,
            0x6eb5cdcb3703796e,
            0xf6fbf7f5d1b44fa6,
            0xd951286749991dc3,
            0x7cecfba567fb1342,
            0x9c1bda7f8c872325,
            0xb9f13a0aa87f2325,
            0x5837505e621ad108,
        ],
    ),
    (
        "southern_diagonal",
        [
            0x48f598b2281f87d9,
            0x559538b87462a048,
            0xb5f1b832c857f62e,
            0xfb560cb56cf7b8dc,
            0xff0eba194063f24b,
            0xb6c7d0989fd345a9,
            0x2a746fbe00c0ca36,
            0x4858e9d7449d1512,
            0xf77cd4b9f80a373f,
            0x80ec86fe3763f26d,
            0xa874ebb70b8f414c,
            0xa2355b1cd0643e51,
            0x0dcbfbc6869b0479,
            0xe641bba8b36df2bb,
            0x912f67de6084b4d9,
        ],
    ),
    (
        "large_seed_fixed_reference",
        [
            0x069aee93a25d9910,
            0x825d91d16e72df73,
            0x3ddafaaa5a181206,
            0x413c78b0789dfa10,
            0xfee689907416bcc1,
            0x709541b97c8cdca5,
            0x20dd6aaed63d7c0e,
            0x98de7bf5ba5ee88e,
            0xa24e1cc597b0f6fe,
            0x7e9b2fdcf00985e9,
            0xa02e3e8abed719ca,
            0x563806c97581438b,
            0xb537d93eb372d04c,
            0x0477158f303220a1,
            0x12164469cca2bea6,
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

fn fingerprint(map: &WorldMap) -> [u64; 15] {
    [
        hash_f32(&map.elevation),
        hash_f32(&map.ridge_influence),
        hash_f32(&map.temperature),
        hash_f32(&map.rainfall),
        fnv1a(map.water_mask.iter().map(|&w| u8::from(w))),
        fnv1a(map.dist_to_water.iter().flat_map(|d| d.to_le_bytes())),
        fnv1a(map.biome.iter().flat_map(|&b| biome_to_id(b).to_le_bytes())),
        hash_f32(&map.temperature_c),
        hash_f32(&map.rainfall_mm),
        hash_f32(&map.discharge_m3s),
        fnv1a(map.flow_direction.iter().copied()),
        fnv1a(map.basin_id.iter().flat_map(|b| b.to_le_bytes())),
        fnv1a(map.river_mask.iter().map(|&r| u8::from(r))),
        fnv1a(map.dist_to_river.iter().flat_map(|d| d.to_le_bytes())),
        hash_f32(&map.water_surface_m),
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
    let mut tropical_easterly = sized(7, 128, 128);
    tropical_easterly.regional_climate.latitude_deg = 12.0;
    tropical_easterly.regional_climate.wind_from_deg = 75.0;

    let mut southern_diagonal = sized(99, 128, 128);
    southern_diagonal.regional_climate.latitude_deg = -48.0;
    southern_diagonal.regional_climate.wind_from_deg = 225.0;

    vec![
        ("default_256", sized(42, 256, 256)),
        ("non_square_200x96", sized(3, 200, 96)),
        ("tropical_easterly", tropical_easterly),
        ("southern_diagonal", southern_diagonal),
        (
            "large_seed_fixed_reference",
            WorldGenConfig {
                land_reference_extent_m: Some(Meters(204_800.0)),
                target_land_fraction: Some(0.7),
                ..sized(u64::MAX - 5, 128, 128)
            },
        ),
    ]
}

#[test]
fn generator_output_is_unchanged() {
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
            assert_eq!(got, want, "{name}: layer `{layer}` changed\n{table}");
        }
    }
}
