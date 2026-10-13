//! Trees built from Conquest Reforged trunk, branch and leaf pieces.
//!
//! `decor` decides where trees stand and passes a [`TreeRequest`]; [`Trees::build`] writes the
//! tree through a [`BlockSink`]. A build must depend only on the request (its own seed, not a
//! shared random sequence) and must stay within [`MAX_REACH`] blocks of the trunk sideways,
//! because the engine rebuilds a tree in every piece of the map it overlaps and clips it there.
//!
//! The current implementation is the phase-0 oak; other species fall back to it until they are
//! written.

use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::noise::Rng;
use crate::sink::{BlockId, BlockSink, Blocks, AIR};

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

const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// Branch thickness, as Conquest names the pieces.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Thick {
    Large,
    Medium,
    Small,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    Log,
    Pillar,
    Branch(Thick),
    Leaves,
}

/// Block ids for one wood type, with every branch connection pre-resolved.
struct Kit {
    log: BlockId,
    pillar: BlockId,
    leaves: BlockId,
    /// `[thickness][north | east << 1 | south << 2 | west << 3 | up << 4]`
    branches: [[BlockId; 32]; 3],
}

impl Kit {
    fn new(b: &Blocks, wood: &str) -> Result<Self> {
        let flag = |v: bool| if v { "true" } else { "false" };
        let mut branches = [[AIR; 32]; 3];
        for (t, piece) in ["large_branch", "branch", "small_branch"]
            .iter()
            .enumerate()
        {
            for (bits, id) in branches[t].iter_mut().enumerate() {
                *id = b.id(
                    &format!("conquest:{wood}_log_{piece}"),
                    &[
                        ("north", flag(bits & 1 != 0)),
                        ("east", flag(bits & 2 != 0)),
                        ("south", flag(bits & 4 != 0)),
                        ("west", flag(bits & 8 != 0)),
                        ("up", flag(bits & 16 != 0)),
                        ("waterlogged", "false"),
                    ],
                )?;
            }
        }
        Ok(Self {
            log: b.id(&format!("minecraft:{wood}_log"), &[("axis", "y")])?,
            // layer=3 is the thickest pillar, 12 px wide.
            pillar: b.id(
                &format!("conquest:{wood}_log_pillar"),
                &[("layer", "3"), ("down", "false"), ("waterlogged", "false")],
            )?,
            leaves: b.id(
                &format!("minecraft:{wood}_leaves"),
                &[("persistent", "true"), ("distance", "1")],
            )?,
            branches,
        })
    }
}

pub struct Trees {
    oak: Kit,
}

/// A tree being assembled before it is written, so connections can be worked out first.
struct Plan<'s> {
    sink: &'s dyn BlockSink,
    cells: HashMap<(i32, i32, i32), Piece>,
}

impl Plan<'_> {
    fn free(&self, x: i32, y: i32, z: i32) -> bool {
        !self.cells.contains_key(&(x, y, z)) && self.sink.get(x, y, z) == AIR
    }

    fn put(&mut self, x: i32, y: i32, z: i32, piece: Piece) {
        self.cells.insert((x, y, z), piece);
    }

    fn put_if_free(&mut self, x: i32, y: i32, z: i32, piece: Piece) -> bool {
        let free = self.free(x, y, z);
        if free {
            self.put(x, y, z, piece);
        }
        free
    }

    fn is_branch(&self, x: i32, y: i32, z: i32) -> bool {
        matches!(self.cells.get(&(x, y, z)), Some(Piece::Branch(_)))
    }
}

impl Trees {
    pub fn new(blocks: &Blocks) -> Result<Self> {
        Ok(Self {
            oak: Kit::new(blocks, "oak")?,
        })
    }

    /// Rough height and crown radius in blocks, for previews.
    pub fn shape(&self, req: &TreeRequest) -> (f32, f32) {
        let big = req.size >= 0.4;
        if big {
            (17.0, 7.0)
        } else {
            (11.0, 4.5)
        }
    }

    pub fn build(&self, req: &TreeRequest, sink: &mut dyn BlockSink) {
        let kit = &self.oak;
        let mut plan = Plan {
            sink: &*sink,
            cells: HashMap::new(),
        };
        oak(&mut plan, req);

        let mut writes = Vec::with_capacity(plan.cells.len());
        for (&(x, y, z), &piece) in &plan.cells {
            let id = match piece {
                Piece::Log => kit.log,
                Piece::Pillar => kit.pillar,
                Piece::Leaves => kit.leaves,
                Piece::Branch(t) => {
                    // As Conquest's own update rule: arms towards neighbouring branches, a post
                    // when the block below is solid or a branch. A pillar's top face isn't
                    // sturdy, so the game itself would drop the post on the next block update;
                    // keep it so the trunk reads as one piece.
                    let up = plan.cells.contains_key(&(x, y - 1, z))
                        || plan.sink.get(x, y - 1, z) != AIR;
                    let bits = plan.is_branch(x, y, z - 1) as usize
                        | (plan.is_branch(x + 1, y, z) as usize) << 1
                        | (plan.is_branch(x, y, z + 1) as usize) << 2
                        | (plan.is_branch(x - 1, y, z) as usize) << 3
                        | (up as usize) << 4;
                    kit.branches[t as usize][bits]
                }
            };
            writes.push((x, y, z, id));
        }
        for (x, y, z, id) in writes {
            sink.set(x, y, z, id);
        }
    }
}

/// An oak built the way Conquest builders do it: a tapering trunk (full logs, a 12 px pillar,
/// then large-branch posts), limbs that run out and climb while thinning from large to small
/// branches and fork at the ends, and loose leaf clumps on the twig ends, so the branch
/// structure stays visible under and between them.
fn oak(plan: &mut Plan, req: &TreeRequest) {
    let mut rng = Rng::new(req.seed);
    let (x, base, z) = (req.x, req.y, req.z);
    let big = req.size >= 0.4;
    let logs = if big { rng.range(3, 4) } else { 2 };
    let pillars = rng.range(1, 2);
    let posts = if big { rng.range(4, 5) } else { 3 };
    for y in base..base + logs {
        plan.put(x, y, z, Piece::Log);
    }
    for y in base + logs..base + logs + pillars {
        plan.put(x, y, z, Piece::Pillar);
    }
    let crown = base + logs + pillars;
    for y in crown..crown + posts {
        plan.put(x, y, z, Piece::Branch(Thick::Large));
    }

    // One limb per side, at different heights so limbs never stack into walls.
    let mut tips = Vec::new();
    let sides = if big { 4 } else { 3 };
    let start = rng.range(0, 3) as usize;
    for i in 0..sides {
        let y = crown + i as i32 * (posts - 1) / sides as i32;
        limb(plan, &mut rng, &mut tips, (x, y, z), (start + i) % 4, big);
    }

    // The leader carries on up, splits into two twigs and gets the top clump.
    let lead = rng.range(1, 2);
    for k in 0..lead {
        plan.put(x, crown + posts + k, z, Piece::Branch(Thick::Medium));
    }
    let ty = crown + posts + lead - 1;
    tips.push((x, ty + 1, z));
    for k in 0..2 {
        let (dx, dz) = DIRS[(start + 1 + 2 * k) % 4];
        if plan.put_if_free(x + dx, ty, z + dz, Piece::Branch(Thick::Small)) {
            tips.push((x + dx, ty, z + dz));
        }
    }

    // Loose, flattened clumps above the twig ends; the space under them stays open.
    let rx: f64 = if big { 2.4 } else { 1.9 };
    for (tx, ty, tz) in tips {
        let r = rx as i32 + 1;
        for dx in -r..=r {
            for dz in -r..=r {
                for dy in -1..=2 {
                    let d = (dx * dx + dz * dz) as f64 / (rx * rx)
                        + (dy as f64 - 0.4).powi(2) / (1.5 * 1.5);
                    if d > 1.0 || (d > 0.45 && rng.unit() < 0.3) {
                        continue;
                    }
                    plan.put_if_free(tx + dx, ty + dy, tz + dz, Piece::Leaves);
                }
            }
        }
    }
}

/// One limb: a thick stretch out of the trunk, a climb, a thinner stretch further out (sometimes
/// with a sideways jog), then a fork into two small twigs whose ends get leaf clumps.
fn limb(
    plan: &mut Plan,
    rng: &mut Rng,
    tips: &mut Vec<(i32, i32, i32)>,
    from: (i32, i32, i32),
    dir: usize,
    big: bool,
) {
    let (mut cx, mut cy, mut cz) = from;
    let (dx, dz) = DIRS[dir];
    let side = DIRS[(dir + if rng.unit() < 0.5 { 1 } else { 3 }) % 4];
    let (inner, outer) = if big {
        (rng.range(2, 3), rng.range(2, 3))
    } else {
        (rng.range(1, 2), rng.range(1, 2))
    };

    for _ in 0..inner {
        cx += dx;
        cz += dz;
        plan.put_if_free(cx, cy, cz, Piece::Branch(Thick::Large));
    }
    for _ in 0..rng.range(1, 2) {
        cy += 1;
        plan.put_if_free(cx, cy, cz, Piece::Branch(Thick::Medium));
    }
    for k in 0..outer {
        let (sx, sz) = if k == 1 && rng.unit() < 0.4 {
            side
        } else {
            (dx, dz)
        };
        cx += sx;
        cz += sz;
        plan.put_if_free(cx, cy, cz, Piece::Branch(Thick::Medium));
    }

    // Fork: one twig carries on outwards, the other goes to the side.
    let (ox, oz) = (cx + dx, cz + dz);
    if plan.put_if_free(ox, cy, oz, Piece::Branch(Thick::Small)) {
        tips.push((ox, cy, oz));
    }
    let (fx, fz) = (cx - side.0, cz - side.1);
    if plan.put_if_free(fx, cy, fz, Piece::Branch(Thick::Small)) {
        if rng.unit() < 0.5
            && plan.put_if_free(fx - side.0, cy, fz - side.1, Piece::Branch(Thick::Small))
        {
            tips.push((fx - side.0, cy, fz - side.1));
        } else {
            tips.push((fx, cy, fz));
        }
    }
}
