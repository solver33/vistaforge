//! The biomes Vistaforge generates. `terrain` decides where each biome is; `surface` and
//! `decor` decide what it looks like (ground blocks, plants, rocks, trees, and the Minecraft
//! biome written to the chunks, which sets grass, foliage and water colours).
//!
//! New biomes may be added at the end; ids are stable because presets and saved settings use them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Biome {
    /// Open grassland with scattered trees.
    Plains,
    /// Flower-rich grassland.
    Meadow,
    /// Broadleaf forest, mostly oak.
    Forest,
    BirchForest,
    /// Spruce forest.
    Taiga,
    /// Pine woods on dry, sandy or rocky ground.
    PineForest,
    /// Wet woodland with standing water and willows.
    Swamp,
    /// Open wetland: reeds, sedges, pools, no big trees.
    Marsh,
    /// Rolling uplands: heath, moss, rock outcrops.
    Highlands,
    /// Steep rocky mountain slopes.
    Mountains,
    /// Grassy slopes and meadows above the tree line.
    AlpineMeadow,
    /// Snow and rock peaks.
    SnowyPeaks,
    Beach,
    River,
    Lake,
}

impl Biome {
    pub const ALL: &'static [Biome] = &[
        Biome::Plains,
        Biome::Meadow,
        Biome::Forest,
        Biome::BirchForest,
        Biome::Taiga,
        Biome::PineForest,
        Biome::Swamp,
        Biome::Marsh,
        Biome::Highlands,
        Biome::Mountains,
        Biome::AlpineMeadow,
        Biome::SnowyPeaks,
        Biome::Beach,
        Biome::River,
        Biome::Lake,
    ];

    /// Stable id used in settings files and the app.
    pub fn id(self) -> &'static str {
        match self {
            Biome::Plains => "plains",
            Biome::Meadow => "meadow",
            Biome::Forest => "forest",
            Biome::BirchForest => "birch_forest",
            Biome::Taiga => "taiga",
            Biome::PineForest => "pine_forest",
            Biome::Swamp => "swamp",
            Biome::Marsh => "marsh",
            Biome::Highlands => "highlands",
            Biome::Mountains => "mountains",
            Biome::AlpineMeadow => "alpine_meadow",
            Biome::SnowyPeaks => "snowy_peaks",
            Biome::Beach => "beach",
            Biome::River => "river",
            Biome::Lake => "lake",
        }
    }

    /// English name for the app.
    pub fn name(self) -> &'static str {
        match self {
            Biome::Plains => "Plains",
            Biome::Meadow => "Meadow",
            Biome::Forest => "Forest",
            Biome::BirchForest => "Birch forest",
            Biome::Taiga => "Taiga",
            Biome::PineForest => "Pine forest",
            Biome::Swamp => "Swamp",
            Biome::Marsh => "Marsh",
            Biome::Highlands => "Highlands",
            Biome::Mountains => "Mountains",
            Biome::AlpineMeadow => "Alpine meadow",
            Biome::SnowyPeaks => "Snowy peaks",
            Biome::Beach => "Beach",
            Biome::River => "River",
            Biome::Lake => "Lake",
        }
    }

    pub fn from_id(id: &str) -> Option<Biome> {
        Biome::ALL.iter().copied().find(|b| b.id() == id)
    }

    /// Biomes made by the terrain itself (water and shores) rather than chosen in a biome mix.
    pub fn is_feature(self) -> bool {
        matches!(self, Biome::Beach | Biome::River | Biome::Lake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for &b in Biome::ALL {
            assert_eq!(Biome::from_id(b.id()), Some(b));
            assert_eq!(
                serde_json::to_string(&b).unwrap(),
                format!("\"{}\"", b.id())
            );
        }
    }
}
