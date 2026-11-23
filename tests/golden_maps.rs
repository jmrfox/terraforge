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
            0xf156e8cf9b91969c,
            0x5501a45141791335,
            0x0092ea6ab830b5dd,
            0xec7bc49885eb64df,
            0x16ba2d0c35a68ade,
            0xfc3578b2b4f0f6fc,
            0xcb29f0d60d8f7301,
            0x641ac86ef7d0d1be,
            0x0786084895299db1,
            0x4d0b3d055d09b9d0,
            0xece6b58a8cfdc7d2,
            0x270081c2c73c5e96,
        ],
    ),
    (
        "non_square_200x96",
        [
            0x06a88811d8a9132b,
            0xc5162ad21fa061ed,
            0x3759d578539a1a0a,
            0xfcab5e7e55f395ad,
            0x441a21fed0f81365,
            0xb737bfd75f60d0a3,
            0x9b32e108c1ba7732,
            0x279e94d037c5baa1,
            0x53768f4685bd9459,
            0x719926f79ed669d3,
            0x0c511c8a9f26e575,
            0x57f4ea32abf21659,
            0xd92181ac5d95d34d,
            0x8696ebdb2c7e9baa,
            0x0396a9f239ed1566,
        ],
    ),
    (
        "tropical_easterly",
        [
            0x7beea321c6312560,
            0xcfc9e0f0a5ce3570,
            0x1df7a34fee457143,
            0x50f969ab0f673d63,
            0x268aade838264232,
            0x9ee7cde6f169697f,
            0x97482171bfa40b85,
            0x73b9b97f19969cc5,
            0x4a32ebe646e6fa87,
            0x8250228b9df41b99,
            0xd951286749991dc3,
            0x7cecfba567fb1342,
            0x9c1bda7f8c872325,
            0xb9f13a0aa87f2325,
            0x763d327e569cad3c,
        ],
    ),
    (
        "southern_diagonal",
        [
            0x48f598b2281f87d9,
            0x559538b87462a048,
            0xb5f1b832c857f62e,
            0xa6bd087ce909e6d9,
            0x92e603c22f4cc8a7,
            0x4a6294f3957e6207,
            0xed8b253eb350e164,
            0x4858e9d7449d1512,
            0xb54cf70d64da97cc,
            0x78e52f21b89b823e,
            0xa874ebb70b8f414c,
            0xa2355b1cd0643e51,
            0x1f63f9fd125450e2,
            0x7383cd433a470822,
            0x0d24fe2bf068ca07,
        ],
    ),
    (
        "large_seed_fixed_reference",
        [
            0x069aee93a25d9910,
            0x825d91d16e72df73,
            0x3ddafaaa5a181206,
            0x7f7ff79dcd2196ab,
            0xfee689907416bcc1,
            0x709541b97c8cdca5,
            0x0624b6edcbeb4f3d,
            0x98de7bf5ba5ee88e,
            0xdd068b81a600829e,
            0x64c16fda587d6c86,
            0xa02e3e8abed719ca,
            0x563806c97581438b,
            0xe338cf7d52670bf9,
            0x9e93807047b84d41,
            0x1259013bd648a122,
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
