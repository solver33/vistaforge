//! Waterfalls: streaks of white water down the falling curtain, and steam and mist hanging over
//! the plunge pool at its foot.
//!
//! Each column writes only its own cells, white water into falling water and mist into empty
//! air, and decides from the grid alone, so it comes out the same in every piece.

use crate::column::ColumnGrid;
use crate::noise::{hash2, hash64};
use crate::sink::{BlockId, BlockSink, Blocks, AIR};
use crate::surface::field::{dither, hash_unit, unit};
use crate::surface::nearest_fall;

/// How far mist drifts from a curtain, in blocks.
pub(crate) const MIST_REACH: i32 = 6;

mod salt {
    pub const STREAK: u64 = 0xFA11;
    pub const FOAM: u64 = 0xFA12;
    pub const MIST: u64 = 0xFA13;
    pub const PUFF: u64 = 0xFA14;
}

/// `conquest:white_cloud` opacities for mist, faintest first.
const MIST_OPACITY: [&str; 4] = ["1", "2", "3", "4"];

pub(crate) struct Falls {
    seed: u64,
    /// The detail slider as a factor (1 at the default).
    detail: f32,
    falling: BlockId,
    /// Waterlogged white-water panes standing across a curtain that flows along x, and along z.
    foam: Option<[BlockId; 2]>,
    steam: Option<BlockId>,
    mist: Vec<BlockId>,
}

impl Falls {
    pub fn new(b: &Blocks, seed: u64, detail: f32) -> anyhow::Result<Self> {
        let pane = |along_x: bool| {
            let (ns, ew) = if along_x {
                ("true", "false")
            } else {
                ("false", "true")
            };
            b.id(
                "conquest:waterfall_pane",
                &[
                    ("north", ns),
                    ("south", ns),
                    ("east", ew),
                    ("west", ew),
                    ("up", "true"),
                    ("down", "true"),
                    ("waterlogged", "true"),
                ],
            )
        };
        Ok(Self {
            seed,
            detail,
            falling: b.id("minecraft:water", &[("level", "8")])?,
            foam: pane(true).and_then(|x| Ok([x, pane(false)?])).ok(),
            steam: b.default_id("conquest:steam").ok(),
            mist: MIST_OPACITY
                .iter()
                .filter_map(|o| b.id("conquest:white_cloud", &[("opacity", o)]).ok())
                .collect(),
        })
    }

    fn cell(&self, salt: u64, x: i32, y: i32, z: i32) -> f32 {
        hash_unit(hash64(
            hash2(self.seed ^ salt, x as i64, z as i64) ^ (y as u64),
        ))
    }

    /// White water down a curtain column: whole streaks of it, and churning at the foot.
    pub fn foam(&self, x: i32, z: i32, grid: &ColumnGrid, sink: &mut dyn BlockSink) {
        let Some(foam) = self.foam else {
            return;
        };
        let Some(col) = grid.try_get(x, z) else {
            return;
        };
        let (Some(w), Some(f)) = (col.water_level, col.fall) else {
            return;
        };
        // Which way the water runs: from the water above the lip towards the pool below. The
        // white water stands across it. Looking two blocks around keeps the curtain columns
        // beside each other turned the same way.
        let (mut fx, mut fz) = (0, 0);
        for dx in -2..=2 {
            for dz in -2..=2 {
                let Some(c) = grid.try_get(x + dx, z + dz) else {
                    continue;
                };
                let toward = match c.water_level {
                    Some(cw) if cw >= f => -1,
                    Some(cw) if cw == w && c.fall.is_none() => 1,
                    _ => continue,
                };
                fx += toward * dx;
                fz += toward * dz;
            }
        }
        let pane = foam[(fz.abs() > fx.abs()) as usize];
        let streak = unit(self.seed, salt::STREAK, x, z);
        for y in w + 1..=f {
            let p = if y <= w + 2 {
                0.85
            } else {
                0.25 + 0.55 * streak
            };
            if sink.get(x, y, z) == self.falling && self.cell(salt::FOAM, x, y, z) < p {
                sink.set(x, y, z, pane);
            }
        }
    }

    /// Steam and mist over the water and banks near the foot of a waterfall: thicker, wider and
    /// taller under a higher drop, thinning out away from it.
    pub fn mist(&self, x: i32, z: i32, grid: &ColumnGrid, sink: &mut dyn BlockSink) {
        if self.mist.is_empty() && self.steam.is_none() {
            return;
        }
        let Some(fall) = nearest_fall(x, z, grid, MIST_REACH) else {
            return;
        };
        let size = (2.5 + fall.drop as f32 / 3.0).min(MIST_REACH as f32);
        let edge = size + dither(self.seed, salt::MIST, x, z, 1.2);
        if fall.dist > edge {
            return;
        }
        let near = 1.0 - fall.dist / edge.max(0.5);
        let thick = 1 + ((fall.drop.min(20) as f32 / 4.0) * near).round() as i32;
        let bottom = fall.pool + 1;
        for y in bottom..bottom + thick {
            if sink.get(x, y, z) != AIR {
                continue;
            }
            let lift = (y - bottom) as f32 / thick as f32;
            let p = (0.3 + 0.6 * near) * (1.0 - 0.4 * lift) * self.detail;
            let u = self.cell(salt::PUFF, x, y, z);
            if u >= p {
                continue;
            }
            // Steam rolls on the water; fainter mist hangs above it.
            let block = match self.steam {
                Some(steam) if lift < 0.5 && (u < 0.7 * p || self.mist.is_empty()) => steam,
                _ if !self.mist.is_empty() => {
                    let dense = (near * (1.0 - lift) * self.mist.len() as f32) as usize;
                    self.mist[dense.min(self.mist.len() - 1)]
                }
                _ => continue,
            };
            sink.set(x, y, z, block);
        }
    }
}
