use super::world::Biome;

pub const RIVER_RGBA: [u8; 4] = [30, 80, 200, 255];

pub fn biome_rgba(biome: Biome) -> [u8; 4] {
    match biome {
        Biome::Ocean => [25, 55, 130, 255],
        Biome::Lake => [45, 95, 175, 255],
        Biome::Ice => [220, 235, 245, 255],
        Biome::Tundra => [160, 175, 140, 255],
        Biome::Taiga => [55, 95, 65, 255],
        Biome::Grassland => [110, 155, 65, 255],
        Biome::TemperateForest => [35, 100, 45, 255],
        Biome::Desert => [210, 185, 120, 255],
        Biome::Savanna => [175, 155, 70, 255],
        Biome::TropicalForest => [20, 120, 55, 255],
        Biome::Mountain => [120, 110, 100, 255],
    }
}

/// Display order for the map demo legend.
pub const LEGEND_ENTRIES: &[(&str, Biome)] = &[
    ("Ocean", Biome::Ocean),
    ("Lake", Biome::Lake),
    ("Ice", Biome::Ice),
    ("Tundra", Biome::Tundra),
    ("Taiga", Biome::Taiga),
    ("Grassland", Biome::Grassland),
    ("Temp. forest", Biome::TemperateForest),
    ("Desert", Biome::Desert),
    ("Savanna", Biome::Savanna),
    ("Trop. forest", Biome::TropicalForest),
    ("Mountain", Biome::Mountain),
];
