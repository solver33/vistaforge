//! Everything the user chooses for one map. The app edits a [`WorldSettings`], sends it to the
//! core for previews and exports, and saves it as JSON, so field names are part of the format:
//! add fields with `#[serde(default)]`, don't rename them.

use serde::{Deserialize, Serialize};

use crate::biome::Biome;

pub const MIN_SIZE: u32 = 256;
pub const MAX_SIZE: u32 = 4096;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldSettings {
    /// World (save folder) name.
    pub name: String,
    /// Side of the square map in blocks, a multiple of 16 in `MIN_SIZE..=MAX_SIZE`.
    pub size: u32,
    pub seed: u64,
    /// Id of the preset the settings started from (informational).
    pub preset: String,
    /// Biome mix: which biomes appear and roughly what share of the land each takes.
    pub biomes: Vec<BiomeShare>,
    pub sliders: Sliders,
    /// Y of the water surface for seas, and the default for lakes.
    pub sea_level: i32,
    /// Hand-placed landforms, applied on top of the generated terrain.
    pub edits: Vec<ShapeEdit>,
    /// How far up the world goes.
    pub world_height: WorldHeight,
}

/// How far up the world goes. The bottom is always Y −64, as in vanilla.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldHeight {
    /// Vanilla: up to Y 319.
    #[default]
    Standard,
    /// Up to Y 2031, the most Minecraft 1.20.1 allows. The export adds a data pack to the world
    /// that raises the overworld's limit, so no mod is needed; the game warns about
    /// "experimental settings" when the world is opened.
    Tall,
}

impl WorldHeight {
    /// The highest Y a block can be at.
    pub fn max_y(self) -> i32 {
        match self {
            Self::Standard => 319,
            Self::Tall => 2031,
        }
    }

    /// Blocks from the bottom (Y −64) to the top, both included.
    pub fn height(self) -> i32 {
        self.max_y() + 65
    }
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            name: "Vistaforge World".into(),
            size: 1024,
            seed: 1,
            preset: "plains".into(),
            biomes: vec![BiomeShare {
                biome: Biome::Plains,
                share: 1.0,
            }],
            sliders: Sliders::default(),
            sea_level: 63,
            edits: Vec::new(),
            world_height: WorldHeight::Standard,
        }
    }
}

impl WorldSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.size < MIN_SIZE || self.size > MAX_SIZE || !self.size.is_multiple_of(16) {
            return Err(format!(
                "size must be a multiple of 16 between {MIN_SIZE} and {MAX_SIZE}"
            ));
        }
        if !(-60..=300).contains(&self.sea_level) {
            return Err("sea level must be between -60 and 300".into());
        }
        if self.biomes.iter().all(|b| b.share <= 0.0) {
            return Err("the biome mix needs at least one biome".into());
        }
        Ok(())
    }

    /// Biome shares scaled to sum to 1, without zero entries.
    pub fn biome_mix(&self) -> Vec<BiomeShare> {
        let total: f32 = self.biomes.iter().map(|b| b.share.max(0.0)).sum();
        self.biomes
            .iter()
            .filter(|b| b.share > 0.0)
            .map(|b| BiomeShare {
                biome: b.biome,
                share: b.share / total,
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiomeShare {
    pub biome: Biome,
    /// Relative weight; the app shows these as percentages.
    pub share: f32,
}

/// The main controls, each 0..=1 with 0.5 as the preset's "normal".
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sliders {
    /// Overall height differences.
    pub relief: f32,
    /// Small-scale bumpiness of the ground.
    pub roughness: f32,
    /// How much of the map is mountain ranges.
    pub mountains: f32,
    /// Strength of erosion (valleys, gullies, ridges) — the main "realism" control.
    pub erosion: f32,
    /// Lakes, ponds and wetland.
    pub water: f32,
    /// Number and size of rivers.
    pub rivers: f32,
    /// Tree density.
    pub trees: f32,
    /// Plant and flower density.
    pub vegetation: f32,
    /// Ground detail: rocks, fallen logs, debris, mixing of ground blocks.
    pub detail: f32,
    /// How wide biome borders blend.
    pub biome_blend: f32,
    /// Size of biome regions.
    pub biome_scale: f32,
    /// Share of the map under sea (0 = landlocked, the default).
    pub sea: f32,
    /// Shape of the sea: 0 a coast along one side, 0.5 one large island, 1 an archipelago.
    pub islands: f32,
}

impl Default for Sliders {
    fn default() -> Self {
        Self {
            relief: 0.5,
            roughness: 0.5,
            mountains: 0.5,
            erosion: 0.5,
            water: 0.5,
            rivers: 0.5,
            trees: 0.5,
            vegetation: 0.5,
            detail: 0.5,
            biome_blend: 0.5,
            biome_scale: 0.5,
            sea: 0.0,
            islands: 0.5,
        }
    }
}

/// A landform placed by hand on the map preview. Coordinates are in blocks (`0..size`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeEdit {
    /// A rounded rise; a negative height makes a basin.
    Hill {
        x: f32,
        z: f32,
        radius: f32,
        height: f32,
    },
    /// A mountain with a peak at `height` blocks above the surroundings.
    Mountain {
        x: f32,
        z: f32,
        radius: f32,
        height: f32,
    },
    /// Flattens the ground towards Y = `level`.
    Plateau {
        x: f32,
        z: f32,
        radius: f32,
        level: f32,
    },
    Lake {
        x: f32,
        z: f32,
        radius: f32,
        depth: f32,
    },
    /// A river along a polyline, flowing from the first point to the last.
    River {
        points: Vec<[f32; 2]>,
        width: f32,
        depth: f32,
    },
    /// A mountain ridge along a polyline.
    Ridge {
        points: Vec<[f32; 2]>,
        width: f32,
        height: f32,
    },
    /// Forces a biome inside a circle.
    BiomeZone {
        x: f32,
        z: f32,
        radius: f32,
        biome: Biome,
    },
    /// A road along a polyline: trodden earth with wheel ruts, laid into the ground (which is
    /// levelled a little beside it), with almost nothing growing on it.
    Road {
        points: Vec<[f32; 2]>,
        /// Width of the road surface in blocks.
        width: f32,
    },
    /// A waterfall: a stream pours over a cliff `drop` blocks high into a pool at its foot.
    Waterfall {
        /// The lip, where the water goes over the edge.
        x: f32,
        z: f32,
        /// Half the width of the falling water, in blocks.
        radius: f32,
        /// Height of the fall in blocks.
        drop: f32,
        /// Direction the water flows over the lip, in degrees: 0 towards +x (east), 90 towards
        /// +z (south).
        angle: f32,
    },
}

/// A named starting point for [`WorldSettings`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub biomes: Vec<BiomeShare>,
    pub sliders: Sliders,
    pub sea_level: i32,
    /// The world height the preset is made for, when it needs one (tall presets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_height: Option<WorldHeight>,
}

impl Preset {
    pub fn settings(&self, name: &str, size: u32, seed: u64) -> WorldSettings {
        WorldSettings {
            name: name.into(),
            size,
            seed,
            preset: self.id.clone(),
            biomes: self.biomes.clone(),
            sliders: self.sliders,
            sea_level: self.sea_level,
            edits: Vec::new(),
            world_height: self.world_height.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_defaults() {
        let mut s = WorldSettings::default();
        s.edits.push(ShapeEdit::River {
            points: vec![[0.0, 10.0], [500.0, 900.0]],
            width: 12.0,
            depth: 3.0,
        });
        s.edits.push(ShapeEdit::Road {
            points: vec![[5.0, 5.0], [50.0, 80.0]],
            width: 5.0,
        });
        s.edits.push(ShapeEdit::Waterfall {
            x: 300.0,
            z: 200.0,
            radius: 4.0,
            drop: 18.0,
            angle: 90.0,
        });
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"river\""));
        assert!(json.contains("\"kind\":\"road\""));
        assert!(json.contains("\"kind\":\"waterfall\""));
        assert_eq!(serde_json::from_str::<WorldSettings>(&json).unwrap(), s);
        // Old files without newer fields still load.
        let partial: WorldSettings = serde_json::from_str(r#"{"size": 512}"#).unwrap();
        assert_eq!(partial.size, 512);
        assert_eq!(partial.sliders, Sliders::default());
        assert_eq!(partial.world_height, WorldHeight::Standard);
        assert!(partial.validate().is_ok());
        let tall: WorldSettings = serde_json::from_str(r#"{"world_height": "tall"}"#).unwrap();
        assert_eq!(tall.world_height, WorldHeight::Tall);
    }

    #[test]
    fn world_heights_fit_minecraft() {
        for h in [WorldHeight::Standard, WorldHeight::Tall] {
            // Dimension types need a height that is a multiple of 16, ending at Y 2031 at most.
            assert_eq!(h.height() % 16, 0);
            assert!(h.max_y() <= 2031);
        }
        assert_eq!(WorldHeight::Standard.height(), 384);
        assert_eq!(WorldHeight::Tall.height(), 2096);
    }

    #[test]
    fn biome_mix_is_normalised() {
        let s = WorldSettings {
            biomes: vec![
                BiomeShare {
                    biome: Biome::Plains,
                    share: 3.0,
                },
                BiomeShare {
                    biome: Biome::Forest,
                    share: 1.0,
                },
                BiomeShare {
                    biome: Biome::Swamp,
                    share: 0.0,
                },
            ],
            ..WorldSettings::default()
        };
        let mix = s.biome_mix();
        assert_eq!(mix.len(), 2);
        assert!((mix[0].share - 0.75).abs() < 1e-6);
    }
}
