//! Block ids for the wood types and leaves trees are built from, resolved once against the
//! user's catalog.

use anyhow::Result;

use super::plan::{Piece, SIDES};
use crate::sink::{BlockId, Blocks, AIR};

/// Wood types. A plan cell stores the index; the mossy variant of each sits right after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Wood {
    Oak = 0,
    Birch = 2,
    Spruce = 4,
    Pine = 6,
    Willow = 8,
}

/// Catalog names of each wood type, in index order: the plain wood, then its mossy variant.
const WOODS: [&str; 10] = [
    "oak",
    "mossy_oak",
    "birch",
    "mossy_birch",
    "norway_spruce",
    "mossy_norway_spruce",
    "pine",
    "mossy_pine",
    "willow",
    "mossy_willow",
];

/// Leaf blocks; [`Piece::Leaves`] holds the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Leaf {
    Oak = 0,
    Birch,
    Spruce,
    Pine,
    Willow,
    /// The second look of the weeping willow leaves (`toggle=2`).
    Willow2,
    /// Bare twigs (`dead_deciduous_branches`).
    Twigs,
    Hawthorn,
    Lilac,
}

const LEAVES: [(&str, &[(&str, &str)]); 9] = [
    ("minecraft:oak_leaves", &[]),
    ("minecraft:birch_leaves", &[]),
    ("minecraft:spruce_leaves", &[]),
    ("conquest:pine_needles", &[]),
    ("conquest:weeping_willow_leaves", &[("toggle", "1")]),
    ("conquest:weeping_willow_leaves", &[("toggle", "2")]),
    ("conquest:dead_deciduous_branches", &[]),
    ("conquest:hawthorn_bush", &[]),
    ("conquest:lilac", &[]),
];

fn flag(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// Every piece of one wood type, with each state the writer can need.
pub(super) struct Kit {
    log: BlockId,
    bark: BlockId,
    /// `[layer - 1][down][waterlogged]`
    pillar: [[[BlockId; 2]; 2]; 5],
    stump: [[[BlockId; 2]; 2]; 5],
    /// `[layer - 1][corner][down][waterlogged]`
    quarter: [[[[BlockId; 2]; 2]; 4]; 5],
    quarter_stump: [[[[BlockId; 2]; 2]; 4]; 5],
    /// `[layer - 1][side][waterlogged]`
    slab: [[[BlockId; 2]; 4]; 4],
    /// `[thickness][north | east << 1 | south << 2 | west << 3 | up << 4][waterlogged]`
    branch: [[[BlockId; 2]; 32]; 3],
}

/// Conquest's `facing` for a vertical quarter in each corner (see `plan::CORNERS`): a quarter
/// facing north fills the south-east corner.
const QUARTER_FACING: [&str; 4] = ["south", "west", "north", "east"];
/// Conquest's `facing` for a vertical slab against each side (see `plan::SIDES`): a slab
/// facing north lies against the south side.
const SLAB_FACING: [&str; 4] = ["south", "west", "north", "east"];

impl Kit {
    fn new(b: &Blocks, wood: &str) -> Result<Self> {
        let log = match format!("minecraft:{wood}_log") {
            vanilla if b.has(&vanilla) => vanilla,
            _ => format!("conquest:{wood}_log"),
        };
        let bark = [
            format!("conquest:{wood}_log_bark"),
            format!("minecraft:{wood}_wood"),
        ]
        .into_iter()
        .find(|n| b.has(n))
        .unwrap_or_else(|| log.clone());
        let piece = |kind: &str| format!("conquest:{wood}_log_{kind}");

        let mut kit = Kit {
            log: b.id(&log, &[("axis", "y")])?,
            bark: b.id(&bark, &[("axis", "y")])?,
            pillar: [[[AIR; 2]; 2]; 5],
            stump: [[[AIR; 2]; 2]; 5],
            quarter: [[[[AIR; 2]; 2]; 4]; 5],
            quarter_stump: [[[[AIR; 2]; 2]; 4]; 5],
            slab: [[[AIR; 2]; 4]; 4],
            branch: [[[AIR; 2]; 32]; 3],
        };
        for wl in 0..2 {
            let water = flag(wl == 1);
            for l in 0..5 {
                let layer = (l + 1).to_string();
                for down in 0..2 {
                    let props = [
                        ("layer", layer.as_str()),
                        ("down", flag(down == 1)),
                        ("waterlogged", water),
                    ];
                    kit.pillar[l][down][wl] = b.id(&piece("pillar"), &props)?;
                    kit.stump[l][down][wl] = b.id(&piece("stump"), &props)?;
                    for (c, facing) in QUARTER_FACING.iter().enumerate() {
                        let props = [
                            ("layer", layer.as_str()),
                            ("facing", facing),
                            ("down", flag(down == 1)),
                            ("waterlogged", water),
                        ];
                        kit.quarter[l][c][down][wl] = b.id(&piece("vertical_quarter"), &props)?;
                        kit.quarter_stump[l][c][down][wl] =
                            b.id(&piece("vertical_quarter_stump"), &props)?;
                    }
                }
            }
            for l in 0..4 {
                let layer = (l + 1).to_string();
                for (s, facing) in SLAB_FACING.iter().enumerate() {
                    kit.slab[l][s][wl] = b.id(
                        &piece("vertical_slab"),
                        &[
                            ("layer", layer.as_str()),
                            ("facing", facing),
                            ("extension_toggle", "false"),
                            ("waterlogged", water),
                        ],
                    )?;
                }
            }
            for (t, kind) in ["large_branch", "branch", "small_branch"]
                .iter()
                .enumerate()
            {
                for bits in 0..32 {
                    let mut props = vec![("up", flag(bits & 16 != 0)), ("waterlogged", water)];
                    for (s, dir) in ["north", "east", "south", "west"].iter().enumerate() {
                        props.push((dir, flag(bits & (1 << s) != 0)));
                    }
                    kit.branch[t][bits][wl] = b.id(&piece(kind), &props)?;
                }
            }
        }
        Ok(kit)
    }

    /// The block for a piece. `arms` (bits as in `branch`) only matters for branches.
    pub(super) fn id(
        &self,
        piece: Piece,
        stump: bool,
        down: bool,
        wet: bool,
        arms: usize,
    ) -> BlockId {
        let (d, w) = (down as usize, wet as usize);
        match piece {
            Piece::Empty | Piece::Leaves(_) => AIR,
            Piece::Log => self.log,
            Piece::Wood => self.bark,
            Piece::Pillar(l) if stump => self.stump[l as usize - 1][d][w],
            Piece::Pillar(l) => self.pillar[l as usize - 1][d][w],
            Piece::Quarter(l, c) if stump => self.quarter_stump[l as usize - 1][c as usize][d][w],
            Piece::Quarter(l, c) => self.quarter[l as usize - 1][c as usize][d][w],
            Piece::Slab(l, s) => self.slab[l as usize - 1][s as usize][w],
            Piece::Branch(t) => self.branch[t.index()][arms][w],
        }
    }
}

/// Every block trees are made of.
pub(super) struct Kits {
    pub woods: Vec<Kit>,
    pub leaves: Vec<BlockId>,
    pub water: BlockId,
}

impl Kits {
    pub fn new(b: &Blocks) -> Result<Self> {
        let woods = WOODS
            .iter()
            .map(|w| Kit::new(b, w))
            .collect::<Result<Vec<_>>>()?;
        let mut leaves = Vec::new();
        for (name, extra) in LEAVES {
            let mut props = vec![("persistent", "true"), ("distance", "1")];
            props.extend_from_slice(extra);
            leaves.push(b.id(name, &props)?);
        }
        Ok(Self {
            woods,
            leaves,
            water: b.id("minecraft:water", &[("level", "0")])?,
        })
    }
}

const _: () = assert!(SIDES.len() == 4 && LEAVES.len() == Leaf::Lilac as usize + 1);
