//! Independent noise seeds derived from the map seed.

/// Noise streams used by the pipeline.
#[derive(Clone, Copy)]
pub(crate) enum NoiseStream {
    Continent = 1,
    Detail,
    Ridge,
    DetailEnvelope,
    RidgeEnvelope,
    Temperature,
    Rainfall,
}

/// Hash the full 64-bit seed into a stream seed.
///
/// The `noise` crate seeds each octave with `seed + octave`, so the result stays far
/// enough below `u32::MAX` that the addition cannot overflow.
pub(crate) fn stream_seed(seed: u64, stream: NoiseStream) -> u32 {
    let mut z = seed.wrapping_add((stream as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    ((z >> 32) as u32) % (u32::MAX - 64)
}
