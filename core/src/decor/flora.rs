//! Plants and other small things that stand on the ground: Conquest grasses, flowers, ferns,
//! reeds, berry bushes, ground leaves, moss and lichen carpets, loose rocks and water lilies.
//!
//! The block shape is read from each block's class in the catalog, and every state a plant can
//! take (its `layers` on thin ground, height, variant, waterlogging) is resolved up front.
//! Plant communities list what grows where, per biome and zone.

use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::biome::Biome;
use crate::sink::{BlockId, Blocks, AIR};
use crate::surface::Zone;

/// How a plant block is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// One block.
    Dense,
    /// One block, three model variants (`random`); a second one stacked on top makes it taller.
    Stack,
    /// Like `Stack`, with `height` 1 for the lower block and 2 for the one stacked on it.
    Height2,
    /// A single block whose model is 1..=n blocks tall (`height`).
    Tall(u8),
    /// Two blocks, `half` lower and upper.
    Double,
    /// A carpet of 1..=4 flowers (`flower_amount`).
    Bed,
    /// A berry bush at some `age`.
    Berry,
    /// Fallen leaves or needles lying on the ground.
    Leaves,
    /// Moss or lichen lying on a full block (`down`).
    Moss,
    /// Loose stones: `density` 1..=4, `edge`.
    Rocks,
    /// Floats on water.
    Floating,
}

impl Form {
    /// Whether a second block can stand on top of the first.
    pub fn stacks(self) -> bool {
        matches!(self, Form::Stack | Form::Height2 | Form::Double)
    }
}

/// Every state of one plant that decor places.
pub(crate) struct Plant {
    pub form: Form,
    /// `[variant][upper * 2 + wet][layers - 1]`; AIR where a state doesn't exist.
    ids: Vec<[[BlockId; 8]; 4]>,
}

impl Plant {
    pub fn variants(&self) -> usize {
        self.ids.len()
    }

    /// The block for a variant, standing on ground with `layers` (8 on a full block), lower or
    /// upper part, dry or in water. `None` when that state doesn't exist (an upper part of a
    /// one-block plant, or a plant that can't hold water).
    pub fn id(&self, variant: usize, upper: bool, layers: u8, wet: bool) -> Option<BlockId> {
        let v = &self.ids[variant % self.ids.len()];
        let id = v[upper as usize * 2 + wet as usize][(layers.clamp(1, 8) - 1) as usize];
        (id != AIR).then_some(id)
    }
}

fn flag(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

fn form_of(class: &str) -> Option<Form> {
    Some(match class {
        "PlantsDense" | "Shrub" | "Bush" | "YoungTree" => Form::Dense,
        "PlantsDenseStackable" => Form::Stack,
        "PlantsDenseStackableHeight2" => Form::Height2,
        "PlantsTall2" => Form::Tall(2),
        "PlantsTall3" => Form::Tall(3),
        "PlantsTall4" => Form::Tall(4),
        "TallGrassVanilla" | "TallFlowerVanilla" => Form::Double,
        "Flowerbed" => Form::Bed,
        "BerryBush" => Form::Berry,
        "LeavesGround" => Form::Leaves,
        "LichenMoss" => Form::Moss,
        "Rocks" => Form::Rocks,
        "LilyPad" | "LilyPadToggle2" => Form::Floating,
        _ => return None,
    })
}

type Props = Vec<(&'static str, String)>;

fn resolve(b: &Blocks, name: &str) -> Result<Plant> {
    let Some(class) = b.catalog.class_of(name) else {
        bail!("plant {name} is missing from the catalog");
    };
    let Some(form) = form_of(class) else {
        bail!("plant {name} has an unexpected shape ({class})");
    };
    let info = b.catalog.block(name).expect("class_of found it");
    let has = |p: &str| info.properties.contains_key(p);
    let one = |k: &'static str, v: String| -> Props { vec![(k, v)] };
    // Lower and upper property sets for each variant, without `layers` and `waterlogged`.
    let variants: Vec<(Props, Option<Props>)> = match form {
        Form::Dense | Form::Leaves | Form::Moss => vec![(vec![], None)],
        Form::Stack => (0..3)
            .map(|r| {
                (
                    one("random", r.to_string()),
                    Some(one("random", r.to_string())),
                )
            })
            .collect(),
        Form::Height2 => (0..3)
            .map(|r| {
                let part = |h: u8| vec![("height", h.to_string()), ("random", r.to_string())];
                (part(1), Some(part(2)))
            })
            .collect(),
        Form::Tall(n) => (1..=n)
            .map(|h| (one("height", h.to_string()), None))
            .collect(),
        Form::Double => vec![(
            one("half", "lower".into()),
            Some(one("half", "upper".into())),
        )],
        Form::Bed => (1..=4)
            .map(|a| (one("flower_amount", a.to_string()), None))
            .collect(),
        Form::Berry => [1, 3, 5, 7]
            .iter()
            .map(|a| (one("age", a.to_string()), None))
            .collect(),
        Form::Rocks => (0..8)
            .map(|v| {
                let props = vec![
                    ("density", (v / 2 + 1).to_string()),
                    ("edge", flag(v % 2 == 1).to_string()),
                ];
                (props, None)
            })
            .collect(),
        Form::Floating => match info.properties.get("toggle") {
            Some(values) => values
                .iter()
                .map(|t| (one("toggle", t.clone()), None))
                .collect(),
            None => vec![(vec![], None)],
        },
    };
    let mut ids = Vec::with_capacity(variants.len());
    for (lower, upper) in &variants {
        let mut v = [[AIR; 8]; 4];
        for (part, props) in [Some(lower), upper.as_ref()].into_iter().enumerate() {
            let Some(props) = props else {
                continue;
            };
            for wet in [false, true] {
                if wet && !has("waterlogged") {
                    continue;
                }
                let row = &mut v[part * 2 + wet as usize];
                for (l, slot) in row.iter_mut().enumerate() {
                    if l < 7 && !has("layers") {
                        continue;
                    }
                    let layers = (l + 1).to_string();
                    let mut p: Vec<(&str, &str)> =
                        props.iter().map(|(k, val)| (*k, val.as_str())).collect();
                    if has("layers") {
                        p.push(("layers", &layers));
                    }
                    if has("offset_toggle") {
                        p.push(("offset_toggle", "false"));
                    }
                    if has("waterlogged") {
                        p.push(("waterlogged", flag(wet)));
                    }
                    if form == Form::Moss {
                        for side in ["north", "east", "south", "west"] {
                            p.push((side, "false"));
                        }
                        p.push(("down", "true"));
                    }
                    *slot = b.id(name, &p)?;
                }
                // Without `layers` the plant looks the same on any ground.
                if !has("layers") {
                    *row = [row[7]; 8];
                }
            }
        }
        ids.push(v);
    }
    Ok(Plant { form, ids })
}

/// A plant community: how much of the ground carries plants, the background grasses that grow
/// everywhere, and the species that grow in drifts (patches of one species).
pub(crate) struct Community {
    pub density: f32,
    /// Typical drift size, in blocks.
    pub patch: f64,
    pub background: &'static [(&'static str, u32)],
    /// Share of patches that are background only.
    pub background_share: f32,
    pub drifts: &'static [(&'static str, u32)],
}

const MEADOW_GRASS: &[(&str, u32)] = &[
    ("common_meadow_grass", 4),
    ("grass", 3),
    ("kentucky_bluegrass", 2),
    ("timothy_grass", 2),
    ("small_fescue", 2),
    ("meadow_fescue", 1),
    ("green_meadow_fescue", 1),
];
const WOOD_GRASS: &[(&str, u32)] = &[
    ("woodland_sedge", 3),
    ("greater_wood_rush", 2),
    ("dogs_mercury", 2),
    ("grass", 1),
];
const TAIGA_GRASS: &[(&str, u32)] = &[
    ("wavy_hair_grass", 3),
    ("small_fescue", 2),
    ("deergrass", 1),
    ("greater_wood_rush", 1),
];
const HEATH_GRASS: &[(&str, u32)] = &[
    ("sheeps_fescue", 3),
    ("wavy_hair_grass", 2),
    ("deergrass", 2),
    ("purple_moor_grass", 2),
];
const WET_GRASS: &[(&str, u32)] = &[
    ("rushes", 3),
    ("bottle_sedge", 2),
    ("sedge", 2),
    ("sweet_grass", 1),
    ("lush_grass", 1),
];
const STONY_GRASS: &[(&str, u32)] = &[("sheeps_fescue", 2), ("small_fescue", 2), ("dead_grass", 1)];

const PLAINS_OPEN: Community = Community {
    density: 0.72,
    patch: 7.0,
    background: MEADOW_GRASS,
    background_share: 0.65,
    drifts: &[
        ("buttercups", 3),
        ("red_clover_flowers", 3),
        ("oxeye_daisy", 2),
        ("white_yarrow", 2),
        ("dandelion", 2),
        ("wildflowers", 2),
        ("tall_grass", 2),
        ("black_knapweed", 1),
        ("small_scabious", 1),
        ("harebell", 1),
        ("poppy", 1),
        ("cornflower", 1),
        ("wild_daisy", 1),
        ("cow_parsley", 1),
        ("meadow_saffron", 1),
    ],
};

const MEADOW_OPEN: Community = Community {
    density: 0.85,
    patch: 6.0,
    background: MEADOW_GRASS,
    background_share: 0.35,
    drifts: &[
        ("oxeye_daisy", 3),
        ("buttercups", 3),
        ("red_clover_flowers", 3),
        ("meadow_saffron", 2),
        ("harebell", 2),
        ("cornflower", 2),
        ("poppy", 2),
        ("small_scabious", 2),
        ("white_yarrow", 2),
        ("yellow_yarrow", 1),
        ("wild_basil", 1),
        ("allium", 1),
        ("azure_bluet", 1),
        ("lobelia_flowers", 1),
        ("pink_petals", 1),
        ("wildflowers", 2),
        ("rosebay_willowherb", 1),
        ("angelica", 1),
        ("melancholy_thistle", 1),
        ("tall_grass", 1),
    ],
};

const COPSE: Community = Community {
    density: 0.5,
    patch: 6.0,
    background: WOOD_GRASS,
    background_share: 0.45,
    drifts: &[
        ("bracken", 3),
        ("fern", 2),
        ("cow_parsley", 2),
        ("nettles", 2),
        ("dogs_mercury", 2),
        ("wild_overgrown_nettles", 1),
        ("raspberry_bush", 1),
        ("blackberry_bush", 1),
        ("wild_shrub", 1),
        ("young_tree", 1),
    ],
};

const FOREST_WOOD: Community = Community {
    density: 0.42,
    patch: 6.0,
    background: WOOD_GRASS,
    background_share: 0.4,
    drifts: &[
        ("bracken", 4),
        ("fern", 3),
        ("fern_1", 2),
        ("fern_2", 2),
        ("sweet_woodruff", 2),
        ("lily_of_the_valley", 2),
        ("wood_horsetail", 2),
        ("dogs_mercury", 2),
        ("large_fern_1", 1),
        ("large_fern_2", 1),
        ("harts_tongue_fern", 1),
        ("thick_fern", 1),
        ("common_cow_wheat", 1),
        ("brittlegill_mushrooms", 1),
        ("morel_mushrooms", 1),
        ("wild_shrub", 1),
        ("dark_wild_shrub", 1),
        ("young_tree", 1),
        ("blackberry_bush", 1),
        ("large_fern", 1),
    ],
};

const GLADE: Community = Community {
    density: 0.7,
    patch: 6.0,
    background: MEADOW_GRASS,
    background_share: 0.5,
    drifts: &[
        ("bracken", 2),
        ("rosebay_willowherb", 2),
        ("cow_parsley", 2),
        ("tall_grass", 2),
        ("buttercups", 1),
        ("harebell", 1),
        ("oxeye_daisy", 1),
        ("wild_daisy", 1),
        ("raspberry_bush", 1),
        ("angelica", 1),
        ("nettles", 1),
    ],
};

const BIRCH_WOOD: Community = Community {
    density: 0.45,
    patch: 6.0,
    background: &[
        ("small_fescue", 2),
        ("wavy_hair_grass", 2),
        ("grass", 2),
        ("greater_wood_rush", 1),
    ],
    background_share: 0.4,
    drifts: &[
        ("bracken", 4),
        ("common_cow_wheat", 2),
        ("lily_of_the_valley", 2),
        ("bilberry_bush", 2),
        ("fern", 2),
        ("red_common_cow_wheat", 1),
        ("lingonberry_bush", 1),
        ("harebell", 1),
        ("wood_horsetail", 1),
        ("brittlegill_mushrooms", 1),
    ],
};

const TAIGA_WOOD: Community = Community {
    density: 0.36,
    patch: 6.0,
    background: TAIGA_GRASS,
    background_share: 0.35,
    drifts: &[
        ("bilberry_bush", 4),
        ("lingonberry_bush", 3),
        ("fern_1", 2),
        ("fern_2", 2),
        ("wood_horsetail", 2),
        ("large_fern_3", 1),
        ("bracken", 1),
        ("brittlegill_mushrooms", 1),
        ("morel_mushrooms", 1),
        ("dead_grass", 1),
        ("young_tree", 1),
        ("purple_wolfs_bane", 1),
    ],
};

const TAIGA_OPEN: Community = Community {
    density: 0.6,
    patch: 7.0,
    background: TAIGA_GRASS,
    background_share: 0.5,
    drifts: &[
        ("rosebay_willowherb", 3),
        ("bilberry_bush", 2),
        ("bracken", 2),
        ("harebell", 1),
        ("dead_grass", 1),
        ("yellow_wolfs_bane", 1),
        ("large_fern", 1),
    ],
};

const PINE_WOOD: Community = Community {
    density: 0.36,
    patch: 7.0,
    background: HEATH_GRASS,
    background_share: 0.35,
    drifts: &[
        ("heather", 4),
        ("lingonberry_bush", 3),
        ("bilberry_bush", 2),
        ("dead_bracken", 1),
        ("bracken", 1),
        ("sparse_headed_feathergrass", 1),
        ("dense_headed_feathergrass", 1),
        ("dead_grass", 1),
    ],
};

const PINE_OPEN: Community = Community {
    density: 0.45,
    patch: 7.0,
    background: HEATH_GRASS,
    background_share: 0.4,
    drifts: &[
        ("heather", 4),
        ("broom_bush", 1),
        ("thyme", 1),
        ("harebell", 1),
        ("dead_grass", 1),
        ("rock_rose", 1),
    ],
};

const SWAMP_LAND: Community = Community {
    density: 0.6,
    patch: 5.0,
    background: WET_GRASS,
    background_share: 0.4,
    drifts: &[
        ("marsh_ragwort", 2),
        ("water_mint", 2),
        ("fern", 2),
        ("lush_grass", 2),
        ("greater_fen_sedge", 2),
        ("thick_rushes", 2),
        ("rushes", 2),
        ("nettles", 1),
        ("cattails", 1),
        ("wild_parsley", 1),
    ],
};

const SWAMP_WET: Community = Community {
    density: 0.75,
    patch: 5.0,
    background: WET_GRASS,
    background_share: 0.3,
    drifts: &[
        ("cattails", 3),
        ("rushes", 3),
        ("thick_rushes", 2),
        ("bottle_sedge", 2),
        ("greater_fen_sedges", 2),
        ("water_mint", 1),
        ("marsh_ragwort", 1),
        ("sedge", 1),
    ],
};

const MARSH_OPEN: Community = Community {
    density: 0.7,
    patch: 6.0,
    background: &[
        ("rushes", 3),
        ("purple_moor_grass", 3),
        ("bottle_sedge", 2),
        ("deergrass", 2),
        ("sedge", 1),
    ],
    background_share: 0.45,
    drifts: &[
        ("cotton_grass", 3),
        ("bog_asphodel", 2),
        ("cross_leaved_heath", 2),
        ("thick_rushes", 2),
        ("moorland_spotted_orchid", 1),
        ("marsh_ragwort", 1),
        ("bog_blueberry_bush", 1),
    ],
};

const HIGHLAND_OPEN: Community = Community {
    density: 0.6,
    patch: 8.0,
    background: HEATH_GRASS,
    background_share: 0.35,
    drifts: &[
        ("heather", 5),
        ("cross_leaved_heath", 2),
        ("bilberry_bush", 2),
        ("bracken", 2),
        ("lingonberry_bush", 1),
        ("dead_bracken", 1),
        ("harebell", 1),
        ("thyme", 1),
        ("broom_bush", 1),
        ("cotton_grass", 1),
        ("small_scabious", 1),
    ],
};

const BOG: Community = Community {
    density: 0.6,
    patch: 5.0,
    background: &[("deergrass", 2), ("purple_moor_grass", 2), ("rushes", 2)],
    background_share: 0.35,
    drifts: &[
        ("cotton_grass", 3),
        ("bog_asphodel", 2),
        ("cross_leaved_heath", 2),
        ("rushes", 2),
        ("bog_blueberry_bush", 1),
        ("moorland_spotted_orchid", 1),
    ],
};

const MOUNTAIN_OPEN: Community = Community {
    density: 0.3,
    patch: 6.0,
    background: STONY_GRASS,
    background_share: 0.55,
    drifts: &[
        ("thyme", 2),
        ("harebell", 2),
        ("small_scabious", 1),
        ("alpine_sow_thistle", 1),
        ("rock_rose", 1),
        ("pasque_flower", 1),
        ("yellow_wolfs_bane", 1),
    ],
};

const ALPINE_OPEN: Community = Community {
    density: 0.75,
    patch: 6.0,
    background: &[
        ("small_fescue", 3),
        ("grass", 2),
        ("sheeps_fescue", 2),
        ("common_meadow_grass", 1),
    ],
    background_share: 0.35,
    drifts: &[
        ("buttercups", 3),
        ("harebell", 3),
        ("pasque_flower", 2),
        ("small_scabious", 2),
        ("white_yarrow", 2),
        ("alpine_sow_thistle", 2),
        ("thyme", 2),
        ("simbelmyne", 1),
        ("elanor", 1),
        ("niphredil", 1),
        ("purple_wolfs_bane", 1),
        ("yellow_wolfs_bane", 1),
        ("lobelia_flowers", 1),
        ("meadow_saffron", 1),
        ("wildflowers", 1),
    ],
};

const DUNES: Community = Community {
    density: 0.15,
    patch: 6.0,
    background: &[
        ("sheeps_fescue", 2),
        ("dead_grass", 1),
        ("sea_arrowgrass", 1),
    ],
    background_share: 0.5,
    drifts: &[
        ("sea_lavender", 2),
        ("sea_arrowgrass", 2),
        ("desert_shrub", 1),
        ("dead_bush", 1),
        ("thyme", 1),
    ],
};

const SHORE: Community = Community {
    density: 0.55,
    patch: 5.0,
    background: WET_GRASS,
    background_share: 0.35,
    drifts: &[
        ("rushes", 3),
        ("thick_rushes", 2),
        ("sweet_grass", 2),
        ("water_mint", 2),
        ("bottle_sedge", 2),
        ("marsh_ragwort", 1),
        ("greater_fen_sedge", 1),
        ("cattails", 1),
        ("sedge", 1),
    ],
};

const DAMP: Community = Community {
    density: 0.55,
    patch: 5.0,
    background: WET_GRASS,
    background_share: 0.45,
    drifts: &[
        ("rushes", 2),
        ("marsh_ragwort", 2),
        ("lush_grass", 2),
        ("water_mint", 1),
        ("bottle_sedge", 1),
        ("cotton_grass", 1),
        ("thick_rushes", 1),
    ],
};

const STONY: Community = Community {
    density: 0.25,
    patch: 5.0,
    background: STONY_GRASS,
    background_share: 0.5,
    drifts: &[
        ("thyme", 2),
        ("harebell", 1),
        ("rock_rose", 1),
        ("dead_grass", 1),
        ("wild_daisy", 1),
    ],
};

const BARE: Community = Community {
    density: 0.05,
    patch: 4.0,
    background: &[("sheeps_fescue", 1), ("dead_grass", 1)],
    background_share: 0.6,
    drifts: &[("thyme", 1), ("harebell", 1)],
};

const NOTHING: Community = Community {
    density: 0.0,
    patch: 4.0,
    background: &[],
    background_share: 1.0,
    drifts: &[],
};

pub(crate) fn community(biome: Biome, zone: Zone) -> &'static Community {
    use Biome as B;
    match (biome, zone) {
        (B::SnowyPeaks, _) | (_, Zone::Bed(_)) => &NOTHING,
        (_, Zone::Rock) | (_, Zone::Scree) => &BARE,
        (B::Beach, _) => &DUNES,
        (B::Swamp, Zone::Shore | Zone::Wet) => &SWAMP_WET,
        (B::Swamp, _) => &SWAMP_LAND,
        (B::Marsh, _) => &MARSH_OPEN,
        (B::Highlands, Zone::Wet) => &BOG,
        (_, Zone::Shore) => &SHORE,
        (B::Mountains, Zone::Stony) => &BARE,
        (_, Zone::Stony) => &STONY,
        (B::Highlands, _) => &HIGHLAND_OPEN,
        (_, Zone::Wet) => &DAMP,
        (B::Plains | B::River | B::Lake, Zone::Wood) | (B::Meadow, Zone::Wood) => &COPSE,
        (B::Plains | B::River | B::Lake, _) => &PLAINS_OPEN,
        (B::Meadow, _) => &MEADOW_OPEN,
        (B::Forest, Zone::Wood) => &FOREST_WOOD,
        (B::Forest, _) => &GLADE,
        (B::BirchForest, Zone::Wood) => &BIRCH_WOOD,
        (B::BirchForest, _) => &GLADE,
        (B::Taiga, Zone::Wood) => &TAIGA_WOOD,
        (B::Taiga, _) => &TAIGA_OPEN,
        (B::PineForest, Zone::Wood) => &PINE_WOOD,
        (B::PineForest, _) => &PINE_OPEN,
        (B::Mountains, Zone::Wood) => &TAIGA_WOOD,
        (B::Mountains, _) => &MOUNTAIN_OPEN,
        (B::AlpineMeadow, Zone::Wood) => &TAIGA_WOOD,
        (B::AlpineMeadow, _) => &ALPINE_OPEN,
    }
}

/// Reeds and rushes that stand in shallow water, by biome.
pub(crate) fn emergent(biome: Biome) -> &'static [(&'static str, u32)] {
    match biome {
        Biome::Swamp | Biome::Marsh => &[
            ("cattail_reeds", 3),
            ("giant_wild_reeds", 2),
            ("greater_fen_sedges", 2),
            ("rushes", 2),
            ("summer_dead_reeds", 1),
        ],
        Biome::River => &[
            ("rushes", 3),
            ("thick_rushes", 2),
            ("giant_wild_reeds", 1),
            ("greater_fen_sedges", 1),
        ],
        Biome::Taiga | Biome::Highlands | Biome::Mountains | Biome::AlpineMeadow => &[
            ("rushes", 3),
            ("thick_rushes", 2),
            ("greater_fen_sedges", 1),
        ],
        Biome::SnowyPeaks => &[],
        _ => &[
            ("cattail_reeds", 3),
            ("giant_wild_reeds", 2),
            ("greater_fen_sedges", 2),
            ("rushes", 2),
            ("thick_rushes", 1),
        ],
    }
}

/// What floats on still water, by biome.
pub(crate) fn floating(biome: Biome) -> &'static [(&'static str, u32)] {
    match biome {
        Biome::Swamp | Biome::Marsh => &[
            ("duckweed", 4),
            ("big_water_lilies", 1),
            ("white_water_lilies", 1),
        ],
        Biome::SnowyPeaks => &[("floating_ice", 1)],
        Biome::River | Biome::Mountains | Biome::Beach => &[],
        _ => &[
            ("white_water_lilies", 2),
            ("big_water_lilies", 2),
            ("duckweed", 1),
        ],
    }
}

/// Moss and lichen carpets for full blocks of ground and rock tops, by biome.
pub(crate) fn carpets(biome: Biome) -> &'static [(&'static str, u32)] {
    match biome {
        Biome::Taiga | Biome::Forest | Biome::Swamp => {
            &[("green_sphagnum_moss", 3), ("dark_green_sphagnum_moss", 2)]
        }
        Biome::PineForest => &[("reindeer_lichen", 3), ("green_sphagnum_moss", 1)],
        Biome::Highlands | Biome::Marsh => &[
            ("reindeer_lichen", 2),
            ("red_sphagnum_moss", 1),
            ("yellow_sphagnum_moss", 1),
            ("alpine_bearberry", 1),
        ],
        Biome::Mountains | Biome::AlpineMeadow | Biome::SnowyPeaks => &[
            ("reindeer_lichen", 2),
            ("elegant_sunburst_lichen", 1),
            ("alpine_bearberry", 1),
            ("wolf_lichen", 1),
        ],
        Biome::Beach => &[("maritime_sunburst_lichen", 1)],
        _ => &[("green_sphagnum_moss", 2), ("wolf_lichen", 1)],
    }
}

/// Every plant name any table here uses (without the `conquest:` prefix).
fn all_names() -> Vec<&'static str> {
    let communities = [
        &PLAINS_OPEN,
        &MEADOW_OPEN,
        &COPSE,
        &FOREST_WOOD,
        &GLADE,
        &BIRCH_WOOD,
        &TAIGA_WOOD,
        &TAIGA_OPEN,
        &PINE_WOOD,
        &PINE_OPEN,
        &SWAMP_LAND,
        &SWAMP_WET,
        &MARSH_OPEN,
        &HIGHLAND_OPEN,
        &BOG,
        &MOUNTAIN_OPEN,
        &ALPINE_OPEN,
        &DUNES,
        &SHORE,
        &DAMP,
        &STONY,
        &BARE,
    ];
    let mut names: Vec<&str> = communities
        .iter()
        .flat_map(|c| c.background.iter().chain(c.drifts.iter()).map(|e| e.0))
        .collect();
    for &b in Biome::ALL {
        names.extend(emergent(b).iter().map(|e| e.0));
        names.extend(floating(b).iter().map(|e| e.0));
        names.extend(carpets(b).iter().map(|e| e.0));
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// Index of a plant in [`Flora`].
pub(crate) type PlantId = u16;

pub(crate) struct Flora {
    pub plants: Vec<Plant>,
    names: HashMap<String, PlantId>,
    /// Plant names that weren't in the catalog (skipped).
    pub missing: Vec<String>,
}

impl Flora {
    /// Resolves every plant the tables use, plus `extra` full names (rocks, ground leaves).
    pub fn new(b: &Blocks, extra: &[&str]) -> Result<Self> {
        let mut f = Flora {
            plants: Vec::new(),
            names: HashMap::new(),
            missing: Vec::new(),
        };
        let full: Vec<String> = all_names()
            .into_iter()
            .map(|n| format!("conquest:{n}"))
            .chain(extra.iter().map(|n| n.to_string()))
            .collect();
        for name in full {
            if f.names.contains_key(&name) {
                continue;
            }
            if !b.has(&name) {
                f.missing.push(name);
                continue;
            }
            let plant = resolve(b, &name)?;
            f.names.insert(name, f.plants.len() as PlantId);
            f.plants.push(plant);
        }
        Ok(f)
    }

    /// A plant by short name (`conquest:` implied) or full name.
    pub fn find(&self, name: &str) -> Option<PlantId> {
        if name.contains(':') {
            self.names.get(name).copied()
        } else {
            self.names.get(&format!("conquest:{name}")).copied()
        }
    }

    pub fn get(&self, id: PlantId) -> &Plant {
        &self.plants[id as usize]
    }

    /// Resolves a weighted name list, dropping names that are missing.
    pub fn list(&self, items: &[(&str, u32)]) -> Vec<(PlantId, u32)> {
        items
            .iter()
            .filter_map(|&(n, w)| self.find(n).map(|id| (id, w)))
            .collect()
    }
}
