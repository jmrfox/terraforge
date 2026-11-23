//! Lossless compressed storage for generated maps (`.tfmap`).
//!
//! A file holds a short uncompressed header (magic, format version, compressed length,
//! and checksum) followed by one zlib stream containing map metadata, the generation
//! config as JSON, and the map layers. The length makes the format self-delimiting, so
//! a map can be embedded in a larger save stream.
//!
//! Numeric layers are delta-coded and split into byte planes before compression, because
//! neighboring cells have similar values. A layer is omitted only when it can be rebuilt
//! bit-for-bit.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::climate::{normalized_rainfall, normalized_temperature};
use crate::config::WorldGenConfig;
use crate::grid_ops::{chamfer_distance, chamfer_distance_water};
use crate::preview::{biome_from_id, biome_to_id};
use crate::world::{Biome, WorldMap, FLOW_OFFSETS};

/// File extension for stored maps, without the dot.
pub const MAP_FILE_EXTENSION: &str = "tfmap";
/// Current `.tfmap` format version.
pub const MAP_FILE_VERSION: u16 = 1;

const MAGIC: [u8; 4] = *b"TFMP";
/// Rejects corrupt headers before they can request an enormous allocation.
const MAX_CELLS: u64 = 1 << 28;
const MAX_CONFIG_BYTES: u32 = 1 << 20;

/// A map loaded from storage, with the config that generated it.
#[derive(Debug, Clone)]
pub struct StoredMap {
    pub map: WorldMap,
    pub config: WorldGenConfig,
}

/// Failure while reading or writing a stored map.
#[derive(Debug)]
pub enum MapFileError {
    Io(io::Error),
    /// The data is not a valid map file, or it is truncated or corrupt.
    InvalidFile(String),
    UnsupportedVersion(u16),
    /// The map and config passed for writing do not describe the same world.
    InconsistentMap(String),
}

impl fmt::Display for MapFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "map file I/O error: {e}"),
            Self::InvalidFile(reason) => write!(f, "invalid map file: {reason}"),
            Self::UnsupportedVersion(version) => write!(
                f,
                "map file format {version} is not supported (this build reads format {MAP_FILE_VERSION})"
            ),
            Self::InconsistentMap(reason) => write!(f, "inconsistent map: {reason}"),
        }
    }
}

impl std::error::Error for MapFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for MapFileError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::InvalidData
            | io::ErrorKind::InvalidInput => Self::InvalidFile(e.to_string()),
            _ => Self::Io(e),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Elevation = 0,
    RidgeInfluence = 1,
    Biome = 2,
    TemperatureC = 3,
    RainfallMm = 4,
    Temperature = 5,
    Rainfall = 6,
    WaterMask = 7,
    DistToWater = 8,
    Discharge = 9,
    FlowDirection = 10,
    BasinId = 11,
    RiverMask = 12,
    DistToRiver = 13,
    /// Water surface XORed with the dry-land surface, so only lakes hold nonzero values.
    WaterSurfaceDelta = 14,
}

impl Layer {
    fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Self::Elevation,
            1 => Self::RidgeInfluence,
            2 => Self::Biome,
            3 => Self::TemperatureC,
            4 => Self::RainfallMm,
            5 => Self::Temperature,
            6 => Self::Rainfall,
            7 => Self::WaterMask,
            8 => Self::DistToWater,
            9 => Self::Discharge,
            10 => Self::FlowDirection,
            11 => Self::BasinId,
            12 => Self::RiverMask,
            13 => Self::DistToRiver,
            14 => Self::WaterSurfaceDelta,
            _ => return None,
        })
    }

    fn byte_len(self, cells: usize) -> usize {
        match self {
            Self::Biome | Self::WaterMask | Self::FlowDirection | Self::RiverMask => cells,
            _ => cells * 4,
        }
    }
}

/// Delta from the left neighbor (or the cell above at row starts), split into byte planes.
fn encode_words(words: &[u32], width: usize) -> Vec<u8> {
    let n = words.len();
    let mut out = vec![0u8; n * 4];
    for (i, &word) in words.iter().enumerate() {
        let predicted = match i {
            0 => 0,
            _ if i % width == 0 => words[i - width],
            _ => words[i - 1],
        };
        let delta = word.wrapping_sub(predicted).to_le_bytes();
        for (plane, byte) in delta.into_iter().enumerate() {
            out[plane * n + i] = byte;
        }
    }
    out
}

fn decode_words(bytes: &[u8], width: usize) -> Vec<u32> {
    let n = bytes.len() / 4;
    let mut words = vec![0u32; n];
    for i in 0..n {
        let delta =
            u32::from_le_bytes([bytes[i], bytes[n + i], bytes[2 * n + i], bytes[3 * n + i]]);
        let predicted = match i {
            0 => 0,
            _ if i % width == 0 => words[i - width],
            _ => words[i - 1],
        };
        words[i] = predicted.wrapping_add(delta);
    }
    words
}

fn encode_floats(values: &[f32], width: usize) -> Vec<u8> {
    let words: Vec<u32> = values.iter().map(|v| v.to_bits()).collect();
    encode_words(&words, width)
}

fn decode_floats(bytes: &[u8], width: usize) -> Vec<f32> {
    decode_words(bytes, width)
        .into_iter()
        .map(f32::from_bits)
        .collect()
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn temperature_from_celsius(temperature_c: &[f32]) -> Vec<f32> {
    temperature_c
        .iter()
        .map(|&c| normalized_temperature(c))
        .collect()
}

fn rainfall_from_mm(rainfall_mm: &[f32], water: &[bool]) -> Vec<f32> {
    rainfall_mm
        .iter()
        .zip(water)
        .map(|(&mm, &is_water)| normalized_rainfall(mm, is_water))
        .collect()
}

fn water_from_biomes(biomes: &[Biome]) -> Vec<bool> {
    biomes
        .iter()
        .map(|b| matches!(b, Biome::Ocean | Biome::Lake))
        .collect()
}

fn rivers_from_discharge(discharge: &[f32], water: &[bool], min_discharge: f64) -> Vec<bool> {
    discharge
        .iter()
        .zip(water)
        .map(|(&q, &is_water)| !is_water && f64::from(q) >= min_discharge)
        .collect()
}

fn river_distances(width: usize, height: usize, rivers: &[bool]) -> Vec<u32> {
    chamfer_distance(width, height, |i| rivers[i])
}

/// Surface of each cell if it held no lake: sea level on the ocean, otherwise the terrain.
fn dry_surface_bits(
    elevation: &[f32],
    biome: &[Biome],
    ocean_floor_m: f64,
    max_elevation_m: f64,
    sea_level_m: f64,
) -> Vec<u32> {
    let span = (max_elevation_m - ocean_floor_m).max(1.0);
    elevation
        .iter()
        .zip(biome)
        .map(|(&e, &b)| {
            let meters = if b == Biome::Ocean {
                sea_level_m
            } else {
                ocean_floor_m + f64::from(e) * span
            };
            (meters as f32).to_bits()
        })
        .collect()
}

fn water_surface_delta(map: &WorldMap) -> Vec<u32> {
    dry_surface_bits(
        &map.elevation,
        &map.biome,
        map.ocean_floor_m,
        map.max_elevation_m,
        map.sea_level_m,
    )
    .into_iter()
    .zip(&map.water_surface_m)
    .map(|(dry, surface)| dry ^ surface.to_bits())
    .collect()
}

/// Layers this map must store because they cannot be rebuilt exactly from the others.
fn layers_to_store(map: &WorldMap, config: &WorldGenConfig) -> Vec<Layer> {
    let mut layers = vec![
        Layer::Elevation,
        Layer::RidgeInfluence,
        Layer::Biome,
        Layer::TemperatureC,
        Layer::RainfallMm,
        Layer::Discharge,
        Layer::FlowDirection,
        Layer::BasinId,
        Layer::WaterSurfaceDelta,
    ];
    if map.river_mask
        != rivers_from_discharge(
            &map.discharge_m3s,
            &map.water_mask,
            config.hydrology.river_min_discharge_m3s,
        )
    {
        layers.push(Layer::RiverMask);
    }
    if map.dist_to_river != river_distances(map.width, map.height, &map.river_mask) {
        layers.push(Layer::DistToRiver);
    }
    if !same_bits(
        &map.temperature,
        &temperature_from_celsius(&map.temperature_c),
    ) {
        layers.push(Layer::Temperature);
    }
    if !same_bits(
        &map.rainfall,
        &rainfall_from_mm(&map.rainfall_mm, &map.water_mask),
    ) {
        layers.push(Layer::Rainfall);
    }
    if map.water_mask != water_from_biomes(&map.biome) {
        layers.push(Layer::WaterMask);
    }
    if map.dist_to_water != chamfer_distance_water(map.width, map.height, &map.water_mask) {
        layers.push(Layer::DistToWater);
    }
    layers
}

fn encode_layer(map: &WorldMap, layer: Layer) -> Vec<u8> {
    let w = map.width;
    match layer {
        Layer::Elevation => encode_floats(&map.elevation, w),
        Layer::RidgeInfluence => encode_floats(&map.ridge_influence, w),
        Layer::TemperatureC => encode_floats(&map.temperature_c, w),
        Layer::RainfallMm => encode_floats(&map.rainfall_mm, w),
        Layer::Temperature => encode_floats(&map.temperature, w),
        Layer::Rainfall => encode_floats(&map.rainfall, w),
        Layer::DistToWater => encode_words(&map.dist_to_water, w),
        Layer::WaterMask => map.water_mask.iter().map(|&v| u8::from(v)).collect(),
        Layer::Biome => map.biome.iter().map(|&b| biome_to_id(b) as u8).collect(),
        Layer::Discharge => encode_floats(&map.discharge_m3s, w),
        Layer::FlowDirection => map.flow_direction.clone(),
        Layer::BasinId => encode_words(&map.basin_id, w),
        Layer::RiverMask => map.river_mask.iter().map(|&v| u8::from(v)).collect(),
        Layer::DistToRiver => encode_words(&map.dist_to_river, w),
        Layer::WaterSurfaceDelta => encode_words(&water_surface_delta(map), w),
    }
}

fn check_consistent(map: &WorldMap, config: &WorldGenConfig) -> Result<(), MapFileError> {
    let inconsistent = |reason: String| Err(MapFileError::InconsistentMap(reason));
    if (map.width, map.height, map.seed) != (config.width, config.height, config.seed) {
        return inconsistent("width, height, and seed must match the config".to_string());
    }
    let cells = (map.width as u64).checked_mul(map.height as u64);
    if map.width == 0 || map.height == 0 || cells.is_none_or(|c| c > MAX_CELLS) {
        return inconsistent(format!("unsupported size {}×{}", map.width, map.height));
    }
    let n = map.width * map.height;
    let required = [
        ("elevation", map.elevation.len()),
        ("ridge_influence", map.ridge_influence.len()),
        ("temperature", map.temperature.len()),
        ("rainfall", map.rainfall.len()),
        ("temperature_c", map.temperature_c.len()),
        ("rainfall_mm", map.rainfall_mm.len()),
        ("water_mask", map.water_mask.len()),
        ("dist_to_water", map.dist_to_water.len()),
        ("discharge_m3s", map.discharge_m3s.len()),
        ("flow_direction", map.flow_direction.len()),
        ("basin_id", map.basin_id.len()),
        ("river_mask", map.river_mask.len()),
        ("dist_to_river", map.dist_to_river.len()),
        ("water_surface_m", map.water_surface_m.len()),
        ("biome", map.biome.len()),
    ];
    for (name, len) in required {
        if len != n {
            return inconsistent(format!("{name} has {len} cells, expected {n}"));
        }
    }
    Ok(())
}

/// Write a map and its config to any writer in `.tfmap` format.
pub fn write_stored_map<W: Write>(
    writer: W,
    map: &WorldMap,
    config: &WorldGenConfig,
) -> Result<(), MapFileError> {
    check_consistent(map, config)?;
    write_layers(writer, map, config, &layers_to_store(map, config))
}

fn write_layers<W: Write>(
    mut writer: W,
    map: &WorldMap,
    config: &WorldGenConfig,
    layers: &[Layer],
) -> Result<(), MapFileError> {
    let config_json = serde_json::to_vec(config)
        .map_err(|e| MapFileError::InconsistentMap(format!("config: {e}")))?;
    if config_json.len() > MAX_CONFIG_BYTES as usize {
        return Err(MapFileError::InconsistentMap(
            "config is too large".to_string(),
        ));
    }

    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&(map.width as u64).to_le_bytes())?;
    z.write_all(&(map.height as u64).to_le_bytes())?;
    z.write_all(&map.seed.to_le_bytes())?;
    z.write_all(&map.generator_version.to_le_bytes())?;
    for value in [
        map.cell_size_m,
        map.ocean_floor_m,
        map.max_elevation_m,
        map.sea_level_m,
    ] {
        z.write_all(&value.to_bits().to_le_bytes())?;
    }
    z.write_all(&(config_json.len() as u32).to_le_bytes())?;
    z.write_all(&config_json)?;

    z.write_all(&[layers.len() as u8])?;
    for &layer in layers {
        z.write_all(&[layer as u8])?;
        z.write_all(&encode_layer(map, layer))?;
    }
    let compressed = z.finish()?;

    writer.write_all(&MAGIC)?;
    writer.write_all(&MAP_FILE_VERSION.to_le_bytes())?;
    writer.write_all(&(compressed.len() as u64).to_le_bytes())?;
    writer.write_all(&checksum(&compressed).to_le_bytes())?;
    writer.write_all(&compressed)?;
    writer.flush()?;
    Ok(())
}

/// FNV-1a over the compressed payload, to detect truncation and accidental corruption.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn read_array<const N: usize, R: Read>(reader: &mut R) -> Result<[u8; N], MapFileError> {
    let mut bytes = [0u8; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Reads exactly `len` bytes, growing only as data actually arrives.
fn read_payload<R: Read>(reader: &mut R, len: usize) -> Result<Vec<u8>, MapFileError> {
    let mut bytes = Vec::new();
    reader.by_ref().take(len as u64).read_to_end(&mut bytes)?;
    if bytes.len() != len {
        return Err(invalid("file ends before the data is complete"));
    }
    Ok(bytes)
}

fn invalid(reason: &str) -> MapFileError {
    MapFileError::InvalidFile(reason.to_string())
}

#[derive(Default)]
struct LoadedLayers {
    elevation: Option<Vec<f32>>,
    ridge_influence: Option<Vec<f32>>,
    biome: Option<Vec<Biome>>,
    temperature_c: Option<Vec<f32>>,
    rainfall_mm: Option<Vec<f32>>,
    temperature: Option<Vec<f32>>,
    rainfall: Option<Vec<f32>>,
    water_mask: Option<Vec<bool>>,
    dist_to_water: Option<Vec<u32>>,
    discharge: Option<Vec<f32>>,
    flow_direction: Option<Vec<u8>>,
    basin_id: Option<Vec<u32>>,
    river_mask: Option<Vec<bool>>,
    dist_to_river: Option<Vec<u32>>,
    water_surface_delta: Option<Vec<u32>>,
}

impl LoadedLayers {
    fn insert(&mut self, layer: Layer, bytes: &[u8], width: usize) -> Result<(), MapFileError> {
        fn set<T>(slot: &mut Option<T>, value: T) -> Result<(), MapFileError> {
            if slot.replace(value).is_some() {
                return Err(invalid("duplicate layer"));
            }
            Ok(())
        }
        match layer {
            Layer::Elevation => set(&mut self.elevation, decode_floats(bytes, width)),
            Layer::RidgeInfluence => set(&mut self.ridge_influence, decode_floats(bytes, width)),
            Layer::TemperatureC => set(&mut self.temperature_c, decode_floats(bytes, width)),
            Layer::RainfallMm => set(&mut self.rainfall_mm, decode_floats(bytes, width)),
            Layer::Temperature => set(&mut self.temperature, decode_floats(bytes, width)),
            Layer::Rainfall => set(&mut self.rainfall, decode_floats(bytes, width)),
            Layer::DistToWater => set(&mut self.dist_to_water, decode_words(bytes, width)),
            Layer::WaterMask => {
                if bytes.iter().any(|&b| b > 1) {
                    return Err(invalid("water mask contains an invalid value"));
                }
                set(
                    &mut self.water_mask,
                    bytes.iter().map(|&b| b == 1).collect(),
                )
            }
            Layer::Biome => {
                let biomes = bytes
                    .iter()
                    .map(|&id| biome_from_id(u16::from(id)))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| invalid("unknown biome id"))?;
                set(&mut self.biome, biomes)
            }
            Layer::Discharge => set(&mut self.discharge, decode_floats(bytes, width)),
            Layer::FlowDirection => {
                if bytes.iter().any(|&b| usize::from(b) > FLOW_OFFSETS.len()) {
                    return Err(invalid("flow direction contains an invalid value"));
                }
                set(&mut self.flow_direction, bytes.to_vec())
            }
            Layer::BasinId => set(&mut self.basin_id, decode_words(bytes, width)),
            Layer::RiverMask => {
                if bytes.iter().any(|&b| b > 1) {
                    return Err(invalid("river mask contains an invalid value"));
                }
                set(
                    &mut self.river_mask,
                    bytes.iter().map(|&b| b == 1).collect(),
                )
            }
            Layer::DistToRiver => set(&mut self.dist_to_river, decode_words(bytes, width)),
            Layer::WaterSurfaceDelta => {
                set(&mut self.water_surface_delta, decode_words(bytes, width))
            }
        }
    }
}

/// Read a map and its config from any reader containing `.tfmap` data. Bytes after the
/// map are left unread.
pub fn read_stored_map<R: Read>(mut reader: R) -> Result<StoredMap, MapFileError> {
    if read_array::<4, _>(&mut reader)? != MAGIC {
        return Err(invalid("not a Terraforge map"));
    }
    let version = u16::from_le_bytes(read_array(&mut reader)?);
    if version != MAP_FILE_VERSION {
        return Err(MapFileError::UnsupportedVersion(version));
    }

    let compressed_len = u64::from_le_bytes(read_array(&mut reader)?);
    let expected_checksum = u64::from_le_bytes(read_array(&mut reader)?);
    let mut compressed = Vec::new();
    reader
        .by_ref()
        .take(compressed_len)
        .read_to_end(&mut compressed)?;
    if compressed.len() as u64 != compressed_len {
        return Err(invalid("file ends before the data is complete"));
    }
    if checksum(&compressed) != expected_checksum {
        return Err(invalid("checksum mismatch"));
    }

    let mut z = ZlibDecoder::new(compressed.as_slice());
    let width = u64::from_le_bytes(read_array(&mut z)?);
    let height = u64::from_le_bytes(read_array(&mut z)?);
    if width == 0
        || height == 0
        || width
            .checked_mul(height)
            .is_none_or(|cells| cells > MAX_CELLS)
    {
        return Err(invalid("unsupported map size"));
    }
    let (width, height) = (width as usize, height as usize);
    let cells = width * height;

    let seed = u64::from_le_bytes(read_array(&mut z)?);
    let generator_version = u32::from_le_bytes(read_array(&mut z)?);
    let mut read_f64 = || -> Result<f64, MapFileError> {
        Ok(f64::from_bits(u64::from_le_bytes(read_array(&mut z)?)))
    };
    let cell_size_m = read_f64()?;
    let ocean_floor_m = read_f64()?;
    let max_elevation_m = read_f64()?;
    let sea_level_m = read_f64()?;

    let config_len = u32::from_le_bytes(read_array(&mut z)?);
    if config_len > MAX_CONFIG_BYTES {
        return Err(invalid("config is too large"));
    }
    let config_json = read_payload(&mut z, config_len as usize)?;
    let config: WorldGenConfig = serde_json::from_slice(&config_json)
        .map_err(|e| MapFileError::InvalidFile(format!("config: {e}")))?;
    if (config.width, config.height, config.seed) != (width, height, seed) {
        return Err(invalid("config does not match the stored map"));
    }

    let mut loaded = LoadedLayers::default();
    let [layer_count] = read_array::<1, _>(&mut z)?;
    for _ in 0..layer_count {
        let [id] = read_array::<1, _>(&mut z)?;
        let layer = Layer::from_id(id).ok_or_else(|| invalid("unknown layer"))?;
        let bytes = read_payload(&mut z, layer.byte_len(cells))?;
        loaded.insert(layer, &bytes, width)?;
    }

    let mut trailing = [0u8; 1];
    if z.read(&mut trailing)? != 0 {
        return Err(invalid("unexpected data after the last layer"));
    }

    let missing = |name: &str| MapFileError::InvalidFile(format!("missing {name} layer"));
    let biome = loaded.biome.ok_or_else(|| missing("biome"))?;
    let water_mask = loaded
        .water_mask
        .unwrap_or_else(|| water_from_biomes(&biome));
    let temperature_c = loaded.temperature_c.ok_or_else(|| missing("temperature"))?;
    let rainfall_mm = loaded.rainfall_mm.ok_or_else(|| missing("rainfall"))?;
    let temperature = loaded
        .temperature
        .unwrap_or_else(|| temperature_from_celsius(&temperature_c));
    let rainfall = loaded
        .rainfall
        .unwrap_or_else(|| rainfall_from_mm(&rainfall_mm, &water_mask));
    let dist_to_water = loaded
        .dist_to_water
        .unwrap_or_else(|| chamfer_distance_water(width, height, &water_mask));

    // Files without hydrology load with empty layers, and `add_hydrology` can fill them.
    let elevation = loaded.elevation.ok_or_else(|| missing("elevation"))?;
    let river_mask = match (loaded.river_mask, &loaded.discharge) {
        (Some(mask), _) => mask,
        (None, Some(discharge)) => rivers_from_discharge(
            discharge,
            &water_mask,
            config.hydrology.river_min_discharge_m3s,
        ),
        (None, None) => vec![false; cells],
    };
    let dist_to_river = loaded
        .dist_to_river
        .unwrap_or_else(|| river_distances(width, height, &river_mask));
    let surface_delta = loaded.water_surface_delta.unwrap_or_else(|| vec![0; cells]);
    let water_surface_m = dry_surface_bits(
        &elevation,
        &biome,
        ocean_floor_m,
        max_elevation_m,
        sea_level_m,
    )
    .into_iter()
    .zip(surface_delta)
    .map(|(dry, delta)| f32::from_bits(dry ^ delta))
    .collect();

    let map = WorldMap {
        width,
        height,
        seed,
        generator_version,
        cell_size_m,
        ocean_floor_m,
        max_elevation_m,
        sea_level_m,
        elevation,
        ridge_influence: loaded
            .ridge_influence
            .ok_or_else(|| missing("ridge influence"))?,
        temperature,
        rainfall,
        temperature_c,
        rainfall_mm,
        water_mask,
        dist_to_water,
        discharge_m3s: loaded.discharge.unwrap_or_else(|| vec![0.0; cells]),
        flow_direction: loaded.flow_direction.unwrap_or_else(|| vec![0; cells]),
        basin_id: loaded.basin_id.unwrap_or_else(|| vec![0; cells]),
        river_mask,
        dist_to_river,
        water_surface_m,
        biome,
    };
    Ok(StoredMap { map, config })
}

/// Save a map to a `.tfmap` file. The file is replaced only after a complete write.
pub fn save_map_file(
    path: impl AsRef<Path>,
    map: &WorldMap,
    config: &WorldGenConfig,
) -> Result<(), MapFileError> {
    let path = path.as_ref();
    let file_name = path.file_name().ok_or_else(|| {
        MapFileError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "map path has no file name",
        ))
    })?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    let mut temp_name = OsString::from(file_name);
    temp_name.push(".tmp");
    let temp_path = path.with_file_name(temp_name);

    let result = (|| {
        let mut writer = BufWriter::new(File::create(&temp_path)?);
        write_stored_map(&mut writer, map, config)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Load a map from a `.tfmap` file.
pub fn load_map_file(path: impl AsRef<Path>) -> Result<StoredMap, MapFileError> {
    read_stored_map(BufReader::new(File::open(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_world;

    fn sample_config() -> WorldGenConfig {
        WorldGenConfig {
            seed: 42,
            width: 128,
            height: 96,
            ..WorldGenConfig::default()
        }
    }

    fn encode(map: &WorldMap, config: &WorldGenConfig) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_stored_map(&mut bytes, map, config).unwrap();
        bytes
    }

    fn assert_identical(a: &WorldMap, b: &WorldMap) {
        assert_eq!(
            (a.width, a.height, a.seed, a.generator_version),
            (b.width, b.height, b.seed, b.generator_version)
        );
        for (x, y) in [
            (a.cell_size_m, b.cell_size_m),
            (a.ocean_floor_m, b.ocean_floor_m),
            (a.max_elevation_m, b.max_elevation_m),
            (a.sea_level_m, b.sea_level_m),
        ] {
            assert_eq!(x.to_bits(), y.to_bits());
        }
        for (x, y) in [
            (&a.elevation, &b.elevation),
            (&a.ridge_influence, &b.ridge_influence),
            (&a.temperature, &b.temperature),
            (&a.rainfall, &b.rainfall),
            (&a.temperature_c, &b.temperature_c),
            (&a.rainfall_mm, &b.rainfall_mm),
            (&a.discharge_m3s, &b.discharge_m3s),
            (&a.water_surface_m, &b.water_surface_m),
        ] {
            assert!(same_bits(x, y));
        }
        assert_eq!(a.water_mask, b.water_mask);
        assert_eq!(a.dist_to_water, b.dist_to_water);
        assert_eq!(a.flow_direction, b.flow_direction);
        assert_eq!(a.basin_id, b.basin_id);
        assert_eq!(a.river_mask, b.river_mask);
        assert_eq!(a.dist_to_river, b.dist_to_river);
        assert_eq!(a.biome, b.biome);
    }

    #[test]
    fn round_trips_exactly() {
        let config = sample_config();
        let map = generate_world(&config);
        let stored = read_stored_map(encode(&map, &config).as_slice()).unwrap();
        assert_identical(&map, &stored.map);
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::to_value(&stored.config).unwrap()
        );
    }

    #[test]
    fn omits_layers_it_can_rebuild() {
        let config = sample_config();
        let map = generate_world(&config);
        assert_eq!(
            layers_to_store(&map, &config),
            [
                Layer::Elevation,
                Layer::RidgeInfluence,
                Layer::Biome,
                Layer::TemperatureC,
                Layer::RainfallMm,
                Layer::Discharge,
                Layer::FlowDirection,
                Layer::BasinId,
                Layer::WaterSurfaceDelta,
            ]
        );
    }

    #[test]
    fn file_without_hydrology_can_be_upgraded() {
        let config = sample_config();
        let map = generate_world(&config);
        let mut bytes = Vec::new();
        let layers = [
            Layer::Elevation,
            Layer::RidgeInfluence,
            Layer::Biome,
            Layer::TemperatureC,
            Layer::RainfallMm,
        ];
        write_layers(&mut bytes, &map, &config, &layers).unwrap();

        let mut stored = read_stored_map(bytes.as_slice()).unwrap();
        assert!(stored.map.discharge_m3s.iter().all(|&q| q == 0.0));
        assert!(!stored.map.river_mask.contains(&true));

        crate::add_hydrology(&mut stored.map, &stored.config);
        assert_identical(&map, &stored.map);
    }

    #[test]
    fn edited_derived_layers_are_preserved() {
        let config = sample_config();
        let mut map = generate_world(&config);
        map.temperature[5] = 0.123;
        map.rainfall[6] = 0.456;
        map.dist_to_water[7] += 3;
        map.water_mask[8] = !map.water_mask[8];
        map.river_mask[9] = !map.river_mask[9];
        map.dist_to_river[10] = 3;
        map.water_surface_m[11] = 1234.5;
        let stored = read_stored_map(encode(&map, &config).as_slice()).unwrap();
        assert_identical(&map, &stored.map);
    }

    #[test]
    fn extreme_float_bits_survive() {
        let config = sample_config();
        let mut map = generate_world(&config);
        map.elevation[0] = f32::from_bits(0x7FC0_1234);
        map.elevation[1] = -0.0;
        map.elevation[2] = f32::MIN_POSITIVE / 2.0;
        map.elevation[3] = f32::INFINITY;
        let stored = read_stored_map(encode(&map, &config).as_slice()).unwrap();
        assert!(same_bits(&map.elevation, &stored.map.elevation));
    }

    #[test]
    fn compresses_well_below_raw_size() {
        let config = WorldGenConfig {
            width: 256,
            height: 256,
            ..WorldGenConfig::default()
        };
        let map = generate_world(&config);
        // Eight float layers, three 32-bit layers, and four byte layers.
        let raw = 256 * 256 * (8 * 4 + 3 * 4 + 4);
        let size = encode(&map, &config).len();
        assert!(size * 3 < raw, "{size} bytes from {raw}");
    }

    #[test]
    fn rejects_wrong_magic_and_version() {
        let config = sample_config();
        let bytes = encode(&generate_world(&config), &config);

        let mut wrong_magic = bytes.clone();
        wrong_magic[0] = b'X';
        assert!(matches!(
            read_stored_map(wrong_magic.as_slice()),
            Err(MapFileError::InvalidFile(_))
        ));

        let mut wrong_version = bytes;
        wrong_version[4] = 99;
        assert!(matches!(
            read_stored_map(wrong_version.as_slice()),
            Err(MapFileError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn rejects_truncated_data() {
        let config = WorldGenConfig {
            width: 32,
            height: 32,
            ..sample_config()
        };
        let bytes = encode(&generate_world(&config), &config);
        for len in (0..bytes.len()).step_by(bytes.len() / 97 + 1) {
            assert!(read_stored_map(&bytes[..len]).is_err(), "prefix {len}");
        }
        assert!(read_stored_map(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn flipped_bytes_never_yield_a_different_map() {
        let config = WorldGenConfig {
            width: 32,
            height: 32,
            ..sample_config()
        };
        let map = generate_world(&config);
        let bytes = encode(&map, &config);
        for pos in (0..bytes.len()).step_by(bytes.len() / 211 + 1) {
            let mut corrupt = bytes.clone();
            corrupt[pos] ^= 0x40;
            assert!(read_stored_map(corrupt.as_slice()).is_err(), "byte {pos}");
        }
    }

    #[test]
    fn rejects_huge_declared_size_without_allocating() {
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&u64::MAX.to_le_bytes()).unwrap();
        z.write_all(&2u64.to_le_bytes()).unwrap();
        let compressed = z.finish().unwrap();

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&MAP_FILE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&checksum(&compressed).to_le_bytes());
        bytes.extend_from_slice(&compressed);
        assert!(matches!(
            read_stored_map(bytes.as_slice()),
            Err(MapFileError::InvalidFile(_))
        ));

        let mut huge_payload = Vec::new();
        huge_payload.extend_from_slice(&MAGIC);
        huge_payload.extend_from_slice(&MAP_FILE_VERSION.to_le_bytes());
        huge_payload.extend_from_slice(&u64::MAX.to_le_bytes());
        huge_payload.extend_from_slice(&0u64.to_le_bytes());
        assert!(matches!(
            read_stored_map(huge_payload.as_slice()),
            Err(MapFileError::InvalidFile(_))
        ));
    }

    #[test]
    fn maps_can_be_embedded_in_a_larger_stream() {
        let first = sample_config();
        let second = WorldGenConfig {
            seed: 7,
            ..first.clone()
        };
        let (map_a, map_b) = (generate_world(&first), generate_world(&second));

        let mut stream = Vec::new();
        write_stored_map(&mut stream, &map_a, &first).unwrap();
        write_stored_map(&mut stream, &map_b, &second).unwrap();
        stream.extend_from_slice(b"game data");

        let mut reader = stream.as_slice();
        assert_identical(&map_a, &read_stored_map(&mut reader).unwrap().map);
        assert_identical(&map_b, &read_stored_map(&mut reader).unwrap().map);
        assert_eq!(reader, b"game data");
    }

    #[test]
    fn write_rejects_mismatched_config() {
        let config = sample_config();
        let map = generate_world(&config);
        let other = WorldGenConfig { seed: 43, ..config };
        assert!(matches!(
            write_stored_map(Vec::new(), &map, &other),
            Err(MapFileError::InconsistentMap(_))
        ));
    }

    #[test]
    fn file_helpers_round_trip_and_replace() {
        let dir = std::env::temp_dir().join(format!("terraforge_map_{}", std::process::id()));
        let path = dir.join("nested").join("world.tfmap");
        let first = sample_config();
        let second = WorldGenConfig {
            seed: 7,
            ..first.clone()
        };

        let map = generate_world(&first);
        save_map_file(&path, &map, &first).unwrap();
        assert_identical(&map, &load_map_file(&path).unwrap().map);

        let map = generate_world(&second);
        save_map_file(&path, &map, &second).unwrap();
        assert_identical(&map, &load_map_file(&path).unwrap().map);
        assert!(!path.with_file_name("world.tfmap.tmp").exists());

        fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(load_map_file(&path), Err(MapFileError::Io(_))));
    }
}
