use egui::{ColorImage, Context, TextureHandle, TextureOptions};

use terraforge::{PreviewLayer, WorldMap, map_to_preview_rgba8};

pub struct MapTexture {
    pub handle: TextureHandle,
    pub width: u32,
    pub height: u32,
}

pub fn upload_map_texture(
    ctx: &Context,
    map: &WorldMap,
    layer: PreviewLayer,
    rivers_overlay: bool,
    existing: Option<&str>,
) -> MapTexture {
    let pixels = map_to_preview_rgba8(map, layer, rivers_overlay);
    let width = map.width as u32;
    let height = map.height as u32;
    let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &pixels);
    let name = existing.unwrap_or("map_preview");
    let handle = ctx.load_texture(name, image, TextureOptions::NEAREST);
    MapTexture {
        handle,
        width,
        height,
    }
}
