//! Trees built from Conquest Reforged trunk, branch and leaf pieces.
//!
//! `decor` decides where trees stand and passes a [`TreeRequest`]; [`Trees::build`] writes the
//! tree through a [`BlockSink`]. A build must depend only on the request (its own seed, not a
//! shared random sequence) and must stay within [`MAX_REACH`] blocks of the trunk sideways,
//! because the engine rebuilds a tree in every piece of the map it overlaps and clips it there.
//!
//! A build has two steps. `species` lays the tree out in a [`plan::Plan`] from the request
//! alone: round trunks of logs, vertical slabs, vertical quarters and pillars; limbs of bark
//! and large, normal and small branches; leaves in loose clumps. Then [`Trees::build`] fits
//! the plan to the world: it stretches the trunk down to the ground of every column it covers,
//! works out each branch's arms and post the way Conquest does, and writes the blocks. The
//! world is only read in the columns being written, so neighbouring pieces of the map agree.
//!
//! `tools/tree-render` draws side views of every species with [`render`].

mod kit;
mod plan;
pub mod render;
mod species;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::sink::{BlockSink, Blocks, AIR};
use kit::Kits;
use plan::{Cell, Piece, Plan, DOWN, FOOT, MOSSY, SIDES, STUMP};

/// Furthest a tree may place a block from its trunk column, along x or z.
pub const MAX_REACH: i32 = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Species {
    Oak,
    Birch,
    Spruce,
    Pine,
    Willow,
    /// A dead or dying tree.
    Dead,
    /// A low shrub.
    Bush,
}

impl Species {
    pub const ALL: &'static [Species] = &[
        Species::Oak,
        Species::Birch,
        Species::Spruce,
        Species::Pine,
        Species::Willow,
        Species::Dead,
        Species::Bush,
    ];

    /// English name for the app.
    pub fn name(self) -> &'static str {
        match self {
            Species::Oak => "Oak",
            Species::Birch => "Birch",
            Species::Spruce => "Spruce",
            Species::Pine => "Pine",
            Species::Willow => "Willow",
            Species::Dead => "Dead tree",
            Species::Bush => "Bush",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeRequest {
    pub species: Species,
    /// Trunk column, and the Y of the cell the trunk starts in (see `ColumnInfo::ground`).
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// 0 = sapling-sized, 0.5 = typical adult, 1 = ancient giant.
    pub size: f32,
    /// Seed for this tree's shape.
    pub seed: u64,
}

pub struct Trees {
    kits: Kits,
}

impl Trees {
    pub fn new(blocks: &Blocks) -> Result<Self> {
        Ok(Self {
            kits: Kits::new(blocks)?,
        })
    }

    /// Rough height and crown radius in blocks, for previews.
    pub fn shape(&self, req: &TreeRequest) -> (f32, f32) {
        species::shape(req.species, req.size)
    }

    pub fn build(&self, req: &TreeRequest, sink: &mut dyn BlockSink) {
        let mut plan = species::plan(req);
        let origin = (req.x, req.y, req.z);
        let replace = self.settle(&mut plan, &*sink, origin);
        let water = self.kits.water;
        let mut writes = Vec::new();
        for (lx, ly, lz, cell) in plan.cells() {
            let (x, y, z) = (lx + origin.0, ly + origin.1, lz + origin.2);
            let target = sink.get(x, y, z);
            let wet = target == water;
            let free = match cell.piece {
                Piece::Leaves(_) => target == AIR,
                _ => {
                    target == AIR
                        || wet
                        || self.kits.leaves.contains(&target)
                        || replace.contains(&(x, y, z))
                }
            };
            if !free {
                continue;
            }
            let id = match cell.piece {
                Piece::Leaves(l) => self.kits.leaves[l as usize],
                piece => {
                    let mossy = (cell.flags & MOSSY != 0) as u8;
                    let kit = &self.kits.woods[(cell.wood + mossy) as usize];
                    let arms = match piece {
                        Piece::Branch(_) => branch_bits(&plan, &*sink, origin, (lx, ly, lz)),
                        _ => 0,
                    };
                    kit.id(
                        piece,
                        cell.flags & STUMP != 0,
                        cell.flags & DOWN != 0,
                        wet,
                        arms,
                    )
                }
            };
            writes.push((x, y, z, id));
        }
        for (x, y, z, id) in writes {
            sink.set(x, y, z, id);
        }
    }

    /// Stretches the bottom of the trunk down to the ground in every column it covers, and
    /// decides how it meets a Conquest layer block there: pillars and quarters stand on the
    /// layer with `down=true` (reaching into it), anything else replaces the layer block so
    /// no gap shows under it. Returns the layer cells the trunk may replace.
    fn settle(
        &self,
        plan: &mut Plan,
        sink: &dyn BlockSink,
        (ox, oy, oz): (i32, i32, i32),
    ) -> Vec<(i32, i32, i32)> {
        let mut feet: Vec<(i32, i32, i32, Cell)> = Vec::new();
        for (x, y, z, cell) in plan.cells() {
            if cell.flags & FOOT != 0 && !feet.iter().any(|f| f.0 == x && f.2 == z) {
                feet.push((x, y, z, cell)); // cells come bottom up: this is the lowest
            }
        }
        let mut replace = Vec::new();
        for (x, low, z, cell) in feet {
            let (wx, wz) = (x + ox, z + oz);
            let (from, to) = (low + oy + 3, low + oy - species::BELOW + 1);
            let Some((g, layers)) = self.ground(sink, wx, from, wz, to) else {
                continue;
            };
            let g = g - oy;
            let partial = layers < 8;
            // Below a root, only the post carries on down.
            let foot = Cell {
                flags: cell.flags & !FOOT,
                links: 0,
                ..cell
            };
            let bottom = if partial && !cell.piece.has_down() {
                g
            } else {
                g + 1
            };
            for y in bottom..low {
                plan.set(x, y, z, foot);
            }
            if partial {
                let mut at = plan.get(x, bottom, z);
                if at.piece.has_down() {
                    at.flags |= DOWN;
                } else {
                    replace.push((wx, g + oy, wz));
                    if let Piece::Slab(..) = at.piece {
                        at.piece = Piece::Wood;
                    }
                }
                plan.set(x, bottom, z, at);
            }
        }
        replace
    }

    /// The top of the ground in a column, scanning down from `from` to `to`: its Y and the
    /// Conquest layer count of the block there (8 for a full block). Water counts as air.
    fn ground(
        &self,
        sink: &dyn BlockSink,
        x: i32,
        from: i32,
        z: i32,
        to: i32,
    ) -> Option<(i32, u8)> {
        (to..=from).rev().find_map(|y| {
            let id = sink.get(x, y, z);
            if id == AIR || id == self.kits.water {
                return None;
            }
            let layers = sink.palette().with(id, |s| {
                s.get("layers")
                    .and_then(|l| l.parse::<u8>().ok())
                    .unwrap_or(8)
            });
            Some((y, layers))
        })
    }
}

/// Arms and post of a branch piece: an arm along each side its limb runs through (to the next
/// branch, or into the trunk it leaves, which closes the gap there), and a post when anything
/// stands below, as Conquest's own rule has it. Conquest joins every pair of neighbouring
/// branches when a block next to them changes; the plan joins only each limb's own path, so
/// limbs that pass side by side read as separate limbs. A pillar's top face isn't sturdy, so
/// the game would drop the post above one on the next block update; it is kept so the trunk
/// reads as one piece.
fn branch_bits(
    plan: &Plan,
    sink: &dyn BlockSink,
    (ox, oy, oz): (i32, i32, i32),
    (x, y, z): (i32, i32, i32),
) -> usize {
    let mut bits = 0;
    let links = plan.get(x, y, z).links;
    for (s, (dx, dz)) in SIDES.iter().enumerate() {
        if links & (1 << s) != 0 && plan.piece(x + dx, y, z + dz).is_wood() {
            bits |= 1 << s;
        }
    }
    if plan.piece(x, y - 1, z) != Piece::Empty || sink.get(x + ox, y - 1 + oy, z + oz) != AIR {
        bits |= 16;
    }
    bits
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::catalog::Catalog;
    use crate::column::Rect;
    use crate::sink::{BlockId, Palette};
    use crate::world::Area;

    fn catalog() -> Option<Catalog> {
        let path = std::env::var("VISTAFORGE_CATALOG")
            .unwrap_or_else(|_| "/mnt/project-files/vistaforge/catalog-cr-1.7.0.json".into());
        Catalog::load(Path::new(&path)).ok()
    }

    /// Every species builds onto uneven ground of grass blocks and Conquest layers without
    /// floating: each trunk column reaches the ground.
    #[test]
    fn trees_stand_on_uneven_ground() {
        let Some(catalog) = catalog() else {
            eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
            return;
        };
        let palette = Palette::new();
        let b = Blocks::new(&catalog, &palette);
        let trees = Trees::new(&b).unwrap();
        let grass = b
            .id("minecraft:grass_block", &[("snowy", "false")])
            .unwrap();
        let layer: Vec<BlockId> = (1..=7)
            .map(|l| {
                b.id(
                    "conquest:grass_block_layer",
                    &[("layers", &l.to_string()), ("waterlogged", "false")],
                )
                .unwrap()
            })
            .collect();
        for (n, &species) in Species::ALL.iter().enumerate() {
            for size in [0.05, 0.3, 0.6, 1.0] {
                let rect = Rect::new(-20, -20, 21, 21);
                let mut area = Area::new(rect, &palette);
                // A slope: ground rises one block every four columns, with layers between.
                for (x, z) in rect.columns() {
                    let h = 60.0 + x as f32 * 0.27 + z as f32 * 0.11;
                    let top = h.floor() as i32;
                    area.fill_column(x, z, 50, top, grass);
                    let l = ((h - h.floor()) * 8.0) as usize;
                    if l > 0 {
                        area.set(x, top, z, layer[l - 1]);
                    }
                }
                let col = crate::column::ColumnInfo::dry(60.0, crate::biome::Biome::Plains);
                let (y, _) = col.ground();
                let req = TreeRequest {
                    species,
                    x: 0,
                    y,
                    z: 0,
                    size,
                    seed: 1000 + n as u64,
                };
                trees.build(&req, &mut area);
                let plan = species::plan(&req);
                for (x, _, z, cell) in plan.cells() {
                    if cell.flags & FOOT == 0 {
                        continue;
                    }
                    let (x, z) = (x + req.x, z + req.z);
                    // Find the lowest tree block above the grass blocks. A root that runs into
                    // the slope leaves nothing in its column.
                    let col = area.column(x, z);
                    let Some(lowest) = (51..req.y + 4).find(|&y| {
                        let id = col[(y - crate::world::MIN_Y) as usize];
                        id != AIR && id != grass && !layer.contains(&id)
                    }) else {
                        assert!(
                            x.abs() > 1 || z.abs() > 1,
                            "{species:?} {size}: no trunk at ({x}, {z})"
                        );
                        continue;
                    };
                    let below = col[(lowest - 1 - crate::world::MIN_Y) as usize];
                    assert!(
                        below == grass || layer.contains(&below),
                        "{species:?} {size}: trunk at ({x}, {lowest}, {z}) floats"
                    );
                }
            }
        }
        crate::engine::validate_palette(&palette, &catalog).unwrap();
    }
}
