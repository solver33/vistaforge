//! Biome colours for grass, leaves and water, worked out as the game does: from the biome's
//! temperature and downfall on the colour maps, then blended over the 5×5 columns around.

use super::assets::Assets;
use super::texture::{decode_png, Texture};

/// Which colour a tinted face takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tint {
    None,
    Grass,
    Foliage,
    Water,
    /// A fixed colour, 0xRRGGBB (spruce and birch leaves, …).
    Fixed(u32),
}

impl Tint {
    /// Classifies the tint the exporter saw with no world: the game's default grass and leaf
    /// colours (and Conquest's own leaf default) mean the biome's colour, anything else stays.
    pub fn from_catalog(tint: u32) -> Tint {
        match tint {
            0x7BA24F | 0x91BD59 => Tint::Grass,
            0x48B518 | 0x829841 => Tint::Foliage,
            0x3F76E4 => Tint::Water,
            other => Tint::Fixed(other),
        }
    }

    /// The number the shader reads: 0 none, 1 grass, 2 foliage, 3 water, 4 fixed.
    pub fn code(self) -> (f32, u32) {
        match self {
            Tint::None => (0.0, 0),
            Tint::Grass => (1.0, 0),
            Tint::Foliage => (2.0, 0),
            Tint::Water => (3.0, 0),
            Tint::Fixed(rgb) => (4.0, rgb),
        }
    }
}

/// What a Minecraft biome does to colours.
struct BiomeColors {
    temperature: f32,
    downfall: f32,
    water: u32,
    /// Grass and leaves the biome forces instead of the colour map (swamps).
    fixed: Option<(u32, u32)>,
}

fn biome(id: &str) -> BiomeColors {
    let b = |temperature, downfall| BiomeColors {
        temperature,
        downfall,
        water: 0x3F76E4,
        fixed: None,
    };
    let id = id.strip_prefix("minecraft:").unwrap_or(id);
    match id {
        "plains" | "sunflower_plains" | "beach" => b(0.8, 0.4),
        "meadow" => b(0.5, 0.8),
        "forest" | "flower_forest" | "dark_forest" => b(0.7, 0.8),
        "birch_forest" | "old_growth_birch_forest" => b(0.6, 0.6),
        "taiga" => b(0.25, 0.8),
        "old_growth_pine_taiga" => b(0.3, 0.8),
        "old_growth_spruce_taiga" => b(0.25, 0.8),
        "snowy_taiga" => b(-0.5, 0.4),
        "snowy_plains" | "ice_spikes" => b(0.0, 0.5),
        "windswept_hills" | "windswept_gravelly_hills" | "windswept_forest" | "stony_shore" => {
            b(0.2, 0.3)
        }
        "snowy_slopes" | "grove" => b(-0.3, 0.9),
        "frozen_peaks" | "jagged_peaks" => b(-0.7, 0.9),
        "stony_peaks" => b(1.0, 0.3),
        "jungle" | "bamboo_jungle" | "sparse_jungle" => b(0.95, 0.9),
        "desert" | "savanna" | "savanna_plateau" | "badlands" => b(2.0, 0.0),
        "mangrove_swamp" => BiomeColors {
            water: 0x3A7A6A,
            ..b(0.8, 0.9)
        },
        "swamp" => BiomeColors {
            water: 0x617B64,
            fixed: Some((0x6A7039, 0x6A7039)),
            ..b(0.8, 0.9)
        },
        "warm_ocean" => BiomeColors {
            water: 0x43D5EE,
            ..b(0.5, 0.5)
        },
        "lukewarm_ocean" | "deep_lukewarm_ocean" => BiomeColors {
            water: 0x45ADF2,
            ..b(0.5, 0.5)
        },
        "cold_ocean" | "deep_cold_ocean" => BiomeColors {
            water: 0x3D57D6,
            ..b(0.5, 0.5)
        },
        "frozen_ocean" | "deep_frozen_ocean" | "frozen_river" => BiomeColors {
            water: 0x3938C9,
            ..b(0.0, 0.5)
        },
        _ => b(0.5, 0.5),
    }
}

/// The game's grass and foliage colour maps, when the jars have them.
pub struct ColorMaps {
    grass: Option<Texture>,
    foliage: Option<Texture>,
}

impl ColorMaps {
    pub fn load(assets: Option<&Assets>) -> Self {
        let read = |name: &str| {
            assets
                .and_then(|a| a.read(&format!("assets/minecraft/textures/colormap/{name}.png")))
                .and_then(|b| decode_png(&b))
                .filter(|t| t.width == 256 && t.height == 256)
        };
        Self {
            grass: read("grass"),
            foliage: read("foliage"),
        }
    }

    fn lookup(map: &Option<Texture>, temperature: f32, downfall: f32, default: u32) -> u32 {
        let Some(t) = map else {
            return default;
        };
        let temperature = temperature.clamp(0.0, 1.0);
        let downfall = downfall.clamp(0.0, 1.0) * temperature;
        let i = ((1.0 - temperature) * 255.0) as usize;
        let j = ((1.0 - downfall) * 255.0) as usize;
        let p = &t.rgba[(j * 256 + i) * 4..][..3];
        (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
    }

    /// Grass, foliage and water colours of a Minecraft biome, 0xRRGGBB.
    pub fn colors(&self, biome_id: &str) -> [u32; 3] {
        let b = biome(biome_id);
        if let Some((grass, foliage)) = b.fixed {
            return [grass, foliage, b.water];
        }
        let (grass, foliage) = fallback(b.temperature, b.downfall);
        [
            Self::lookup(&self.grass, b.temperature, b.downfall, grass),
            Self::lookup(&self.foliage, b.temperature, b.downfall, foliage),
            b.water,
        ]
    }
}

/// Vanilla's colour map values for the climates above, for when the map is missing.
fn fallback(temperature: f32, downfall: f32) -> (u32, u32) {
    let key = (
        (temperature * 100.0).round() as i32,
        (downfall * 100.0).round() as i32,
    );
    match key {
        (80, 40) => (0x91BD59, 0x77AB2F),
        (50, 80) => (0x83BB6D, 0x63A948),
        (70, 80) => (0x79C05A, 0x59AE30),
        (60, 60) => (0x88BB67, 0x6BA941),
        (25, 80) => (0x86B783, 0x68A464),
        (30, 80) => (0x86B87F, 0x68A55F),
        (20, 30) => (0x8AB689, 0x6DA36B),
        (-30, 90) | (-70, 90) | (-50, 40) | (0, 50) => (0x80B497, 0x60A17B),
        (95, 90) => (0x59C93C, 0x30BB0B),
        (200, 0) | (100, 30) => (0xBFB755, 0xAEA42A),
        _ => (0x8EB971, 0x71A74D),
    }
}

/// Averages each column's colours with the 5×5 columns around it, as the game's default
/// biome blend does. `colors` is row-major `side`×`side`.
pub fn blend(colors: &[u32], side: usize) -> Vec<u32> {
    let mut out = vec![0; colors.len()];
    for z in 0..side {
        for x in 0..side {
            let mut sum = [0u32; 3];
            let mut n = 0;
            for dz in z.saturating_sub(2)..(z + 3).min(side) {
                for dx in x.saturating_sub(2)..(x + 3).min(side) {
                    let c = colors[dz * side + dx];
                    sum[0] += c >> 16 & 0xFF;
                    sum[1] += c >> 8 & 0xFF;
                    sum[2] += c & 0xFF;
                    n += 1;
                }
            }
            out[z * side + x] = ((sum[0] / n) << 16) | ((sum[1] / n) << 8) | (sum[2] / n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_colours_without_colour_maps() {
        let maps = ColorMaps::load(None);
        assert_eq!(
            maps.colors("minecraft:plains"),
            [0x91BD59, 0x77AB2F, 0x3F76E4]
        );
        assert_eq!(
            maps.colors("minecraft:swamp"),
            [0x6A7039, 0x6A7039, 0x617B64]
        );
        assert_eq!(maps.colors("minecraft:warm_ocean")[2], 0x43D5EE);
        assert_eq!(Tint::from_catalog(0x7BA24F), Tint::Grass);
        assert_eq!(Tint::from_catalog(0x829841), Tint::Foliage);
        assert_eq!(Tint::from_catalog(0x619961), Tint::Fixed(0x619961));
    }

    #[test]
    fn blends_neighbours() {
        let mut c = vec![0u32; 25];
        c[12] = 0x190000;
        let b = blend(&c, 5);
        assert_eq!(b[12], 0x010000);
        assert_eq!(b[0], 0x020000);
    }
}
