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
//! leaves out any wood the ground cuts off from the rest, works out each branch's arms and
//! post the way Conquest does, and writes the blocks. The world is only read in the columns
//! being written, and other trees' blocks count as air there, so a tree's shape comes from
//! its request and the ground alone. Where crowns overlap, the tree built later wins the
//! shared cell: its leaves and wood replace leaves, but nothing replaces wood. The engine
//! builds trees in a fixed order, so neighbouring pieces of the map agree.
//!
//! `tools/tree-render` draws side views of every species with [`render`].

mod kit;
mod plan;
pub mod render;
mod species;

use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::sink::{BlockId, BlockSink, Blocks, AIR};
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
    /// A coconut palm: a slender, curving trunk under a star of drooping fronds.
    Palm,
    /// A lush tropical broadleaf: a tall, buttressed trunk under a wide, flat crown.
    Jungle,
    /// A spruce with snow lying on its needles.
    SnowySpruce,
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
        Species::Palm,
        Species::Jungle,
        Species::SnowySpruce,
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
            Species::Palm => "Palm",
            Species::Jungle => "Jungle tree",
            Species::SnowySpruce => "Snowy spruce",
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
        self.cut_off(&mut plan, &*sink, origin, &replace);
        let water = self.kits.water;
        let mut writes = Vec::new();
        for (lx, ly, lz, cell) in plan.cells() {
            let (x, y, z) = (lx + origin.0, ly + origin.1, lz + origin.2);
            let target = sink.get(x, y, z);
            let wet = target == water;
            // Where crowns overlap, the later tree wins: its leaves, snow and wood replace
            // leaves and the snow on them, and nothing replaces another tree's wood.
            let soft = self.kits.is_leaves(target) || self.kits.is_snow(target);
            let free = target == AIR
                || soft
                || match cell.piece {
                    Piece::Leaves(_) | Piece::Snow(_) => false,
                    _ => wet || replace.contains(&(x, y, z)),
                };
            if !free {
                continue;
            }
            let id = match cell.piece {
                Piece::Leaves(l) => self.kits.leaves[l as usize],
                Piece::Snow(l) => self.kits.snow[l as usize - 1],
                piece => {
                    let mossy = (cell.flags & MOSSY != 0) as u8;
                    let kit = &self.kits.woods[(cell.wood + mossy) as usize];
                    let arms = match piece {
                        Piece::Branch(_) => self.branch_bits(&plan, &*sink, origin, (lx, ly, lz)),
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
    /// decides how it meets a Conquest layer block there. The layer stays, and the bottom
    /// piece stands on it reaching down into it (`down=true`, or `extension_toggle=true` on a
    /// vertical slab; a branch post becomes a pillar as wide). Only a full block takes the
    /// layer's place, sunk into the ground: a thinner piece there would leave a pit round
    /// itself. Returns the layer cells the trunk replaces.
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
        let mut raised = Vec::new();
        for (x, low, z, cell) in feet {
            let (wx, wz) = (x + ox, z + oz);
            let (from, to) = (low + oy + 3, low + oy - species::BELOW + 1);
            let Some((g, layers)) = self.ground(sink, wx, from, wz, to) else {
                continue;
            };
            let g = g - oy;
            let foot = Cell {
                flags: cell.flags & !FOOT,
                links: 0,
                ..cell
            };
            for y in g + 1..low {
                plan.set(x, y, z, foot);
            }
            // What the ground has swallowed is never written: leave it out of the plan, so
            // nothing counts on it.
            for y in low..g + (layers >= 8) as i32 {
                plan.set(x, y, z, Cell::default());
            }
            if layers >= 8 {
                continue;
            }
            // Where the trunk starts in the layer block, its first piece (the flared foot)
            // moves up onto it in place of the second, unless that one joins a limb.
            let in_layer = plan.get(x, g, z);
            let above = plan.get(x, g + 1, z);
            let mut bottom = if in_layer.piece.is_wood() && above.links == 0 {
                in_layer
            } else {
                Cell {
                    flags: above.flags | in_layer.flags & STUMP,
                    ..above
                }
            };
            bottom.flags &= !FOOT;
            match bottom.piece {
                Piece::Log | Piece::Wood => {
                    plan.set(x, g, z, bottom);
                    replace.push((wx, g + oy, wz));
                    continue;
                }
                Piece::Branch(t) if bottom.links == 0 => {
                    bottom.piece = Piece::post_pillar(t);
                    bottom.flags |= DOWN;
                }
                // A limb lying on the ground rests on a block of bark sunk in its place.
                Piece::Branch(_) => {
                    let bark = Cell {
                        piece: Piece::Wood,
                        links: 0,
                        ..bottom
                    };
                    plan.set(x, g, z, bark);
                    replace.push((wx, g + oy, wz));
                    continue;
                }
                p if p.has_down() => bottom.flags |= DOWN,
                _ => continue,
            }
            plan.set(x, g + 1, z, bottom);
            plan.set(x, g, z, Cell::default());
            raised.push([x, g + 1, z]);
        }
        // The ground may have swallowed what a raised piece of the foot leant on.
        plan.prune_apart(&raised);
        replace
    }

    /// Leaves out the wood the ground cuts off from the rest of the tree: past a low limb
    /// dipping into a bank, a stick lying loose in the grass. Cells side by side make one
    /// piece of wood here, and one reaching down meets its neighbours on the level below.
    fn cut_off(
        &self,
        plan: &mut Plan,
        sink: &dyn BlockSink,
        (ox, oy, oz): (i32, i32, i32),
        replace: &[(i32, i32, i32)],
    ) {
        let mut wood: HashMap<[i32; 3], (bool, usize)> = HashMap::new();
        let mut cells = Vec::new();
        for (x, y, z, cell) in plan.cells() {
            let at = (x + ox, y + oy, z + oz);
            let buried = self.is_ground(sink.get(at.0, at.1, at.2)) && !replace.contains(&at);
            if cell.piece.is_wood() && !buried {
                wood.insert([x, y, z], (cell.flags & DOWN != 0, usize::MAX));
                cells.push([x, y, z]);
            }
        }
        let mut sizes = Vec::new();
        for &first in &cells {
            if wood[&first].1 != usize::MAX {
                continue;
            }
            let part = sizes.len();
            let mut todo = vec![first];
            wood.get_mut(&first).unwrap().1 = part;
            let mut size = 0;
            while let Some([x, y, z]) = todo.pop() {
                size += 1;
                let down = wood[&[x, y, z]].0;
                let mut next = vec![[x, y - 1, z], [x, y + 1, z]];
                for (dx, dz) in SIDES {
                    next.push([x + dx, y, z + dz]);
                    if down {
                        next.push([x + dx, y - 1, z + dz]);
                    }
                    if wood.get(&[x + dx, y + 1, z + dz]).is_some_and(|w| w.0) {
                        next.push([x + dx, y + 1, z + dz]);
                    }
                }
                for n in next {
                    if let Some(w) = wood.get_mut(&n) {
                        if w.1 == usize::MAX {
                            w.1 = part;
                            todo.push(n);
                        }
                    }
                }
            }
            sizes.push(size);
        }
        // The first of the largest, whatever order the map keeps its cells in.
        let Some(main) = (0..sizes.len()).rev().max_by_key(|&p| sizes[p]) else {
            return;
        };
        for (c, (_, part)) in wood {
            if part != main {
                plan.set(c[0], c[1], c[2], Cell::default());
            }
        }
    }

    /// Whether a block is ground a tree can stand on or a branch rest on: anything but air,
    /// water and the blocks of other trees, so no tree's shape depends on its neighbours.
    fn is_ground(&self, id: BlockId) -> bool {
        id != AIR && id != self.kits.water && !self.kits.is_tree(id)
    }

    /// The top of the ground in a column, scanning down from `from` to `to`: its Y and the
    /// Conquest layer count of the block there (8 for a full block).
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
            if !self.is_ground(id) {
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

    /// Arms and post of a branch piece: an arm along each side its limb runs through (to the
    /// next branch, or into the trunk it leaves, which closes the gap there), and a post when
    /// the tree itself or the ground is below, as Conquest's own rule has it (other trees don't
    /// count). Conquest joins every pair of neighbouring branches when a block next to them
    /// changes; the plan joins only each limb's own path, so limbs that pass side by side read
    /// as separate limbs. A pillar's top face isn't sturdy, so the game would drop the post
    /// above one on the next block update; it is kept so the trunk reads as one piece.
    fn branch_bits(
        &self,
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
        let below = sink.get(x + ox, y - 1 + oy, z + oz);
        if plan.piece(x, y - 1, z) != Piece::Empty || self.is_ground(below) {
            bits |= 16;
        }
        bits
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::catalog::Catalog;
    use crate::column::Rect;
    use crate::sink::Palette;
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

    /// A tree comes out the same next to a neighbour, except where the neighbour's wood was
    /// there first; and it wins every cell of the neighbour's leaves it wants.
    #[test]
    fn neighbours_do_not_change_a_tree() {
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
        let rect = Rect::new(-24, -24, 25, 25);
        let ground = || {
            let mut area = Area::new(rect, &palette);
            for (x, z) in rect.columns() {
                area.fill_column(x, z, 50, 64, grass);
            }
            area
        };
        let req = |species, x, seed| TreeRequest {
            species,
            x,
            y: 64,
            z: 0,
            size: 0.8,
            seed,
        };
        for (n, &species) in Species::ALL.iter().enumerate() {
            let tree = req(species, 0, 7 + n as u64);
            let neighbour = req(Species::Oak, 5, 99);
            let (mut alone, mut first, mut both) = (ground(), ground(), ground());
            trees.build(&tree, &mut alone);
            trees.build(&neighbour, &mut first);
            trees.build(&neighbour, &mut both);
            trees.build(&tree, &mut both);
            for (x, z) in rect.columns() {
                for y in 64..110 {
                    let mine = alone.get(x, y, z);
                    let theirs = first.get(x, y, z);
                    let got = both.get(x, y, z);
                    if mine == AIR {
                        assert_eq!(got, theirs, "{species:?} changed ({x}, {y}, {z})");
                    } else if trees.kits.is_tree(theirs)
                        && !trees.kits.is_leaves(theirs)
                        && !trees.kits.is_snow(theirs)
                    {
                        assert_eq!(got, theirs, "{species:?} replaced wood at ({x}, {y}, {z})");
                    } else {
                        assert_eq!(got, mine, "{species:?} differs at ({x}, {y}, {z})");
                    }
                }
            }
        }
    }

    /// A forest with trunks four blocks apart, built as the engine builds two overlapping
    /// pieces: each gets every tree standing within `MAX_REACH` of its rect, in the same order.
    /// Trees near one piece's far edge are missing from the other, and the shared columns must
    /// still come out the same.
    #[test]
    fn dense_forest_pieces_agree() {
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
        let mut forest = Vec::new();
        for gx in -12..24 {
            for gz in -6..18 {
                let h = crate::noise::hash2(5, gx, gz);
                forest.push(TreeRequest {
                    species: Species::ALL[(h % Species::ALL.len() as u64) as usize],
                    x: gx as i32 * 4 + (h >> 8) as i32 % 3,
                    y: 64,
                    z: gz as i32 * 4 + (h >> 16) as i32 % 3,
                    size: ((h >> 24) & 0xFF) as f32 / 255.0,
                    seed: h,
                });
            }
        }
        forest.sort_by_key(|t| (t.x, t.z, t.seed));
        let piece = |rect: Rect| {
            let buffer = Rect::new(
                rect.x0 - MAX_REACH,
                rect.z0 - MAX_REACH,
                rect.x1 + MAX_REACH,
                rect.z1 + MAX_REACH,
            );
            let mut area = Area::new(buffer, &palette);
            for (x, z) in buffer.columns() {
                area.fill_column(x, z, 50, 64, grass);
            }
            for t in forest.iter().filter(|t| buffer.contains(t.x, t.z)) {
                trees.build(t, &mut area);
            }
            area
        };
        let a = piece(Rect::new(0, 0, 48, 48));
        let b = piece(Rect::new(24, 8, 72, 56));
        let shared = Rect::new(24, 8, 48, 48);
        let differ = shared
            .columns()
            .filter(|&(x, z)| a.column(x, z) != b.column(x, z))
            .count();
        assert_eq!(differ, 0, "{differ} shared columns differ");
    }

    /// Ground like the gallery world's: gently uneven, with a Conquest grass layer of 1 to 7
    /// on top of almost every column. Returns the area and the cells holding a layer.
    fn layered_ground<'p>(
        b: &Blocks,
        palette: &'p Palette,
        rect: Rect,
    ) -> (Area<'p>, std::collections::HashSet<(i32, i32, i32)>) {
        let grass = b
            .id("minecraft:grass_block", &[("snowy", "false")])
            .unwrap();
        let mut area = Area::new(rect, palette);
        let mut layers = std::collections::HashSet::new();
        for (x, z) in rect.columns() {
            let g = ground_height(x, z);
            let top = g.floor() as i32;
            area.fill_column(x, z, 50, top + 1, grass);
            let l = ((g - g.floor()) * 8.0) as usize;
            if l > 0 {
                let id = b
                    .id(
                        "conquest:grass_block_layer",
                        &[("layers", &l.to_string()), ("waterlogged", "false")],
                    )
                    .unwrap();
                area.set(x, top + 1, z, id);
                layers.insert((x, top + 1, z));
            }
        }
        (area, layers)
    }

    fn ground_height(x: i32, z: i32) -> f32 {
        64.0 + 0.7 * ((x as f32 * 0.21).sin() + (z as f32 * 0.17).cos())
            + 0.4 * ((x + z) as f32 * 0.09).sin()
    }

    /// What is wrong with the wood of the trees in `area`: pieces that touch neither the rest
    /// of their tree nor the ground, and pieces other than full blocks put in place of a
    /// Conquest layer, which leave a pit round them.
    fn wood_problems(
        trees: &Trees,
        area: &Area,
        catalog: &Catalog,
        layer_cells: &std::collections::HashSet<(i32, i32, i32)>,
    ) -> Vec<String> {
        /// A box of a block model: its cell, its corners in world pixels, and whether it is
        /// wood.
        type ModelBox = ((i32, i32, i32), [f32; 3], [f32; 3], bool);
        let palette = area.palette();
        let rect = area.rect;
        let state = |id| palette.with(id, |s| s.to_string());
        let mut boxes: Vec<ModelBox> = Vec::new();
        let mut at: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
        let mut problems = Vec::new();
        let wood: Vec<(i32, i32, i32)> = rect
            .columns()
            .flat_map(|(x, z)| (50..160).map(move |y| (x, y, z)))
            .filter(|&(x, y, z)| {
                let id = area.get(x, y, z);
                trees.kits.is_tree(id) && !trees.kits.is_leaves(id) && !trees.kits.is_snow(id)
            })
            .collect();
        let mut near = std::collections::HashSet::new();
        for &(x, y, z) in &wood {
            for d in 0..27 {
                near.insert((x + d % 3 - 1, y + d / 3 % 3 - 1, z + d / 9 - 1));
            }
        }
        for &c in &near {
            if !rect.contains(c.0, c.2) {
                continue;
            }
            let id = area.get(c.0, c.1, c.2);
            if id == AIR || trees.kits.is_leaves(id) || trees.kits.is_snow(id) {
                continue;
            }
            let ground = !trees.kits.is_tree(id);
            let solid = palette.with(id, |s| render::solid_boxes(s, catalog));
            if !ground && layer_cells.contains(&c) {
                let full = solid.len() == 1 && solid[0] == ([0.0; 3], [16.0; 3]);
                if !full {
                    problems.push(format!("pit round {} at {c:?}", state(id)));
                }
            }
            for (lo, hi) in solid {
                let o = [c.0 as f32 * 16.0, c.1 as f32 * 16.0, c.2 as f32 * 16.0];
                at.entry(c).or_default().push(boxes.len());
                boxes.push((
                    c,
                    [o[0] + lo[0], o[1] + lo[1], o[2] + lo[2]],
                    [o[0] + hi[0], o[1] + hi[1], o[2] + hi[2]],
                    ground,
                ));
            }
        }
        // Union-find over the wood boxes: a tree is one piece of wood, which stands on the
        // ground. (Through the ground doesn't count, or a lone post would pass.)
        let mut parent: Vec<usize> = (0..boxes.len()).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let touch = |a: usize, b: usize| {
            let (_, alo, ahi, _) = boxes[a];
            let (_, blo, bhi, _) = boxes[b];
            let mut overlapping = 0;
            for k in 0..3 {
                let o = ahi[k].min(bhi[k]) - alo[k].max(blo[k]);
                if o < -0.01 {
                    return false;
                }
                if o > 0.25 {
                    overlapping += 1;
                }
            }
            overlapping >= 2
        };
        let mut grounded = std::collections::HashSet::new();
        let mut on_ground = Vec::new();
        for i in 0..boxes.len() {
            let (c, _, _, ground) = boxes[i];
            if ground {
                continue;
            }
            for d in 0..27 {
                let n = (c.0 + d % 3 - 1, c.1 + d / 3 % 3 - 1, c.2 + d / 9 - 1);
                for &j in at.get(&n).map(Vec::as_slice).unwrap_or(&[]) {
                    if boxes[j].3 {
                        if touch(i, j) {
                            on_ground.push(i);
                        }
                    } else if n == c || touch(i, j) {
                        // The boxes of one block are one piece.
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        parent[a] = b;
                    }
                }
            }
        }
        for i in on_ground {
            grounded.insert(find(&mut parent, i));
        }
        let mut parts: HashMap<usize, Vec<(i32, i32, i32)>> = HashMap::new();
        for i in (0..boxes.len()).filter(|&i| !boxes[i].3) {
            let r = find(&mut parent, i);
            parts.entry(r).or_default().push(boxes[i].0);
        }
        let main = parts
            .iter()
            .max_by_key(|(_, cells)| cells.len())
            .map(|(&r, _)| r);
        for (r, mut cells) in parts {
            cells.dedup();
            if Some(r) == main {
                if !grounded.contains(&r) {
                    problems.push("the tree doesn't reach the ground".into());
                }
                continue;
            }
            let c = cells[0];
            problems.push(format!(
                "{} at {c:?} and {} more cells come apart from the tree",
                state(area.get(c.0, c.1, c.2)),
                cells.len() - 1
            ));
        }
        problems
    }

    /// No species leaves wood hanging in the air or pits in layer ground, at any size, on
    /// layer ground and on full blocks.
    #[test]
    fn wood_holds_together() {
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
        let mut problems = Vec::new();
        for &species in Species::ALL {
            for size in [0.05, 0.25, 0.5, 0.75, 1.0] {
                for seed in 0..8 {
                    let rect = Rect::new(-18, -18, 19, 19);
                    let flat = seed % 2 == 1;
                    let (mut area, layers) = if flat {
                        let mut area = Area::new(rect, &palette);
                        for (x, z) in rect.columns() {
                            area.fill_column(x, z, 50, 65, grass);
                        }
                        (area, Default::default())
                    } else {
                        layered_ground(&b, &palette, rect)
                    };
                    let req = TreeRequest {
                        species,
                        x: 0,
                        y: ground_height(0, 0).floor() as i32 + 1,
                        z: 0,
                        size,
                        seed,
                    };
                    trees.build(&req, &mut area);
                    for p in wood_problems(&trees, &area, &catalog, &layers) {
                        problems.push(format!("{species:?} {size} seed {seed}: {p}"));
                    }
                }
            }
        }
        for p in problems.iter().take(400) {
            eprintln!("{p}");
        }
        assert!(problems.is_empty(), "{} problems", problems.len());
    }
}
