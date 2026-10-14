//! The ground materials Vistaforge builds the land surface from: each one is a full Conquest
//! Reforged (or vanilla) block plus the thin version of it that makes smooth ground, in 1/8
//! steps. Thin versions come from CR `Layer` blocks, from CR `Slab` blocks (which have
//! `layers` 1–8 too) or from vanilla snow.
//!
//! Every block name here was checked against the CR 1.7.0 catalog. If a later catalog lacks
//! one, the material falls back to a plain one of the same kind instead of failing.

use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::sink::{BlockId, Blocks, AIR};

/// What a ground material is, which decides what grows on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Grass,
    /// Forest floor: leaf litter, needles, roots.
    Floor,
    Moss,
    Soil,
    Mud,
    Sand,
    Gravel,
    Rock,
    Snow,
    Ice,
}

impl Kind {
    /// Organic covers sit on a soil block rather than on more of themselves.
    pub(crate) fn organic(self) -> bool {
        matches!(self, Kind::Grass | Kind::Floor | Kind::Moss)
    }
}

/// How the thin version of a material is made.
#[derive(Clone, Copy)]
enum Thin<'a> {
    /// A CR `Layer` (or tinted overlay layer): `layers`, `waterlogged`.
    Layer(&'a str),
    /// A CR `Slab`: `layers`, `type=bottom`, `waterlogged`.
    Slab(&'a str),
    /// Vanilla snow layers.
    Snow,
    /// No thin version: thin cells are left empty (only used by fallbacks).
    None,
}

struct Spec<'a> {
    key: &'static str,
    full: &'a str,
    props: &'static [(&'static str, &'static str)],
    thin: Thin<'a>,
    kind: Kind,
    /// Coloured by the biome's grass colour in game (and in previews).
    tinted: bool,
}

const SNOWLESS: &[(&str, &str)] = &[("snowy", "false")];

macro_rules! spec {
    ($key:literal, $full:literal, $thin:expr, $kind:ident) => {
        Spec {
            key: $key,
            full: $full,
            props: &[],
            thin: $thin,
            kind: Kind::$kind,
            tinted: false,
        }
    };
    ($key:literal, $full:literal, $thin:expr, $kind:ident, tinted) => {
        Spec {
            key: $key,
            full: $full,
            props: &[],
            thin: $thin,
            kind: Kind::$kind,
            tinted: true,
        }
    };
    ($key:literal, $full:literal, $props:expr, $thin:expr, $kind:ident, tinted) => {
        Spec {
            key: $key,
            full: $full,
            props: $props,
            thin: $thin,
            kind: Kind::$kind,
            tinted: true,
        }
    };
}

use Thin::{Layer, Slab};

/// Rock types with a CR slab (for smooth bare rock) that strata and boulders draw on.
pub(crate) const ROCKS: &[&str] = &[
    "weathered_granite",
    "light_gray_granite",
    "dark_gray_granite",
    "white_granite",
    "brown_granite",
    "light_brown_granite",
    "gneiss",
    "eroded_gneiss",
    "green_schist",
    "blue_schist",
    "weathered_blue_schist",
    "lichen_blue_schist",
    "mossy_blue_schist",
    "slate",
    "shale",
    "graywacke",
    "warped_slate",
    "gray_limestone",
    "pale_limestone",
    "dark_limestone",
    "tan_limestone",
    "stained_tan_limestone",
    "light_limestone_boulders",
    "mossy_light_limestone_boulders",
    "mossy_limestone",
    "moss_covered_limestone",
    "tan_sandstone",
    "brown_sandstone",
    "faded_gray_sandstone",
    "faded_tan_sandstone",
    "orange_sandstone",
    "mossy_faded_tan_sandstone",
    "weathered_andesite",
    "porous_andesite",
    "weathered_basalt",
    "porous_basalt",
    "tuff",
    "dark_tuff",
];

#[rustfmt::skip]
const SPECS: &[Spec<'static>] = &[
    // Fallbacks first: every kind falls back to one of these, so they must exist.
    spec!("grass", "minecraft:grass_block", SNOWLESS, Layer("conquest:grass_block_layer"), Grass, tinted),
    spec!("dirt", "minecraft:dirt", Thin::None, Soil),
    spec!("sand", "minecraft:sand", Layer("conquest:sand_layer"), Sand),
    spec!("gravel", "minecraft:gravel", Layer("conquest:gravel_layer"), Gravel),
    spec!("stone", "minecraft:stone", Thin::None, Rock),
    spec!("snow", "minecraft:snow_block", Thin::Snow, Snow),

    // Grass and grass growing through other ground.
    spec!("clover", "conquest:clover_covered_grass", SNOWLESS, Layer("conquest:clover_covered_grass_layer"), Grass, tinted),
    spec!("taiga_grass", "conquest:taiga_grass", SNOWLESS, Layer("conquest:taiga_grass_layer"), Grass, tinted),
    spec!("grassy_dirt", "minecraft:grass_block", SNOWLESS, Layer("conquest:grassy_dirt_layer"), Grass, tinted),
    spec!("grassy_stones", "conquest:overgrown_small_stones", Layer("conquest:grassy_small_stones_layer"), Grass, tinted),
    spec!("overgrown_stones", "conquest:overgrown_small_stones", Layer("conquest:overgrown_small_stones_layer"), Grass, tinted),
    spec!("overgrown_gravel", "conquest:overgrown_brown_gravel", Layer("conquest:overgrown_brown_gravel_layer"), Grass, tinted),
    spec!("overgrown_debris", "conquest:overgrown_pale_limestone_debris", Layer("conquest:overgrown_pale_limestone_debris_layer"), Grass, tinted),
    spec!("overgrown_sand", "conquest:overgrown_tan_sand", Layer("conquest:overgrown_tan_sand_layer"), Grass, tinted),
    spec!("overgrown_silt", "conquest:overgrown_light_silt", Slab("conquest:overgrown_light_silt_slab"), Grass, tinted),
    spec!("overgrown_rocky_sandy", "conquest:overgrown_rocky_sandy_soil", Slab("conquest:overgrown_rocky_sandy_soil_slab"), Grass, tinted),

    // Forest floors.
    spec!("overgrown_fir_floor", "conquest:overgrown_fir_forest_floor", Layer("conquest:overgrown_fir_forest_floor_layer"), Floor, tinted),
    spec!("mossy_fir_floor", "conquest:mossy_fir_forest_floor", Layer("conquest:mossy_fir_forest_floor_layer"), Floor, tinted),
    spec!("fir_floor", "conquest:fir_forest_floor", Layer("conquest:fir_forest_floor_layer"), Floor),
    spec!("taiga_floor", "conquest:taiga_forest_floor", Layer("conquest:taiga_forest_floor_layer"), Floor),
    spec!("autumn_floor", "conquest:autumnal_forest_floor", Layer("conquest:autumnal_forest_floor_layer"), Floor),
    spec!("autumn_roots", "conquest:autumnal_forest_floor_with_roots", Layer("conquest:autumnal_forest_floor_with_roots_layer"), Floor),
    spec!("mossy_roots", "conquest:mossy_forest_floor_with_roots", Layer("conquest:mossy_forest_floor_with_roots_layer"), Floor),
    spec!("muddy_floor", "conquest:muddy_autumnal_forest_floor", Layer("conquest:muddy_autumnal_forest_floor_layer"), Floor),
    spec!("shredded_leaves", "conquest:shredded_leaves", Layer("conquest:shredded_leaves_layer"), Floor),
    spec!("podzol", "minecraft:podzol", SNOWLESS, Layer("conquest:vibrant_autumnal_forest_floor_layer"), Floor, tinted),

    // Moss and bog.
    spec!("green_sphagnum", "conquest:green_sphagnum_moss_block", Layer("conquest:green_sphagnum_moss_block_layer"), Moss),
    spec!("light_sphagnum", "conquest:light_green_sphagnum_moss_block", Layer("conquest:light_green_sphagnum_moss_block_layer"), Moss),
    spec!("brown_sphagnum", "conquest:brown_sphagnum_moss_block", Layer("conquest:brown_sphagnum_moss_block_layer"), Moss),
    spec!("yellow_sphagnum", "conquest:yellow_sphagnum_moss_block", Layer("conquest:yellow_sphagnum_moss_block_layer"), Moss),
    spec!("red_sphagnum", "conquest:red_sphagnum_moss_block", Layer("conquest:red_sphagnum_moss_block_layer"), Moss),
    spec!("heather_moss", "conquest:heather_covered_moss", Layer("conquest:heather_covered_moss_layer"), Moss),
    spec!("brown_moss", "conquest:brown_moss", Slab("conquest:brown_moss_slab"), Moss),
    spec!("cushion_moss", "conquest:cushion_moss_covered_loamy_dirt", Slab("conquest:cushion_moss_covered_loamy_dirt_slab"), Moss),
    spec!("mossy_soil", "conquest:mossy_soil", Slab("conquest:mossy_soil_slab"), Moss),

    // Soils.
    spec!("coarse_dirt", "minecraft:coarse_dirt", Layer("conquest:dirt_path_layer"), Soil),
    spec!("brown_earth", "conquest:brown_earth", Slab("conquest:brown_earth_slab"), Soil),
    spec!("rocky_soil", "conquest:rocky_soil", Slab("conquest:rocky_soil_slab"), Soil),
    spec!("rocky_sandy_soil", "conquest:rocky_sandy_soil", Slab("conquest:rocky_sandy_soil_slab"), Soil),
    spec!("sandy_soil", "conquest:sandy_soil", Slab("conquest:sandy_soil_slab"), Soil),
    spec!("coarse_sandy_soil", "conquest:coarse_sandy_soil", Layer("conquest:coarse_sandy_soil_layer"), Soil),
    spec!("lichen_dirt", "conquest:lichen_loamy_dirt", Slab("conquest:lichen_loamy_dirt_slab"), Soil),
    spec!("frozen_dirt", "conquest:frozen_loamy_dirt", Slab("conquest:frozen_loamy_dirt_slab"), Soil),
    spec!("red_earth", "conquest:red_earth", Slab("conquest:red_earth_slab"), Soil),
    spec!("ochre_soil", "conquest:ochre_soil", Slab("conquest:ochre_soil_slab"), Soil),
    spec!("coarse_red_dirt", "conquest:coarse_red_dirt", Slab("conquest:coarse_red_dirt_slab"), Soil),
    spec!("burnt_soil", "conquest:burnt_soil", Slab("conquest:burnt_soil_slab"), Soil),

    // Mud, peat, silt and clay.
    spec!("mud", "conquest:mud", Slab("conquest:mud_slab"), Mud),
    spec!("thick_mud", "conquest:thick_mud", Slab("conquest:thick_mud_slab"), Mud),
    spec!("clay_mud", "conquest:clay_rich_mud", Slab("conquest:clay_rich_mud_slab"), Mud),
    spec!("peat", "conquest:peat", Slab("conquest:peat_slab"), Mud),
    spec!("dark_silt", "conquest:dark_silt", Slab("conquest:dark_silt_slab"), Mud),
    spec!("mossy_silt", "conquest:mossy_light_silt", Slab("conquest:mossy_light_silt_slab"), Mud, tinted),
    spec!("clay", "conquest:dark_gray_clay", Slab("conquest:dark_gray_clay_slab"), Mud),
    spec!("red_clay", "conquest:red_clay", Slab("conquest:red_clay_slab"), Mud),

    // Sands.
    spec!("wet_sand", "conquest:wet_sand", Layer("conquest:wet_sand_layer"), Sand),
    spec!("tan_sand", "conquest:tan_sand", Layer("conquest:tan_sand_layer"), Sand),
    spec!("wet_tan_sand", "conquest:wet_tan_sand", Layer("conquest:wet_tan_sand_layer"), Sand),
    spec!("gray_sand", "conquest:gray_sand", Layer("conquest:gray_sand_layer"), Sand),
    spec!("wet_gray_sand", "conquest:wet_gray_sand", Layer("conquest:wet_gray_sand_layer"), Sand),
    spec!("light_gray_sand", "conquest:light_gray_sand", Layer("conquest:light_gray_sand_layer"), Sand),
    spec!("wet_light_gray_sand", "conquest:wet_light_gray_sand", Layer("conquest:wet_light_gray_sand_layer"), Sand),
    spec!("river_sand", "conquest:dry_river_sand", Layer("conquest:dry_river_sand_layer"), Sand),
    spec!("wet_river_sand", "conquest:wet_river_sand", Layer("conquest:wet_river_sand_layer"), Sand),
    spec!("sand_gravel", "conquest:sand_and_gravel", Layer("conquest:sand_and_gravel_layer"), Sand),
    spec!("wet_sand_gravel", "conquest:wet_sand_and_gravel", Layer("conquest:wet_sand_and_gravel_layer"), Sand),
    spec!("sand_veg", "conquest:sand_and_vegetation", Layer("conquest:sand_and_vegetation_layer"), Sand),
    spec!("wet_sand_veg", "conquest:wet_sand_and_vegetation", Layer("conquest:wet_sand_and_vegetation_layer"), Sand),
    spec!("mossy_sand", "conquest:mossy_tan_sand", Layer("conquest:mossy_tan_sand_layer"), Sand, tinted),

    // Gravels, stones and debris.
    spec!("brown_gravel", "conquest:brown_gravel", Layer("conquest:brown_gravel_layer"), Gravel),
    spec!("gray_gravel", "conquest:gray_gravel", Layer("conquest:gray_gravel_layer"), Gravel),
    spec!("light_gravel", "conquest:light_gravel", Layer("conquest:light_gravel_layer"), Gravel),
    spec!("dark_gravel", "conquest:dark_gravel", Layer("conquest:dark_gravel_layer"), Gravel),
    spec!("limestone_gravel", "conquest:gray_limestone_gravel", Layer("conquest:gray_limestone_gravel_layer"), Gravel),
    spec!("mossy_gravel", "conquest:mossy_brown_gravel", Layer("conquest:mossy_brown_gravel_layer"), Gravel, tinted),
    spec!("small_stones", "conquest:small_stones", Layer("conquest:small_stones_layer"), Gravel),
    spec!("mossy_stones", "conquest:mossy_small_stones", Layer("conquest:mossy_small_stones_layer"), Gravel, tinted),
    spec!("pebbles", "conquest:smooth_pebbles", Layer("conquest:smooth_pebbles_layer"), Gravel),
    spec!("schist_debris", "conquest:schist_debris", Layer("conquest:schist_debris_layer"), Gravel),
    spec!("mossy_schist_debris", "conquest:mossy_schist_debris", Layer("conquest:mossy_schist_debris_layer"), Gravel),
    spec!("sandstone_debris", "conquest:sandstone_debris", Layer("conquest:sandstone_debris_layer"), Gravel),
    spec!("brown_sandstone_debris", "conquest:brown_sandstone_debris", Layer("conquest:brown_sandstone_debris_layer"), Gravel),
    spec!("limestone_debris", "conquest:pale_limestone_debris", Layer("conquest:pale_limestone_debris_layer"), Gravel),
    spec!("mossy_limestone_debris", "conquest:mossy_pale_limestone_debris", Layer("conquest:mossy_pale_limestone_debris_layer"), Gravel, tinted),
    spec!("dark_limestone_debris", "conquest:dark_limestone_debris", Layer("conquest:dark_limestone_debris_layer"), Gravel),

    // Volcanic ash: fine and loose, so it lies deep like sand.
    spec!("ash", "conquest:ash", Layer("conquest:ash_layer"), Sand),

    // Ice.
    spec!("glacier_ice", "conquest:glacier_ice", Slab("conquest:glacier_ice_slab"), Ice),
    spec!("dirty_ice", "conquest:dirty_glacier_ice", Slab("conquest:dirty_glacier_ice_slab"), Ice),
];

/// Index of a material in a [`Kit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Mat(pub u16);

pub(crate) struct Material {
    pub kind: Kind,
    pub full: BlockId,
    /// Thin versions, `layers` 1..=7 (index 0 is one layer).
    pub thin: [BlockId; 7],
    /// The same, waterlogged (equal to `thin` where the block can't hold water).
    pub thin_wet: [BlockId; 7],
    /// Average top colour, before the biome's grass tint.
    pub color: [u8; 3],
    pub tinted: bool,
}

/// Every ground material, resolved against the user's catalog.
pub(crate) struct Kit {
    pub mats: Vec<Material>,
    keys: HashMap<&'static str, Mat>,
    /// Every placed ground block: its material and layer count (0 for a full block).
    blocks: HashMap<BlockId, (Mat, u8)>,
    /// Keys whose blocks were missing from the catalog and fell back to another material.
    pub missing: Vec<&'static str>,
}

fn rgb(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

impl Kit {
    pub fn new(b: &Blocks) -> Result<Self> {
        let mut kit = Kit {
            mats: Vec::new(),
            keys: HashMap::new(),
            blocks: HashMap::new(),
            missing: Vec::new(),
        };
        for spec in SPECS {
            kit.add(b, spec)?;
        }
        for key in ROCKS {
            let (full, slab) = (format!("conquest:{key}"), format!("conquest:{key}_slab"));
            let spec = Spec {
                key,
                full: &full,
                props: &[],
                thin: Slab(&slab),
                kind: Kind::Rock,
                tinted: false,
            };
            kit.add(b, &spec)?;
        }
        Ok(kit)
    }

    fn fallback(kind: Kind) -> &'static str {
        match kind {
            Kind::Grass | Kind::Floor | Kind::Moss => "grass",
            Kind::Soil | Kind::Mud => "dirt",
            Kind::Sand => "sand",
            Kind::Gravel => "gravel",
            Kind::Rock => "stone",
            Kind::Snow | Kind::Ice => "snow",
        }
    }

    fn add(&mut self, b: &Blocks, spec: &Spec) -> Result<()> {
        let thin_name = match spec.thin {
            Thin::Layer(n) | Thin::Slab(n) => Some(n),
            Thin::Snow => Some("minecraft:snow"),
            Thin::None => None,
        };
        let present = b.has(spec.full) && thin_name.is_none_or(|n| b.has(n));
        if !present {
            let Some(&fb) = self.keys.get(Self::fallback(spec.kind)) else {
                bail!("ground material {} is missing from the catalog", spec.full);
            };
            self.missing.push(spec.key);
            self.keys.insert(spec.key, fb);
            return Ok(());
        }
        let full = b.id(spec.full, spec.props)?;
        let mut thin = [AIR; 7];
        let mut thin_wet = [AIR; 7];
        for i in 0..7 {
            let n = (i + 1).to_string();
            let (dry, wet) = match spec.thin {
                Thin::Layer(name) => (
                    b.id(name, &[("layers", &n), ("waterlogged", "false")])?,
                    b.id(name, &[("layers", &n), ("waterlogged", "true")])?,
                ),
                Thin::Slab(name) => (
                    b.id(
                        name,
                        &[("layers", &n), ("type", "bottom"), ("waterlogged", "false")],
                    )?,
                    b.id(
                        name,
                        &[("layers", &n), ("type", "bottom"), ("waterlogged", "true")],
                    )?,
                ),
                Thin::Snow => {
                    let id = b.id("minecraft:snow", &[("layers", &n)])?;
                    (id, id)
                }
                // Rounded away: half a layer of a material with no thin form is no layer.
                Thin::None => (
                    if i >= 3 { full } else { AIR },
                    if i >= 3 { full } else { AIR },
                ),
            };
            thin[i] = dry;
            thin_wet[i] = wet;
        }
        let color = b
            .catalog
            .block(spec.full)
            .and_then(|i| i.top_rgb)
            .or_else(|| {
                thin_name
                    .and_then(|n| b.catalog.block(n))
                    .and_then(|i| i.top_rgb)
            })
            .unwrap_or(0x7f7f7f);
        let mat = Mat(self.mats.len() as u16);
        self.mats.push(Material {
            kind: spec.kind,
            full,
            thin,
            thin_wet,
            color: rgb(color),
            tinted: spec.tinted,
        });
        self.keys.insert(spec.key, mat);
        // The first material to claim a block names it (fallback aliases share blocks).
        self.blocks.entry(full).or_insert((mat, 0));
        for (i, id) in thin.into_iter().chain(thin_wet).enumerate() {
            if id != AIR && id != full {
                self.blocks.entry(id).or_insert((mat, (i % 7) as u8 + 1));
            }
        }
        Ok(())
    }

    /// A material by key. Keys are fixed in this module, so an unknown key is a bug.
    pub fn get(&self, key: &str) -> Mat {
        *self
            .keys
            .get(key)
            .unwrap_or_else(|| panic!("unknown ground material {key}"))
    }

    pub fn mat(&self, m: Mat) -> &Material {
        &self.mats[m.0 as usize]
    }

    /// The material a placed block belongs to and its layer count (0 for a full block), if it
    /// is ground.
    pub fn of_block(&self, id: BlockId) -> Option<(Mat, u8)> {
        self.blocks.get(&id).copied()
    }
}
