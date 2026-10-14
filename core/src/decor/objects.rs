//! Things bigger than a plant: boulders, fallen logs and stumps.
//!
//! Objects are anchored one per [`CELL`]×[`CELL`] cell at a hashed spot. Whether an object is
//! built, and its shape, depend only on the seed, the anchor and grid columns within
//! [`super::MAX_REACH`] of it (and the trees, which are worked out from the same columns), so
//! every piece of the map that overlaps an object builds it the same way. Each block is then
//! written only where the cell is free (air, water or thin ground), checked cell by cell.

use anyhow::Result;

use crate::biome::Biome;
use crate::column::ColumnGrid;
use crate::noise::{hash2, hash64};
use crate::sink::{BlockId, BlockSink, Blocks, AIR};
use crate::surface::field::{hash_unit, pick};
use crate::surface::materials::Kind;
use crate::surface::{Surface, Top, Zone};

use super::flora::Flora;

/// Side of an object cell.
pub(crate) const CELL: i32 = 10;
const SALT: u64 = 0x0B1E_C700;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wood {
    Oak,
    Birch,
    Spruce,
    Pine,
    Willow,
}

const WOODS: [(Wood, &str, &str); 5] = [
    (Wood::Oak, "minecraft:oak_log", "oak"),
    (Wood::Birch, "minecraft:birch_log", "birch"),
    (Wood::Spruce, "conquest:norway_spruce_log", "norway_spruce"),
    (Wood::Pine, "conquest:pine_log", "pine"),
    (Wood::Willow, "conquest:willow_log", "willow"),
];

/// Blocks of one wood type for logs and stumps.
struct WoodKit {
    /// Lying along x and along z; plain and moss-grown.
    log: [[BlockId; 2]; 2],
    /// A large branch with one arm, towards north, east, south, west: the thin end of a log.
    tip: [BlockId; 4],
    /// `[mossy][thick]`: 8 px and 12 px wide stumps.
    stump: [[BlockId; 2]; 2],
}

fn wood_kit(b: &Blocks, log: &str, wood: &str) -> Result<WoodKit> {
    let mossy_log = format!("conquest:mossy_{wood}_log");
    let axis = |name: &str, a: &str| b.id(name, &[("axis", a)]);
    let mut tip = [AIR; 4];
    for (i, dir) in ["north", "east", "south", "west"].iter().enumerate() {
        let mut props = vec![("up", "false"), ("waterlogged", "false")];
        for side in ["north", "east", "south", "west"] {
            props.push((side, if side == *dir { "true" } else { "false" }));
        }
        tip[i] = b.id(&format!("conquest:{wood}_log_large_branch"), &props)?;
    }
    let stump = |mossy: bool, layer: &str| {
        let name = if mossy {
            format!("conquest:mossy_{wood}_log_stump")
        } else {
            format!("conquest:{wood}_log_stump")
        };
        b.id(
            &name,
            &[
                ("layer", layer),
                ("down", "false"),
                ("waterlogged", "false"),
            ],
        )
    };
    Ok(WoodKit {
        log: [
            [axis(log, "x")?, axis(&mossy_log, "x")?],
            [axis(log, "z")?, axis(&mossy_log, "z")?],
        ],
        tip,
        stump: [
            [stump(false, "2")?, stump(false, "3")?],
            [stump(true, "2")?, stump(true, "3")?],
        ],
    })
}

/// Wood of the fallen logs and stumps of a biome.
fn woods(biome: Biome) -> &'static [(Wood, u32)] {
    use Wood::*;
    match biome.fallback() {
        Biome::Forest => &[(Oak, 4), (Birch, 1)],
        Biome::BirchForest => &[(Birch, 4), (Oak, 1)],
        Biome::Taiga => &[(Spruce, 4), (Pine, 1), (Birch, 1)],
        Biome::PineForest => &[(Pine, 4), (Spruce, 1)],
        Biome::Swamp | Biome::Marsh => &[(Willow, 3), (Oak, 1)],
        Biome::Highlands => &[(Birch, 1), (Pine, 1)],
        Biome::Mountains | Biome::AlpineMeadow | Biome::SnowyPeaks => &[(Spruce, 2), (Pine, 1)],
        Biome::Beach | Biome::River | Biome::Lake => &[(Willow, 1), (Oak, 1), (Birch, 1)],
        Biome::Plains | Biome::Meadow => &[(Oak, 3), (Birch, 1)],
        Biome::Tropical | Biome::SnowyTaiga | Biome::Tundra | Biome::Volcanic => {
            unreachable!("fallback() returns one of the original biomes")
        }
    }
}

/// Chances of a boulder, a fallen log and a stump per object cell, before the detail slider.
fn chances(biome: Biome, zone: Zone) -> (f32, f32, f32) {
    use Biome as B;
    let wooded = zone == Zone::Wood;
    let (boulder, log, stump) = match biome.fallback() {
        B::Plains if wooded => (0.03, 0.12, 0.06),
        B::Plains => (0.03, 0.0, 0.004),
        B::Meadow if wooded => (0.03, 0.1, 0.05),
        B::Meadow => (0.02, 0.0, 0.0),
        B::Forest if wooded => (0.06, 0.3, 0.12),
        B::Forest => (0.04, 0.05, 0.08),
        B::BirchForest if wooded => (0.05, 0.25, 0.1),
        B::BirchForest => (0.04, 0.04, 0.06),
        B::Taiga if wooded => (0.08, 0.35, 0.12),
        B::Taiga => (0.07, 0.04, 0.06),
        B::PineForest if wooded => (0.07, 0.25, 0.1),
        B::PineForest => (0.07, 0.03, 0.05),
        B::Swamp if wooded => (0.02, 0.35, 0.15),
        B::Swamp => (0.02, 0.18, 0.1),
        B::Marsh => (0.02, 0.03, 0.03),
        // Few on mountains: on open slopes, seen from afar, a boulder reads as a pasted lump.
        B::Highlands if wooded => (0.05, 0.1, 0.05),
        B::Highlands => (0.05, 0.0, 0.0),
        B::Mountains if wooded => (0.05, 0.12, 0.05),
        B::Mountains => (0.05, 0.0, 0.0),
        B::AlpineMeadow if wooded => (0.04, 0.1, 0.04),
        B::AlpineMeadow => (0.04, 0.0, 0.0),
        B::SnowyPeaks => (0.02, 0.0, 0.0),
        B::Beach => (0.03, 0.02, 0.0),
        B::River => (0.1, 0.03, 0.02),
        B::Lake => (0.04, 0.02, 0.01),
        B::Tropical | B::SnowyTaiga | B::Tundra | B::Volcanic => {
            unreachable!("fallback() returns one of the original biomes")
        }
    };
    match zone {
        Zone::Stony => (boulder, log * 0.4, stump * 0.4),
        // Scree and bare rock are rough enough already.
        Zone::Scree => (boulder * 0.6, 0.0, 0.0),
        Zone::Rock => (boulder * 0.3, 0.0, 0.0),
        Zone::Shore => (boulder * 1.5, log, stump * 0.5),
        Zone::Wet => (boulder * 0.6, log, stump),
        Zone::Bed(d) => {
            let b = if biome == B::River {
                boulder.max(0.08)
            } else {
                boulder
            };
            (if d < 2 { b } else { b * 0.4 }, 0.0, 0.0)
        }
        Zone::Open | Zone::Wood => (boulder, log, stump),
    }
}

fn damp(biome: Biome, top: &Top, moisture: f32) -> bool {
    moisture > 0.6
        || top.zone == Zone::Wet
        || (top.zone == Zone::Wood
            && matches!(
                biome,
                Biome::Forest | Biome::Taiga | Biome::Swamp | Biome::BirchForest
            ))
        || matches!(biome, Biome::Swamp | Biome::Marsh)
}

pub(crate) struct Objects {
    seed: u64,
    woods: Vec<(Wood, WoodKit)>,
    /// Bracket fungi `[birch polypore, turkey tail][facing n/e/s/w][toggle]`.
    fungi: [[[BlockId; 3]; 4]; 2],
    water: BlockId,
    /// The detail slider as a factor (1 at the default).
    amount: f32,
}

/// What decides whether a cell may take a block of an object.
pub(crate) fn free(surface: &Surface, water: BlockId, id: BlockId) -> bool {
    id == AIR
        || id == water
        || surface
            .kit
            .of_block(id)
            .is_some_and(|(_, layers)| layers > 0)
}

impl Objects {
    pub fn new(b: &Blocks, seed: u64, detail: f32) -> Result<Self> {
        let mut woods = Vec::new();
        for (w, log, name) in WOODS {
            woods.push((w, wood_kit(b, log, name)?));
        }
        let mut fungi = [[[AIR; 3]; 4]; 2];
        for (k, name) in ["conquest:birch_polypore", "conquest:turkey_tail"]
            .iter()
            .enumerate()
        {
            for (f, facing) in ["north", "east", "south", "west"].iter().enumerate() {
                for (t, slot) in fungi[k][f].iter_mut().enumerate() {
                    *slot = b.id(
                        name,
                        &[
                            ("facing", facing),
                            ("position", "1"),
                            ("toggle", &(t + 1).to_string()),
                        ],
                    )?;
                }
            }
        }
        Ok(Self {
            seed,
            woods,
            fungi,
            water: b.id("minecraft:water", &[("level", "0")])?,
            amount: detail.clamp(0.0, 1.0) * 2.0,
        })
    }

    fn wood(&self, w: Wood) -> &WoodKit {
        &self
            .woods
            .iter()
            .find(|(k, _)| *k == w)
            .expect("all woods")
            .1
    }

    /// The anchor of an object cell and its hash.
    pub fn anchor(&self, gx: i32, gz: i32) -> (i32, i32, u64) {
        let h = hash2(self.seed ^ SALT, gx as i64, gz as i64);
        let span = CELL as u64 - 2;
        (
            gx * CELL + 1 + (h % span) as i32,
            gz * CELL + 1 + ((h >> 8) % span) as i32,
            h,
        )
    }

    /// Builds the object of the cell (`gx`, `gz`), if it has one. `near_tree` tells whether a
    /// trunk stands within a distance of a column.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        &self,
        surface: &Surface,
        flora: &Flora,
        gx: i32,
        gz: i32,
        grid: &ColumnGrid,
        near_tree: &dyn Fn(i32, i32, i32) -> bool,
        sink: &mut dyn BlockSink,
    ) {
        let (x, z, h) = self.anchor(gx, gz);
        let Some(top) = surface.top_at(x, z, grid) else {
            return;
        };
        let (pb, pl, ps) = chances(top.biome, top.zone);
        let u = hash_unit(hash64(h ^ 1)) / self.amount.max(1e-3);
        if u < pb {
            self.boulder(surface, flora, x, z, h, &top, grid, near_tree, sink);
        } else if u < pb + pl {
            self.log(surface, flora, x, z, h, &top, grid, near_tree, sink);
        } else if u < pb + pl + ps {
            self.stump(surface, x, z, h, &top, grid, near_tree, sink);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn boulder(
        &self,
        surface: &Surface,
        flora: &Flora,
        x: i32,
        z: i32,
        h: u64,
        top: &Top,
        grid: &ColumnGrid,
        near_tree: &dyn Fn(i32, i32, i32) -> bool,
        sink: &mut dyn BlockSink,
    ) {
        let r = |k: u64| hash_unit(hash64(h ^ k));
        // In the mountains and on rough ground boulders lie low, sunk half into the slope, so
        // they are part of it rather than lumps set on top.
        let low = matches!(
            top.biome.fallback(),
            Biome::Mountains | Biome::Highlands | Biome::SnowyPeaks | Biome::AlpineMeadow
        ) || matches!(top.zone, Zone::Stony | Zone::Scree | Zone::Rock);
        let radius = if low {
            (1.0 + 1.4 * r(2).powf(1.6)).min(2.4)
        } else {
            (1.2 + 1.9 * r(2).powf(1.6)).min(3.1)
        };
        let (rx, rz) = (radius, radius * (0.6 + 0.4 * r(3)));
        let angle = r(4) * std::f32::consts::PI;
        let (sin, cos) = angle.sin_cos();
        let height = radius
            * if low {
                0.35 + 0.3 * r(5)
            } else {
                0.5 + 0.55 * r(5)
            };
        let sunk = if low { 0.6 } else { 0.3 };
        let reach = radius.ceil() as i32 + 1;
        if near_tree(x, z, reach + 1) {
            return;
        }
        let Some(anchor) = grid.try_get(x, z) else {
            return;
        };
        // On a steep slope the downhill side would stand out as a wall.
        if anchor.slope > 0.8 {
            return;
        }
        let base = anchor.height;
        // The whole footprint must be known, so every piece builds the same boulder.
        let mut cols = Vec::new();
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                let Some(c) = grid.try_get(x + dx, z + dz) else {
                    return;
                };
                let (fx, fz) = (dx as f32, dz as f32);
                let (u, v) = (fx * cos + fz * sin, -fx * sin + fz * cos);
                let q = (u / rx).powi(2) + (v / rz).powi(2);
                if q < 1.0 {
                    cols.push((dx, dz, q, *c));
                }
            }
        }
        let kit = &surface.kit;
        let suite = top.strata.suite;
        let rock = surface.geology.boulder(suite, r(6));
        let wet_place = damp(top.biome, top, anchor.moisture);
        let mossy = if wet_place && r(7) < 0.7 {
            surface.geology.mossy_any(rock)
        } else {
            None
        };
        let lichen = flora.list(super::flora::carpets(top.biome));
        for &(dx, dz, q, c) in &cols {
            let (cx, cz) = (x + dx, z + dz);
            let bump = 0.35 * (hash_unit(hash2(h, dx as i64, dz as i64)) - 0.5);
            // Half follows the ground under it, so the downhill side doesn't stand proud.
            let under = base + 0.5 * (c.height - base);
            let crest = under - sunk + height * (1.0 - q).sqrt() + bump;
            let (gy, gl) = c.ground();
            if crest <= gy as f32 + gl as f32 / 8.0 + 0.1 {
                continue;
            }
            let full_top = crest.floor() as i32;
            let layers = ((crest - full_top as f32) * 8.0).round() as u8;
            let last = if layers >= 1 { full_top } else { full_top - 1 };
            for y in gy..=last {
                let cur = sink.get(cx, y, cz);
                if !free(surface, self.water, cur) && kit.of_block(cur).is_none() {
                    continue;
                }
                let is_top = y == last;
                let mat = match mossy {
                    Some(m) if is_top => m,
                    _ => rock,
                };
                let m = kit.mat(mat);
                let id = if y == full_top && layers < 8 {
                    if cur == self.water {
                        m.thin_wet[layers as usize - 1]
                    } else {
                        m.thin[layers as usize - 1]
                    }
                } else {
                    m.full
                };
                sink.set(cx, y, cz, id);
            }
            // Lichen on dry boulder tops, where the top is a full block.
            let full_crown = !(1..8).contains(&layers);
            if mossy.is_none() && full_crown && !lichen.is_empty() {
                let k = hash2(h ^ 0x11C, cx as i64, cz as i64);
                if hash_unit(k) < 0.3 {
                    let id = pick(&lichen, hash_unit(hash64(k)));
                    if let Some(id) = flora.get(id).id(0, false, 8, false) {
                        sink.set_if_air(cx, last + 1, cz, id);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn log(
        &self,
        surface: &Surface,
        flora: &Flora,
        x: i32,
        z: i32,
        h: u64,
        top: &Top,
        grid: &ColumnGrid,
        near_tree: &dyn Fn(i32, i32, i32) -> bool,
        sink: &mut dyn BlockSink,
    ) {
        let r = |k: u64| hash_unit(hash64(h ^ k));
        let len = 3 + (r(2) * 4.0) as i32; // 3..=6
        let Some(anchor) = grid.try_get(x, z) else {
            return;
        };
        if anchor.is_wet() || matches!(top.zone, Zone::Rock | Zone::Scree) {
            return;
        }
        let (y, _) = anchor.ground();
        // Lie along x or z, whichever way the ground is level.
        let first = (h >> 20) & 3;
        let dirs = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        let mut found = None;
        for k in 0..4 {
            let (dx, dz) = dirs[((first + k) % 4) as usize];
            let level = (0..len).all(|i| {
                grid.try_get(x + dx * i, z + dz * i)
                    .is_some_and(|c| !c.is_wet() && c.ground().0 == y)
            });
            let clear = (0..len).all(|i| !near_tree(x + dx * i, z + dz * i, 2));
            if level && clear {
                found = Some((dx, dz));
                break;
            }
        }
        let Some((dx, dz)) = found else {
            return;
        };
        let wood = pick(woods(top.biome), r(3));
        let kit = self.wood(wood);
        let axis = (dz != 0) as usize;
        let wet_place = damp(top.biome, top, anchor.moisture);
        let moss_share = if wet_place { 0.65 } else { 0.2 };
        let carpets = flora.list(super::flora::carpets(top.biome));
        // The tip points back along the log: the arm of the last block faces the anchor.
        let back = match (dx, dz) {
            (1, _) => 3,  // west
            (-1, _) => 1, // east
            (_, 1) => 0,  // north
            _ => 2,       // south
        };
        let fungus = if wood == Wood::Birch { 0 } else { 1 };
        for i in 0..len {
            let (cx, cz) = (x + dx * i, z + dz * i);
            if !free(surface, self.water, sink.get(cx, y, cz)) {
                continue;
            }
            let k = hash2(h ^ 0x106, cx as i64, cz as i64);
            let tip = i == len - 1 && len >= 4;
            let mossy = hash_unit(k) < moss_share;
            let id = if tip {
                kit.tip[back]
            } else {
                kit.log[axis][mossy as usize]
            };
            sink.set(cx, y, cz, id);
            if tip {
                continue;
            }
            // Moss on top, bracket fungi on the sides.
            if !carpets.is_empty() && hash_unit(hash64(k ^ 1)) < moss_share * 0.6 {
                let m = pick(&carpets, hash_unit(hash64(k ^ 2)));
                if let Some(id) = flora.get(m).id(0, false, 8, false) {
                    sink.set_if_air(cx, y + 1, cz, id);
                }
            }
            let fungi_share = if wet_place { 0.22 } else { 0.08 };
            for (side, (sx, sz)) in [(0, (0, -1)), (1, (1, 0)), (2, (0, 1)), (3, (-1, 0))] {
                if (sx != 0) == (dx != 0) {
                    continue; // along the log
                }
                let f = hash64(k ^ (0xF0 + side as u64));
                if hash_unit(f) < fungi_share {
                    // Facing points back at the log.
                    let facing = (side + 2) % 4;
                    let id = self.fungi[fungus][facing][(f % 3) as usize];
                    sink.set_if_air(cx + sx, y, cz + sz, id);
                }
            }
        }
        // Often the stump it broke from stands a step behind the root end.
        if r(4) < 0.45 {
            let (sx, sz) = (x - dx * 2, z - dz * 2);
            if let Some(c) = grid.try_get(sx, sz) {
                let (sy, _) = c.ground();
                if !c.is_wet()
                    && (sy - y).abs() <= 1
                    && free(surface, self.water, sink.get(sx, sy, sz))
                {
                    let mossy = r(5) < moss_share;
                    sink.set(sx, sy, sz, kit.stump[mossy as usize][1]);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn stump(
        &self,
        surface: &Surface,
        x: i32,
        z: i32,
        h: u64,
        top: &Top,
        grid: &ColumnGrid,
        near_tree: &dyn Fn(i32, i32, i32) -> bool,
        sink: &mut dyn BlockSink,
    ) {
        let r = |k: u64| hash_unit(hash64(h ^ k));
        let Some(c) = grid.try_get(x, z) else {
            return;
        };
        if c.is_wet() || matches!(top.zone, Zone::Rock | Zone::Scree) || near_tree(x, z, 1) {
            return;
        }
        let kind = surface.kit.mat(top.mat).kind;
        if matches!(kind, Kind::Rock | Kind::Snow | Kind::Ice) {
            return;
        }
        let (y, _) = c.ground();
        if !free(surface, self.water, sink.get(x, y, z)) {
            return;
        }
        let kit = self.wood(pick(woods(top.biome), r(3)));
        let mossy = damp(top.biome, top, c.moisture) && r(4) < 0.6;
        let thick = r(5) < 0.7;
        sink.set(x, y, z, kit.stump[mossy as usize][thick as usize]);
    }
}
