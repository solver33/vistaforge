//! Decor: everything that stands on the ground — where trees grow and which species, and the
//! plants, flowers, rocks, fallen logs and other detail between them.
//!
//! Every decision must be a function of the seed and the position (hash the coordinates, as
//! [`hash2`] does), never of a random sequence shared across the map: the engine plans the same
//! trees and plants again for each overlapping piece and they have to come out identical.
//! Nothing may be placed more than [`MAX_REACH`] blocks from the column it belongs to.
//!
//! The current implementation is the phase-0 meadow: oaks on a jittered grid and meadow plants.

use anyhow::Result;

use crate::biome::Biome;
use crate::column::{ColumnGrid, ColumnInfo, Rect};
use crate::noise::{hash2, unit2};
use crate::settings::WorldSettings;
use crate::sink::{BlockId, BlockSink, Blocks, AIR};
use crate::trees::{Species, TreeRequest};

/// Furthest decor (other than trees) may reach from the column it belongs to.
pub const MAX_REACH: i32 = 8;

pub struct Decor {
    seed: u64,
    tree_cell: i32,
    plant_chance: f64,
    /// `[plant][layers - 1]`: each plant at every height of the layer it stands on (8 = full block).
    plants: Vec<[BlockId; 8]>,
    /// Blocks a plant may stand on.
    soil: Vec<BlockId>,
}

impl Decor {
    pub fn new(b: &Blocks, settings: &WorldSettings) -> Result<Self> {
        let names = [
            "conquest:common_meadow_grass",
            "conquest:grass",
            "conquest:timothy_grass",
            "conquest:kentucky_bluegrass",
            "conquest:buttercups",
            "conquest:red_clover_flowers",
            "conquest:oxeye_daisy",
        ];
        let mut plants = Vec::new();
        for name in names {
            let mut ids = [AIR; 8];
            for (i, id) in ids.iter_mut().enumerate() {
                *id = b.id(
                    name,
                    &[
                        ("layers", &(i + 1).to_string()),
                        ("offset_toggle", "false"),
                        ("waterlogged", "false"),
                    ],
                )?;
            }
            plants.push(ids);
        }
        let mut soil = vec![b.id("minecraft:grass_block", &[("snowy", "false")])?];
        for layer in [
            "conquest:grass_block_layer",
            "conquest:clover_covered_grass_layer",
        ] {
            for l in 1..=7 {
                soil.push(b.id(
                    layer,
                    &[("layers", &l.to_string()), ("waterlogged", "false")],
                )?);
            }
        }
        let trees = settings.sliders.trees.clamp(0.0, 1.0);
        Ok(Self {
            seed: settings.seed,
            tree_cell: (60.0 - 40.0 * trees) as i32,
            plant_chance: 0.32 * settings.sliders.vegetation.clamp(0.0, 1.0) as f64,
            plants,
            soil,
        })
    }

    /// Trees whose trunks stand in `rect`. `column` gives the terrain of any column.
    pub fn plan_trees(
        &self,
        rect: Rect,
        column: &dyn Fn(i32, i32) -> ColumnInfo,
    ) -> Vec<TreeRequest> {
        let cell = self.tree_cell.max(8);
        let mut out = Vec::new();
        let (gx0, gz0) = (rect.x0.div_euclid(cell), rect.z0.div_euclid(cell));
        let (gx1, gz1) = (
            (rect.x1 - 1).div_euclid(cell),
            (rect.z1 - 1).div_euclid(cell),
        );
        for gx in gx0..=gx1 {
            for gz in gz0..=gz1 {
                let h = hash2(self.seed ^ 0x7EE5, gx as i64, gz as i64);
                if (h >> 60) < 4 {
                    continue; // a quarter of the cells stay empty
                }
                let jitter = |shift: u32| ((h >> shift) % (cell as u64 - 4)) as i32 + 2;
                let (x, z) = (gx * cell + jitter(0), gz * cell + jitter(16));
                if !rect.contains(x, z) {
                    continue;
                }
                let col = column(x, z);
                if col.is_wet() || col.biome != Biome::Plains || col.slope > 0.6 {
                    continue;
                }
                let (y, _) = col.ground();
                out.push(TreeRequest {
                    species: Species::Oak,
                    x,
                    y,
                    z,
                    size: ((h >> 32) & 0xFF) as f32 / 255.0,
                    seed: h,
                });
            }
        }
        out
    }

    /// Plants and other ground detail for the columns of `rect`, after the trees are in.
    pub fn decorate(&self, rect: Rect, grid: &ColumnGrid, sink: &mut dyn BlockSink) {
        for (x, z) in rect.columns() {
            let Some(col) = grid.try_get(x, z) else {
                continue;
            };
            if col.is_wet() || !matches!(col.biome, Biome::Plains | Biome::Meadow) {
                continue;
            }
            if unit2(self.seed ^ 0x9A47, x as i64, z as i64) > self.plant_chance {
                continue;
            }
            let (y, layers) = col.ground();
            // A plant on a layer sits in the cell above it and takes the layer's height.
            let (py, below, height) = if layers > 0 {
                (y + 1, y, layers)
            } else {
                (y, y - 1, 8)
            };
            if !self.soil.contains(&sink.get(x, below, z)) || sink.get(x, py, z) != AIR {
                continue;
            }
            let pick = hash2(self.seed ^ 0x9A48, x as i64, z as i64) as usize % self.plants.len();
            sink.set(x, py, z, self.plants[pick][height as usize - 1]);
        }
    }
}
