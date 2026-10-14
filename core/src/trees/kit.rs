//! Block ids for the wood types and leaves trees are built from, resolved once against the
//! user's catalog.

use std::collections::HashSet;

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
    Palm = 10,
    Acacia = 12,
}

/// Catalog names of each wood type, in index order: the plain wood, then its mossy variant.
const WOODS: [&str; 14] = [
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
    "palm",
    "mossy_palm",
    "acacia",
    "mossy_acacia",
];

/// Woods whose Conquest pieces have no full log of their own: the log and bark blocks their
/// pieces are textured after.
const STAND_INS: [(&str, &str, &str); 1] =
    [("palm", "minecraft:jungle_log", "minecraft:jungle_wood")];

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
    /// The heart of a palm crown (`date_palm_leaves`, without dates).
    Date,
    Jungle,
    /// A green palm frond pointing north; the next three point east, south and west (see
    /// [`SIDES`]).
    Frond,
    /// A green palm frond pointing between north and east; the next three turn on clockwise.
    FrondCorner = Leaf::Frond as isize + 4,
    /// Dry, hanging fronds, laid out like the green ones.
    OldFrond = Leaf::Frond as isize + 8,
    OldFrondCorner = Leaf::Frond as isize + 12,
}

const LEAVES: [(&str, &[(&str, &str)]); 11] = [
    ("minecraft:oak_leaves", &[]),
    ("minecraft:birch_leaves", &[]),
    ("minecraft:spruce_leaves", &[]),
    ("conquest:pine_needles", &[]),
    ("conquest:weeping_willow_leaves", &[("toggle", "1")]),
    ("conquest:weeping_willow_leaves", &[("toggle", "2")]),
    ("conquest:dead_deciduous_branches", &[]),
    ("conquest:hawthorn_bush", &[]),
    ("conquest:lilac", &[]),
    ("conquest:date_palm_leaves", &[("age", "0")]),
    ("minecraft:jungle_leaves", &[]),
];

/// Palm fronds, after the plain leaves: each in four directions.
const FRONDS: [&str; 4] = [
    "conquest:caribbean_royal_palm_leaves",
    "conquest:caribbean_royal_palm_leaves_corner",
    "conquest:old_caribbean_royal_palm_leaves",
    "conquest:old_caribbean_royal_palm_leaves_corner",
];

/// Conquest's `facing` for a frond pointing each way (see [`SIDES`]); for a corner frond, the
/// first of the two sides it points between. Unconfirmed in game: if fronds point back at
/// the trunk, turn this round.
const FROND_FACING: [&str; 4] = ["north", "east", "south", "west"];

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
    /// `[layer - 1][side][down][waterlogged]`; `down` is Conquest's `extension_toggle`.
    slab: [[[[BlockId; 2]; 2]; 4]; 4],
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
        let stand_in = STAND_INS.iter().find(|s| s.0 == wood);
        let log = match format!("minecraft:{wood}_log") {
            vanilla if b.has(&vanilla) => vanilla,
            _ => match stand_in {
                Some(s) => s.1.to_string(),
                None => format!("conquest:{wood}_log"),
            },
        };
        let bark = [
            format!("conquest:{wood}_log_bark"),
            format!("minecraft:{wood}_wood"),
        ]
        .into_iter()
        .chain(stand_in.map(|s| s.2.to_string()))
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
            slab: [[[[AIR; 2]; 2]; 4]; 4],
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
                    for down in 0..2 {
                        kit.slab[l][s][down][wl] = b.id(
                            &piece("vertical_slab"),
                            &[
                                ("layer", layer.as_str()),
                                ("facing", facing),
                                ("extension_toggle", flag(down == 1)),
                                ("waterlogged", water),
                            ],
                        )?;
                    }
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
            Piece::Empty | Piece::Leaves(_) | Piece::Snow(_) => AIR,
            Piece::Log => self.log,
            Piece::Wood => self.bark,
            Piece::Pillar(l) if stump => self.stump[l as usize - 1][d][w],
            Piece::Pillar(l) => self.pillar[l as usize - 1][d][w],
            Piece::Quarter(l, c) if stump => self.quarter_stump[l as usize - 1][c as usize][d][w],
            Piece::Quarter(l, c) => self.quarter[l as usize - 1][c as usize][d][w],
            Piece::Slab(l, s) => self.slab[l as usize - 1][s as usize][d][w],
            Piece::Branch(t) => self.branch[t.index()][arms][w],
        }
    }

    /// Every block of the kit.
    fn all(&self) -> Vec<BlockId> {
        let mut out = vec![self.log, self.bark];
        for t in [&self.pillar, &self.stump] {
            out.extend(t.iter().flatten().flatten());
        }
        for t in [&self.quarter, &self.quarter_stump] {
            out.extend(t.iter().flatten().flatten().flatten());
        }
        out.extend(self.slab.iter().flatten().flatten().flatten());
        out.extend(self.branch.iter().flatten().flatten());
        out
    }
}

/// Every block trees are made of.
pub(super) struct Kits {
    pub woods: Vec<Kit>,
    pub leaves: Vec<BlockId>,
    /// Snow lying on a tree, by [`Piece::Snow`] layers: Conquest snow slabs, so they never
    /// pass for the snow on the ground.
    pub snow: [BlockId; 2],
    pub water: BlockId,
    /// All of the above but water: what another tree may have left in the world.
    tree: HashSet<BlockId>,
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
        for name in FRONDS {
            for facing in FROND_FACING {
                leaves.push(b.id(name, &[("facing", facing)])?);
            }
        }
        let snow_slab = |layers| {
            b.id(
                "conquest:snow_slab",
                &[
                    ("layers", layers),
                    ("type", "bottom"),
                    ("waterlogged", "false"),
                ],
            )
        };
        let snow = [snow_slab("1")?, snow_slab("2")?];
        let tree = woods
            .iter()
            .flat_map(|k| k.all())
            .chain(leaves.iter().copied())
            .chain(snow)
            .collect();
        Ok(Self {
            woods,
            leaves,
            snow,
            water: b.id("minecraft:water", &[("level", "0")])?,
            tree,
        })
    }

    /// A block some tree is built of.
    pub fn is_tree(&self, id: BlockId) -> bool {
        self.tree.contains(&id)
    }

    pub fn is_leaves(&self, id: BlockId) -> bool {
        self.leaves.contains(&id)
    }

    /// Snow some tree left on its leaves.
    pub fn is_snow(&self, id: BlockId) -> bool {
        self.snow.contains(&id)
    }
}

const _: () = assert!(
    SIDES.len() == 4
        && FROND_FACING.len() == SIDES.len()
        && LEAVES.len() == Leaf::Frond as usize
        && Leaf::OldFrondCorner as usize + 4 == LEAVES.len() + 4 * FRONDS.len()
);
