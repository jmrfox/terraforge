use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Seek, Write};
use std::path::Path;

use rayon::prelude::*;
use serde::Serialize;
use tiff::encoder::{colortype, TiffEncoder};
use tiff::tags::Tag;

use super::colors::biome_rgba;
use super::config::WorldGenConfig;
use super::world::{Biome, WorldMap};

/// Output format for map raster export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapExportFormat {
    #[default]
    Png,
    Tiff,
}

impl MapExportFormat {
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
        {
            Some(ext) if ext == "tiff" || ext == "tif" => Self::Tiff,
            _ => Self::Png,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Tiff => "tiff",
        }
    }
}

/// Which raster bands to include in a multi-page TIFF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TiffLayerSet {
    pub biomes: bool,
    pub elevation: bool,
    pub temperature: bool,
    pub rainfall: bool,
    pub biome_id: bool,
    pub water: bool,
}

impl TiffLayerSet {
    /// Biome RGB preview + 16-bit elevation only (legacy two-page export).
    pub fn default_layers() -> Self {
        Self {
            biomes: true,
            elevation: true,
            temperature: false,
            rainfall: false,
            biome_id: false,
            water: false,
        }
    }

    /// All supported layers.
    pub fn full() -> Self {
        Self {
            biomes: true,
            elevation: true,
            temperature: true,
            rainfall: true,
            biome_id: true,
            water: true,
        }
    }

    /// Parse `default`, `full`/`all`, or a comma-separated layer list.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("tiff layer spec must not be empty".into());
        }
        if spec.eq_ignore_ascii_case("default") {
            return Ok(Self::default_layers());
        }
        if spec.eq_ignore_ascii_case("full") || spec.eq_ignore_ascii_case("all") {
            return Ok(Self::full());
        }

        let mut set = Self {
            biomes: false,
            elevation: false,
            temperature: false,
            rainfall: false,
            biome_id: false,
            water: false,
        };
        for part in spec.split(',') {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            match name.to_ascii_lowercase().as_str() {
                "biomes" | "biome" | "preview" => set.biomes = true,
                "elevation" | "elev" => set.elevation = true,
                "temperature" | "temp" => set.temperature = true,
                "rainfall" | "rain" => set.rainfall = true,
                "biome_id" | "biomeid" | "biomes_id" => set.biome_id = true,
                "water" | "water_mask" => set.water = true,
                other => {
                    return Err(format!(
                        "unknown TIFF layer '{other}' (expected biomes, elevation, temperature, rainfall, biome_id, water)"
                    ));
                }
            }
        }
        if !set.has_any() {
            return Err("tiff layer spec must include at least one layer".into());
        }
        Ok(set)
    }

    pub fn has_any(&self) -> bool {
        self.biomes
            || self.elevation
            || self.temperature
            || self.rainfall
            || self.biome_id
            || self.water
    }

    pub fn page_count(&self) -> usize {
        [
            self.biomes,
            self.elevation,
            self.temperature,
            self.rainfall,
            self.biome_id,
            self.water,
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count()
    }
}

impl Default for TiffLayerSet {
    fn default() -> Self {
        Self::full()
    }
}

/// Write a map, selecting TIFF pages via `tiff_layers` when format is TIFF.
pub fn write_map_with_tiff_layers(
    map: &WorldMap,
    path: &Path,
    format: MapExportFormat,
    tiff_layers: TiffLayerSet,
) -> io::Result<()> {
    match format {
        MapExportFormat::Png => write_map_png(map, path),
        MapExportFormat::Tiff => write_map_tiff(map, path, tiff_layers),
    }
}

/// Summary statistics for a generated map (written as JSON sidecar).
#[derive(Debug, Clone, Serialize)]
pub struct MapStats {
    pub config: WorldGenConfig,
    pub width: usize,
    pub height: usize,
    pub cell_size_m: f64,
    pub map_width_m: f64,
    pub map_height_m: f64,
    pub max_elevation_m: f64,
    pub sea_level_m: f64,
    pub land_fraction: f64,
    pub ocean_fraction: f64,
    pub biomes: HashMap<String, usize>,
    pub elapsed_ms: u64,
}

/// Preview layer for GUI / interactive viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewLayer {
    #[default]
    Biomes,
    Elevation,
    Temperature,
    Rainfall,
    Water,
}

impl PreviewLayer {
    pub const ALL: [Self; 5] = [
        Self::Biomes,
        Self::Elevation,
        Self::Temperature,
        Self::Rainfall,
        Self::Water,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Biomes => "Biomes",
            Self::Elevation => "Elevation",
            Self::Temperature => "Temperature",
            Self::Rainfall => "Rainfall",
            Self::Water => "Water",
        }
    }

    /// Short legend text for the right panel (none for biome grid legend).
    pub fn legend_hint(self) -> Option<&'static str> {
        match self {
            Self::Biomes => None,
            Self::Elevation => Some("Grayscale: low → high elevation"),
            Self::Temperature => Some("Grayscale: cold → warm"),
            Self::Rainfall => Some("Grayscale: dry → wet"),
            Self::Water => Some("Blue: ocean & lakes; tan: land"),
        }
    }
}

/// Rasterize a world map to RGBA8 for interactive preview.
pub fn map_to_preview_rgba8(map: &WorldMap, layer: PreviewLayer) -> Vec<u8> {
    match layer {
        PreviewLayer::Biomes => map_to_rgba8(map),
        PreviewLayer::Elevation => scalar_field_to_rgba8(&map.elevation),
        PreviewLayer::Temperature => scalar_field_to_rgba8(&map.temperature),
        PreviewLayer::Rainfall => scalar_field_to_rgba8(&map.rainfall),
        PreviewLayer::Water => water_to_rgba8(map),
    }
}

const WATER_BLUE: [u8; 4] = [32, 64, 140, 255];
const LAND_TAN: [u8; 4] = [160, 140, 100, 255];

fn water_to_rgba8(map: &WorldMap) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels.par_chunks_mut(4).enumerate().for_each(|(idx, px)| {
        let color = if map.water_mask[idx] {
            WATER_BLUE
        } else {
            LAND_TAN
        };
        px.copy_from_slice(&color);
    });
    pixels
}

fn scalar_field_to_rgba8(values: &[f32]) -> Vec<u8> {
    let len = values.len();
    let mut pixels = vec![0u8; len * 4];
    pixels.par_chunks_mut(4).enumerate().for_each(|(idx, px)| {
        let v = (values[idx].clamp(0.0, 1.0) * 255.0).round() as u8;
        px.copy_from_slice(&[v, v, v, 255]);
    });
    pixels
}

/// Rasterize a world map to RGBA8 pixels (one pixel per cell).
pub fn map_to_rgba8(map: &WorldMap) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];

    pixels.par_chunks_mut(4).enumerate().for_each(|(idx, px)| {
        let color = biome_rgba(map.biome[idx]);
        px.copy_from_slice(&color);
    });

    pixels
}

/// Encode normalized scalar field `[0, 1]` as 16-bit grayscale samples.
pub fn floats_to_gray16(values: &[f32]) -> Vec<u16> {
    values
        .iter()
        .map(|&v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
        .collect()
}

/// Encode normalized elevation `[0, 1]` as 16-bit grayscale samples.
pub fn map_elevation_to_gray16(map: &WorldMap) -> Vec<u16> {
    floats_to_gray16(&map.elevation)
}

/// Stable biome discriminant for machine-readable export (see `biome_id_label`).
pub fn biome_to_id(biome: Biome) -> u16 {
    match biome {
        Biome::Ocean => 0,
        Biome::Lake => 1,
        Biome::Ice => 2,
        Biome::Tundra => 3,
        Biome::Taiga => 4,
        Biome::Grassland => 5,
        Biome::TemperateForest => 6,
        Biome::Desert => 7,
        Biome::Savanna => 8,
        Biome::TropicalForest => 9,
        Biome::Mountain => 10,
    }
}

pub fn map_biome_id_to_gray16(map: &WorldMap) -> Vec<u16> {
    map.biome.iter().map(|&b| biome_to_id(b)).collect()
}

pub fn bools_to_gray8(values: &[bool]) -> Vec<u8> {
    values.iter().map(|&v| if v { 255 } else { 0 }).collect()
}

fn rgba8_to_rgb8(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    rgb
}

fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// Write a biome-colored PNG preview of the map.
pub fn write_map_png(map: &WorldMap, path: &Path) -> io::Result<()> {
    ensure_parent_dir(path)?;

    let pixels = map_to_rgba8(map);
    let image = image::RgbaImage::from_raw(map.width as u32, map.height as u32, pixels)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid image dimensions"))?;
    image.save(path).map_err(io::Error::other)
}

fn tiff_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}

fn write_tiff_rgb8_page<W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    width: u32,
    height: u32,
    description: &str,
    rgb: &[u8],
) -> io::Result<()> {
    let mut page = encoder
        .new_image::<colortype::RGB8>(width, height)
        .map_err(tiff_err)?;
    page.encoder()
        .write_tag(Tag::ImageDescription, description)
        .map_err(tiff_err)?;
    page.write_data(rgb).map_err(tiff_err)
}

fn write_tiff_gray16_page<W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    width: u32,
    height: u32,
    description: &str,
    gray: &[u16],
) -> io::Result<()> {
    let mut page = encoder
        .new_image::<colortype::Gray16>(width, height)
        .map_err(tiff_err)?;
    page.encoder()
        .write_tag(Tag::ImageDescription, description)
        .map_err(tiff_err)?;
    page.write_data(gray).map_err(tiff_err)
}

fn write_tiff_gray8_page<W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    width: u32,
    height: u32,
    description: &str,
    gray: &[u8],
) -> io::Result<()> {
    let mut page = encoder
        .new_image::<colortype::Gray8>(width, height)
        .map_err(tiff_err)?;
    page.encoder()
        .write_tag(Tag::ImageDescription, description)
        .map_err(tiff_err)?;
    page.write_data(gray).map_err(tiff_err)
}

const BIOME_ID_LEGEND: &str = "biome_id: 0=Ocean 1=Lake 2=Ice 3=Tundra 4=Taiga 5=Grassland 6=TemperateForest 7=Desert 8=Savanna 9=TropicalForest 10=Mountain";

/// Write a multi-page TIFF with the selected data layers.
pub fn write_map_tiff(map: &WorldMap, path: &Path, layers: TiffLayerSet) -> io::Result<()> {
    ensure_parent_dir(path)?;

    let width = map.width as u32;
    let height = map.height as u32;
    if width == 0 || height == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "map dimensions must be non-zero",
        ));
    }
    if !layers.has_any() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TIFF export requires at least one layer",
        ));
    }

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut encoder = TiffEncoder::new(&mut writer).map_err(tiff_err)?;

    if layers.biomes {
        let rgb = rgba8_to_rgb8(&map_to_rgba8(map));
        write_tiff_rgb8_page(&mut encoder, width, height, "biomes", &rgb)?;
    }
    if layers.elevation {
        write_tiff_gray16_page(
            &mut encoder,
            width,
            height,
            "elevation",
            &map_elevation_to_gray16(map),
        )?;
    }
    if layers.temperature {
        write_tiff_gray16_page(
            &mut encoder,
            width,
            height,
            "temperature",
            &floats_to_gray16(&map.temperature),
        )?;
    }
    if layers.rainfall {
        write_tiff_gray16_page(
            &mut encoder,
            width,
            height,
            "rainfall",
            &floats_to_gray16(&map.rainfall),
        )?;
    }
    if layers.biome_id {
        write_tiff_gray16_page(
            &mut encoder,
            width,
            height,
            BIOME_ID_LEGEND,
            &map_biome_id_to_gray16(map),
        )?;
    }
    if layers.water {
        write_tiff_gray8_page(
            &mut encoder,
            width,
            height,
            "water_mask",
            &bools_to_gray8(&map.water_mask),
        )?;
    }

    writer.flush()?;
    Ok(())
}

/// Compute land/ocean fractions and biome histogram for stats export.
pub fn compute_map_stats(map: &WorldMap, config: &WorldGenConfig, elapsed_ms: u64) -> MapStats {
    let total = map.width * map.height;
    let land_cells = map.water_mask.iter().filter(|&&w| !w).count();
    let ocean_cells = map.biome.iter().filter(|&&b| b == Biome::Ocean).count();

    let mut biomes = HashMap::new();
    for &biome in &map.biome {
        *biomes.entry(biome_label(biome).to_string()).or_insert(0) += 1;
    }

    let params = config.resolve();
    MapStats {
        config: config.clone(),
        width: map.width,
        height: map.height,
        cell_size_m: params.cell_size_m,
        map_width_m: params.map_width_m,
        map_height_m: params.map_height_m,
        max_elevation_m: params.max_elevation_m,
        sea_level_m: params.sea_level_m,
        land_fraction: land_cells as f64 / total as f64,
        ocean_fraction: ocean_cells as f64 / total as f64,
        biomes,
        elapsed_ms,
    }
}

/// Write stats JSON to disk.
pub fn write_map_stats(stats: &MapStats, path: &Path) -> io::Result<()> {
    ensure_parent_dir(path)?;
    let json = serde_json::to_string_pretty(stats).map_err(io::Error::other)?;
    std::fs::write(path, json)
}

fn biome_label(biome: Biome) -> &'static str {
    match biome {
        Biome::Ocean => "Ocean",
        Biome::Lake => "Lake",
        Biome::Ice => "Ice",
        Biome::Tundra => "Tundra",
        Biome::Taiga => "Taiga",
        Biome::Grassland => "Grassland",
        Biome::TemperateForest => "TemperateForest",
        Biome::Desert => "Desert",
        Biome::Savanna => "Savanna",
        Biome::TropicalForest => "TropicalForest",
        Biome::Mountain => "Mountain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generate_world, WorldGenConfig};

    #[test]
    fn map_to_rgba8_correct_length() {
        let config = WorldGenConfig::test_config(1, 64);
        let map = generate_world(&config);
        let pixels = map_to_rgba8(&map);
        assert_eq!(pixels.len(), 64 * 64 * 4);
    }

    #[test]
    fn map_to_preview_rgba8_all_layers_correct_length() {
        let config = WorldGenConfig::test_config(1, 64);
        let map = generate_world(&config);
        let expected = 64 * 64 * 4;
        for layer in PreviewLayer::ALL {
            let pixels = map_to_preview_rgba8(&map, layer);
            assert_eq!(pixels.len(), expected, "{layer:?}");
        }
    }

    #[test]
    fn write_map_png_produces_valid_file() {
        let config = WorldGenConfig::test_config(2, 32);
        let map = generate_world(&config);
        let path = std::env::temp_dir().join("terraforge_mapgen_test.png");
        write_map_png(&map, &path).expect("write png");
        let bytes = std::fs::read(&path).expect("read png");
        assert!(bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn map_elevation_to_gray16_range() {
        let config = WorldGenConfig::test_config(3, 16);
        let map = generate_world(&config);
        let gray = map_elevation_to_gray16(&map);
        assert_eq!(gray.len(), 16 * 16);
        assert!(!gray.is_empty());
    }

    fn count_tiff_pages(path: &Path) -> u32 {
        use std::io::BufReader;
        use tiff::decoder::Decoder;

        let file = std::fs::File::open(path).expect("open tiff");
        let mut decoder = Decoder::new(BufReader::new(file)).expect("decode tiff");
        let mut pages = 0u32;
        loop {
            pages += 1;
            let _ = decoder.read_image().expect("read page");
            if !decoder.more_images() {
                break;
            }
            decoder.next_image().expect("advance page");
        }
        pages
    }

    #[test]
    fn tiff_layer_set_parse() {
        let full = TiffLayerSet::parse("full").unwrap();
        assert_eq!(full.page_count(), 6);

        let custom = TiffLayerSet::parse("elevation,temperature").unwrap();
        assert_eq!(custom.page_count(), 2);
        assert!(!custom.biomes);
        assert!(custom.elevation);
        assert!(custom.temperature);
    }

    #[test]
    fn write_map_tiff_full_layers_has_six_pages() {
        let config = WorldGenConfig::test_config(4, 32);
        let map = generate_world(&config);
        let path = std::env::temp_dir().join("terraforge_mapgen_test_full.tiff");
        write_map_tiff(&map, &path, TiffLayerSet::full()).expect("write tiff");
        assert_eq!(count_tiff_pages(&path), 6);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn write_map_tiff_legacy_layers_has_two_pages() {
        let config = WorldGenConfig::test_config(4, 32);
        let map = generate_world(&config);
        let path = std::env::temp_dir().join("terraforge_mapgen_test_legacy.tiff");
        write_map_tiff(&map, &path, TiffLayerSet::default_layers()).expect("write tiff");
        assert_eq!(count_tiff_pages(&path), 2);
        let _ = std::fs::remove_file(path);
    }
}
