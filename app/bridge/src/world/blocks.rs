//! Block states and biomes met in a world, numbered as they turn up, with what the top-down
//! map needs to know of each block.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use vistaforge_core::catalog::{BlockState, Catalog};
use vistaforge_core::sink::Palette;

use crate::render::library::CatalogBlock;
use crate::render::tint::Tint;

/// Air, light and barriers: nothing to see.
pub const AIR: u8 = 1;
pub const WATER: u8 = 2;
pub const WATERLOGGED: u8 = 4;
/// Small or clear blocks (grass tufts, flowers, torches, glass): the map shows what is under them.
pub const SEE_THROUGH: u8 = 8;
/// Leaves, logs and branches: seen on the map, but not the ground.
pub const TREE: u8 = 16;

/// Blocks with no look, as the renderer treats them.
pub const AIR_LIKE: [&str; 6] = [
    "minecraft:air",
    "minecraft:cave_air",
    "minecraft:void_air",
    "minecraft:light",
    "minecraft:barrier",
    "minecraft:structure_void",
];

/// Plants that only grow under water and carry no `waterlogged` property: the water is
/// implied.
pub const WATER_PLANTS: [&str; 4] = [
    "minecraft:kelp",
    "minecraft:kelp_plant",
    "minecraft:seagrass",
    "minecraft:tall_seagrass",
];

#[derive(Clone, Copy, Debug)]
pub struct MapBlock {
    pub flags: u8,
    /// Average colour of the top face before any tint, 0xRRGGBB.
    pub rgb: u32,
    pub tint: Tint,
}

impl MapBlock {
    pub fn is(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn wet(&self) -> bool {
        self.flags & (WATER | WATERLOGGED) != 0
    }
}

impl Default for MapBlock {
    fn default() -> Self {
        Self {
            flags: AIR,
            rgb: 0,
            tint: Tint::None,
        }
    }
}

pub struct BlockTable {
    /// Every state seen so far; 0 is air.
    pub palette: Palette,
    info: RwLock<Vec<MapBlock>>,
    faces: Arc<HashMap<String, CatalogBlock>>,
    catalog: Arc<Catalog>,
}

impl BlockTable {
    pub fn new(catalog: Arc<Catalog>, faces: Arc<HashMap<String, CatalogBlock>>) -> Self {
        let table = Self {
            palette: Palette::new(),
            info: RwLock::new(Vec::new()),
            faces,
            catalog,
        };
        table.intern(&BlockState::air());
        table
    }

    pub fn intern(&self, state: &BlockState) -> u32 {
        let id = self.palette.intern(state);
        if (id as usize) < self.info.read().unwrap().len() {
            return id;
        }
        let mut info = self.info.write().unwrap();
        while info.len() <= id as usize {
            let next = self.palette.with(info.len() as u32, |s| self.describe(s));
            info.push(next);
        }
        id
    }

    /// What the map knows of every block so far, by id.
    pub fn snapshot(&self) -> Vec<MapBlock> {
        self.info.read().unwrap().clone()
    }

    pub fn len(&self) -> usize {
        self.info.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn describe(&self, state: &BlockState) -> MapBlock {
        let name = state.name.as_str();
        if AIR_LIKE.contains(&name) {
            return MapBlock::default();
        }
        let mut flags = 0;
        if name == "minecraft:water" || name == "minecraft:bubble_column" {
            flags |= WATER;
        } else if WATER_PLANTS.contains(&name) || state.get("waterlogged") == Some("true") {
            flags |= WATERLOGGED;
        }
        let face = self.faces.get(name);
        let top = face.and_then(|f| f.top.as_ref().or(f.side.as_ref()));
        let rgb = top
            .and_then(|t| u32::from_str_radix(&t.rgb, 16).ok())
            .unwrap_or(0x808080);
        let tint = top
            .and_then(|t| t.tint.as_deref())
            .and_then(|t| u32::from_str_radix(t, 16).ok())
            .map(Tint::from_catalog)
            .unwrap_or(Tint::None);
        let is = |c: &str| self.catalog.is_a(name, c);
        let leaves = name.ends_with("leaves") || is("Leaves") || is("class_2397");
        let class = self
            .catalog
            .block(name)
            .map(|b| b.class.as_str())
            .unwrap_or("");
        let tree = leaves
            || class.contains(".trees.")
            || [
                "_log",
                "_wood",
                "branch",
                "_stem",
                "hyphae",
                "mushroom_block",
            ]
            .iter()
            .any(|w| name.contains(w));
        if tree {
            flags |= TREE;
        }
        let opaque = face.and_then(|f| f.opaque).unwrap_or(true);
        let coverage = top.and_then(|t| t.coverage).unwrap_or(1.0);
        if !leaves && flags & WATER == 0 && !opaque && coverage < 0.6 {
            flags |= SEE_THROUGH;
        }
        MapBlock { flags, rgb, tint }
    }
}

/// Biome names, numbered as they turn up.
#[derive(Default)]
pub struct Names {
    inner: RwLock<(Vec<String>, HashMap<String, u32>)>,
}

impl Names {
    pub fn intern(&self, name: &str) -> u32 {
        if let Some(&id) = self.inner.read().unwrap().1.get(name) {
            return id;
        }
        let mut inner = self.inner.write().unwrap();
        if let Some(&id) = inner.1.get(name) {
            return id;
        }
        let id = inner.0.len() as u32;
        inner.0.push(name.to_string());
        inner.1.insert(name.to_string(), id);
        id
    }

    pub fn all(&self) -> Vec<String> {
        self.inner.read().unwrap().0.clone()
    }
}
