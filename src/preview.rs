use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Seek, Write};
use std::path::Path;

use rayon::prelude::*;
use serde::Serialize;
use tiff::encoder::{colortype, TiffEncoder};
use tiff::tags::Tag;

use super::colors::{RIVER_RGBA, biome_rgba};
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
    pub plate_id: bool,
    pub water: bool,
    pub river: bool,
    pub mountain: bool,
    pub orogeny: bool,
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
            plate_id: false,
            water: false,
            river: false,
            mountain: false,
            orogeny: false,
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
            plate_id: true,
            water: true,
            river: true,
            mountain: true,
            orogeny: true,
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
            plate_id: false,
            water: false,
            river: false,
            mountain: false,
            orogeny: false,
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
                "plate_id" | "plateid" | "plates" => set.plate_id = true,
                "water" | "water_mask" => set.water = true,
                "river" | "rivers" | "river_mask" => set.river = true,
                "mountain" | "mountains" | "mountain_mask" => set.mountain = true,
                "orogeny" | "orogeny_mask" => set.orogeny = true,
                other => {
                    return Err(format!(
                        "unknown TIFF layer '{other}' (expected biomes, elevation, temperature, rainfall, biome_id, plate_id, water, river, mountain, orogeny)"
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
            || self.plate_id
            || self.water
            || self.river
            || self.mountain
            || self.orogeny
    }

    pub fn page_count(&self) -> usize {
        [
            self.biomes,
            self.elevation,
            self.temperature,
            self.rainfall,
            self.biome_id,
            self.plate_id,
            self.water,
            self.river,
            self.mountain,
            self.orogeny,
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

/// Write a map using the given export format (PNG or multi-page TIFF).
pub fn write_map(map: &WorldMap, path: &Path, format: MapExportFormat) -> io::Result<()> {
    write_map_with_tiff_layers(map, path, format, TiffLayerSet::default())
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
    Plates,
    Orogeny,
    MacroLand,
    Mountains,
    Water,
    Rivers,
    Temperature,
    Rainfall,
    Drainage,
    CoastDistance,
}

impl PreviewLayer {
    pub const ALL: [Self; 12] = [
        Self::Biomes,
        Self::Elevation,
        Self::Plates,
        Self::Orogeny,
        Self::MacroLand,
        Self::Mountains,
        Self::Water,
        Self::Rivers,
        Self::Temperature,
        Self::Rainfall,
        Self::Drainage,
        Self::CoastDistance,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Biomes => "Biomes",
            Self::Elevation => "Elevation",
            Self::Plates => "Plates",
            Self::Orogeny => "Orogeny",
            Self::MacroLand => "Macro land",
            Self::Mountains => "Mountains",
            Self::Water => "Water",
            Self::Rivers => "Rivers",
            Self::Temperature => "Temperature",
            Self::Rainfall => "Rainfall",
            Self::Drainage => "Drainage",
            Self::CoastDistance => "Coast dist",
        }
    }

    /// Short legend text for the right panel (none for biome grid legend).
    pub fn legend_hint(self) -> Option<&'static str> {
        match self {
            Self::Biomes => None,
            Self::Elevation => Some("Grayscale: low → high elevation"),
            Self::Plates => Some("Distinct color per tectonic plate"),
            Self::Orogeny => Some("Grayscale: plate-boundary uplift intensity"),
            Self::MacroLand => Some("Grayscale: continental crust macro mask"),
            Self::Mountains => Some("Red: mountain cells; gray: other land"),
            Self::Water => Some("Blue: ocean & lakes; tan: land"),
            Self::Rivers => Some("River network on neutral background"),
            Self::Temperature => Some("Grayscale: cold → warm"),
            Self::Rainfall => Some("Grayscale: dry → wet"),
            Self::Drainage => Some("Grayscale: low → high flow accumulation"),
            Self::CoastDistance => Some("Grayscale: near coast → inland"),
        }
    }
}

/// Rasterize a world map to RGBA8 for interactive preview.
pub fn map_to_preview_rgba8(map: &WorldMap, layer: PreviewLayer, rivers_overlay: bool) -> Vec<u8> {
    match layer {
        PreviewLayer::Biomes => map_to_rgba8(map),
        PreviewLayer::Elevation => scalar_field_to_rgba8(&map.elevation, map, rivers_overlay),
        PreviewLayer::Plates => plates_to_rgba8(map, rivers_overlay),
        PreviewLayer::Orogeny => scalar_field_to_rgba8(&map.orogeny, map, rivers_overlay),
        PreviewLayer::MacroLand => scalar_field_to_rgba8(&map.macro_land_mask, map, rivers_overlay),
        PreviewLayer::Mountains => mountains_to_rgba8(map, rivers_overlay),
        PreviewLayer::Water => water_to_rgba8(map, rivers_overlay),
        PreviewLayer::Rivers => rivers_to_rgba8(map),
        PreviewLayer::Temperature => scalar_field_to_rgba8(&map.temperature, map, rivers_overlay),
        PreviewLayer::Rainfall => scalar_field_to_rgba8(&map.rainfall, map, rivers_overlay),
        PreviewLayer::Drainage => drainage_to_rgba8(map, rivers_overlay),
        PreviewLayer::CoastDistance => coast_distance_to_rgba8(map, rivers_overlay),
    }
}

const LAND_NEUTRAL: [u8; 4] = [48, 48, 48, 255];
const WATER_BLUE: [u8; 4] = [32, 64, 140, 255];
const LAND_TAN: [u8; 4] = [160, 140, 100, 255];
const MOUNTAIN_RED: [u8; 4] = [220, 80, 60, 255];

fn hsv_to_rgba(h: f32, s: f32, v: f32) -> [u8; 4] {
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
        255,
    ]
}

fn plate_rgba(plate_id: u32) -> [u8; 4] {
    let hue = (plate_id as f32 * 0.618_033_988_7) % 1.0;
    hsv_to_rgba(hue, 0.62, 0.88)
}

fn plates_to_rgba8(map: &WorldMap, rivers_overlay: bool) -> Vec<u8> {
    let len = map.width * map.height;
    let w = map.width;
    let h = map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let x = idx % w;
            let y = idx / w;
            let my_plate = map.plate_id[idx];
            // Check if any neighbor has a different plate ID
            let mut is_boundary = false;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let nidx = ny as usize * w + nx as usize;
                if map.plate_id[nidx] != my_plate {
                    is_boundary = true;
                    break;
                }
            }
            let mut color = if is_boundary {
                [255u8, 255, 255, 255] // White boundary
            } else {
                [0u8, 0, 0, 255] // Black interior
            };
            if rivers_overlay && map.river_mask[idx] {
                color = RIVER_RGBA;
            }
            px.copy_from_slice(&color);
        });
    pixels
}

fn mountains_to_rgba8(map: &WorldMap, rivers_overlay: bool) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let mut color = if map.water_mask[idx] {
                WATER_BLUE
            } else if map.mountain_mask[idx] {
                MOUNTAIN_RED
            } else {
                LAND_NEUTRAL
            };
            if rivers_overlay && map.river_mask[idx] {
                color = RIVER_RGBA;
            }
            px.copy_from_slice(&color);
        });
    pixels
}

fn water_to_rgba8(map: &WorldMap, rivers_overlay: bool) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let mut color = if map.water_mask[idx] {
                WATER_BLUE
            } else {
                LAND_TAN
            };
            if rivers_overlay && map.river_mask[idx] {
                color = RIVER_RGBA;
            }
            px.copy_from_slice(&color);
        });
    pixels
}

fn rivers_to_rgba8(map: &WorldMap) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let color = if map.river_mask[idx] {
                RIVER_RGBA
            } else if map.water_mask[idx] {
                [24, 24, 32, 255]
            } else {
                LAND_NEUTRAL
            };
            px.copy_from_slice(&color);
        });
    pixels
}

fn drainage_to_rgba8(map: &WorldMap, rivers_overlay: bool) -> Vec<u8> {
    let max_flow = map
        .flow_accumulation
        .iter()
        .cloned()
        .fold(0.0f32, f32::max)
        .max(1.0);
    let log_max = (1.0 + max_flow).ln();
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let norm = if log_max > 0.0 {
                (1.0 + map.flow_accumulation[idx]).ln() / log_max
            } else {
                0.0
            };
            let v = (norm.clamp(0.0, 1.0) * 255.0).round() as u8;
            let mut color = [v, v, v, 255];
            if rivers_overlay && map.river_mask[idx] {
                color = RIVER_RGBA;
            }
            px.copy_from_slice(&color);
        });
    pixels
}

fn coast_distance_to_rgba8(map: &WorldMap, rivers_overlay: bool) -> Vec<u8> {
    let max_dist = map
        .dist_to_water
        .iter()
        .filter(|&&d| d < u32::MAX)
        .max()
        .copied()
        .unwrap_or(1)
        .max(1) as f32;
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let dist = map.dist_to_water[idx];
            let norm = if dist >= u32::MAX {
                1.0
            } else {
                (dist as f32 / max_dist).clamp(0.0, 1.0)
            };
            let v = (norm * 255.0).round() as u8;
            let mut color = [v, v, v, 255];
            if rivers_overlay && map.river_mask[idx] {
                color = RIVER_RGBA;
            }
            px.copy_from_slice(&color);
        });
    pixels
}

fn scalar_field_to_rgba8(values: &[f32], map: &WorldMap, rivers_overlay: bool) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let v = (values[idx].clamp(0.0, 1.0) * 255.0).round() as u8;
            let mut color = [v, v, v, 255];
            if rivers_overlay && map.river_mask[idx] {
                color = RIVER_RGBA;
            }
            px.copy_from_slice(&color);
        });
    pixels
}

/// Rasterize a world map to RGBA8 pixels (one pixel per cell).
pub fn map_to_rgba8(map: &WorldMap) -> Vec<u8> {
    let len = map.width * map.height;
    let mut pixels = vec![0u8; len * 4];

    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(idx, px)| {
            let mut color = biome_rgba(map.biome[idx]);
            if map.river_mask[idx] {
                color = RIVER_RGBA;
            }
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

pub fn map_plate_id_to_gray16(map: &WorldMap) -> Vec<u16> {
    map.plate_id
        .iter()
        .map(|&id| id.min(u16::MAX as u32) as u16)
        .collect()
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
    io::Error::new(io::ErrorKind::Other, e.to_string())
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
    if layers.plate_id {
        write_tiff_gray16_page(
            &mut encoder,
            width,
            height,
            "plate_id",
            &map_plate_id_to_gray16(map),
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
    if layers.river {
        write_tiff_gray8_page(
            &mut encoder,
            width,
            height,
            "river_mask",
            &bools_to_gray8(&map.river_mask),
        )?;
    }
    if layers.mountain {
        write_tiff_gray8_page(
            &mut encoder,
            width,
            height,
            "mountain_mask",
            &bools_to_gray8(&map.mountain_mask),
        )?;
    }
    if layers.orogeny {
        write_tiff_gray16_page(
            &mut encoder,
            width,
            height,
            "orogeny",
            &floats_to_gray16(&map.orogeny),
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
    use crate::{WorldGenConfig, generate_world};

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
            let pixels = map_to_preview_rgba8(&map, layer, true);
            assert_eq!(pixels.len(), expected, "{layer:?}");
            let pixels_no_rivers = map_to_preview_rgba8(&map, layer, false);
            assert_eq!(pixels_no_rivers.len(), expected, "{layer:?} no rivers");
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
        assert_eq!(full.page_count(), 10);

        let custom = TiffLayerSet::parse("elevation,temperature").unwrap();
        assert_eq!(custom.page_count(), 2);
        assert!(!custom.biomes);
        assert!(custom.elevation);
        assert!(custom.temperature);
    }

    #[test]
    fn write_map_tiff_default_layers_has_ten_pages() {
        let config = WorldGenConfig::test_config(4, 32);
        let map = generate_world(&config);
        let path = std::env::temp_dir().join("terraforge_mapgen_test_full.tiff");
        write_map_tiff(&map, &path, TiffLayerSet::full()).expect("write tiff");
        assert_eq!(count_tiff_pages(&path), 10);
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
