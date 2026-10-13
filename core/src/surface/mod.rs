//! Surface: the blocks of each column — bedrock, stone, soil, the top block or Conquest layer,
//! and water — plus the Minecraft biome written for the column (grass, foliage and water
//! colours) and its colour in the app's preview.
//!
//! [`Surface::fill`] must depend only on the seed, the column position and the column grid, so
//! overlapping pieces of the map agree.
//!
//! The current implementation is the phase-0 meadow (grass and clover layers, sandy shores,
//! a gravel and sand lake bed); it ignores most biomes until the real surface rules land.

use std::collections::HashMap;

use anyhow::Result;

use crate::biome::Biome;
use crate::column::{ColumnGrid, ColumnInfo};
use crate::noise::fbm2;
use crate::settings::WorldSettings;
use crate::sink::{BlockId, BlockSink, Blocks};
use crate::world::MIN_Y;

pub struct Surface {
    seed: u64,
    bedrock: BlockId,
    stone: BlockId,
    dirt: BlockId,
    grass: BlockId,
    sand: BlockId,
    gravel: BlockId,
    water: BlockId,
    /// Layers 1..=7 (index 0 is layers=1).
    grass_layers: [BlockId; 7],
    clover_layers: [BlockId; 7],
    sand_layers: [BlockId; 7],
    gravel_layers_wet: [BlockId; 7],
    sand_layers_wet: [BlockId; 7],
    /// Average top colour of every block this module places, for previews.
    colors: HashMap<BlockId, [u8; 3]>,
}

fn layers(b: &Blocks, id: &str, waterlogged: bool) -> Result<[BlockId; 7]> {
    let wl = if waterlogged { "true" } else { "false" };
    let mut out = [0; 7];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = b.id(id, &[("layers", &(i + 1).to_string()), ("waterlogged", wl)])?;
    }
    Ok(out)
}

impl Surface {
    pub fn new(b: &Blocks, settings: &WorldSettings) -> Result<Self> {
        let mut s = Self {
            seed: settings.seed,
            bedrock: b.default_id("minecraft:bedrock")?,
            stone: b.default_id("minecraft:stone")?,
            dirt: b.default_id("minecraft:dirt")?,
            grass: b.id("minecraft:grass_block", &[("snowy", "false")])?,
            sand: b.default_id("minecraft:sand")?,
            gravel: b.default_id("minecraft:gravel")?,
            water: b.id("minecraft:water", &[("level", "0")])?,
            grass_layers: layers(b, "conquest:grass_block_layer", false)?,
            clover_layers: layers(b, "conquest:clover_covered_grass_layer", false)?,
            sand_layers: layers(b, "conquest:sand_layer", false)?,
            gravel_layers_wet: layers(b, "conquest:gravel_layer", true)?,
            sand_layers_wet: layers(b, "conquest:wet_sand_layer", true)?,
            colors: HashMap::new(),
        };
        let mut ids = vec![
            s.bedrock, s.stone, s.dirt, s.grass, s.sand, s.gravel, s.water,
        ];
        for set in [
            s.grass_layers,
            s.clover_layers,
            s.sand_layers,
            s.gravel_layers_wet,
            s.sand_layers_wet,
        ] {
            ids.extend(set);
        }
        for id in ids {
            let rgb = b
                .palette
                .with(id, |st| b.catalog.block(&st.name).and_then(|i| i.top_rgb))
                .unwrap_or(0x7f7f7f);
            s.colors
                .insert(id, [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
        }
        Ok(s)
    }

    /// The Minecraft biome written for this column.
    pub fn minecraft_biome(&self, col: &ColumnInfo) -> &'static str {
        match col.biome {
            Biome::Plains | Biome::Beach | Biome::Lake => "minecraft:plains",
            Biome::Meadow => "minecraft:meadow",
            Biome::Forest => "minecraft:forest",
            Biome::BirchForest => "minecraft:birch_forest",
            Biome::Taiga => "minecraft:taiga",
            Biome::PineForest => "minecraft:old_growth_pine_taiga",
            Biome::Swamp | Biome::Marsh => "minecraft:swamp",
            Biome::Highlands => "minecraft:windswept_hills",
            Biome::Mountains => "minecraft:stony_peaks",
            Biome::AlpineMeadow => "minecraft:meadow",
            Biome::SnowyPeaks => "minecraft:snowy_slopes",
            Biome::River => "minecraft:river",
        }
    }

    /// The top full block and the layer block above it (if any) for a column.
    fn top(&self, x: i32, z: i32, col: &ColumnInfo) -> (BlockId, Option<BlockId>) {
        let (_, layers) = col.ground();
        let water = col.water_level;
        let shallow = water.is_some_and(|w| col.height > w as f32 - 1.5);
        let sandy = col.biome == Biome::Beach || shallow;
        let clover = fbm2(self.seed ^ 0xC10, x as f64 / 24.0, z as f64 / 24.0, 2) > 0.25;
        let full = if water.is_some() && !shallow {
            self.gravel
        } else if sandy {
            self.sand
        } else if layers == 0 {
            self.grass
        } else {
            self.dirt
        };
        let layer = (layers > 0).then(|| {
            let l = layers as usize - 1;
            if shallow {
                self.sand_layers_wet[l]
            } else if water.is_some() {
                self.gravel_layers_wet[l]
            } else if sandy {
                self.sand_layers[l]
            } else if clover {
                self.clover_layers[l]
            } else {
                self.grass_layers[l]
            }
        });
        (full, layer)
    }

    pub fn fill(&self, x: i32, z: i32, grid: &ColumnGrid, sink: &mut dyn BlockSink) {
        let col = grid.get(x, z);
        let (y, _) = col.ground();
        let (top, layer) = self.top(x, z, col);
        let soil = if top == self.grass { self.dirt } else { top };
        sink.set(x, MIN_Y, z, self.bedrock);
        sink.fill_column(x, z, MIN_Y + 1, y - 4, self.stone);
        sink.fill_column(x, z, y - 4, y - 1, soil);
        sink.set(x, y - 1, z, top);
        let mut surface = y;
        if let Some(layer) = layer {
            sink.set(x, y, z, layer);
            surface += 1;
        }
        if let Some(w) = col.water_level {
            sink.fill_column(x, z, surface, w + 1, self.water);
        }
    }

    pub fn preview_color(&self, x: i32, z: i32, col: &ColumnInfo) -> [u8; 3] {
        let (top, layer) = self.top(x, z, col);
        let c = self.colors[&layer.unwrap_or(top)];
        if col.is_wet() {
            // Plains water colour over the bed, darker with depth.
            let t = (0.45 + col.depth() * 0.08).min(0.85);
            let w = [63.0, 118.0, 228.0];
            let mix = |i: usize| (c[i] as f32 * (1.0 - t) + w[i] * t) as u8;
            [mix(0), mix(1), mix(2)]
        } else {
            c
        }
    }
}
