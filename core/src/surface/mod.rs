//! Surface: the blocks of each column — bedrock, rock strata, soil, the top block and the thin
//! Conquest layer that follows the terrain's fractional height, and water — plus the Minecraft
//! biome written for the column (grass, foliage and water colours) and its colour in the app's
//! preview.
//!
//! Each column falls into a zone (open ground, under trees, damp, stony slope, scree, bare rock,
//! shore, or a water bed), and each biome mixes a few Conquest materials per zone in irregular
//! patches (see `recipes`). Below the top come soil and then rock in wavy strata, so cliffs and
//! banks show layered rock (see `geology`).
//!
//! Waterfalls: falling water fills the curtain above a column's water up to its `fall`, the
//! plunge pool below is scoured to dark mossy stones, and the spray keeps the ground beside it
//! wet and mossy.
//!
//! [`Surface::fill`] depends only on the seed, the column position and the columns within
//! [`NEAR`] blocks of it, so overlapping pieces of the map agree.

pub(crate) mod field;
pub(crate) mod geology;
pub(crate) mod materials;
pub(crate) mod recipes;

use anyhow::Result;

use crate::biome::Biome;
use crate::column::{ColumnGrid, ColumnInfo, WaterKind};
use crate::noise::{hash64, value2};
use crate::settings::WorldSettings;
use crate::sink::{BlockId, BlockSink, Blocks, AIR};
use crate::world::MIN_Y;

use field::{dither, hash_unit, local_biome, patch, pick, smoothstep, woodland};

fn wetland(b: Biome) -> bool {
    matches!(b, Biome::Swamp | Biome::Marsh)
}
use geology::{Geology, Strata};
use materials::{Kind, Kit, Mat};
use recipes::{recipe, Mix};

/// Stands in a mix for the local geology's loose rock.
const DEBRIS: Mat = Mat(u16::MAX);

struct RMix {
    base: Vec<(Mat, u32)>,
    scale: f64,
    accent: Vec<(Mat, u32)>,
    accent_scale: f64,
    accent_share: f32,
}

struct RRecipe {
    open: RMix,
    wood: RMix,
    wet: RMix,
    stony: RMix,
    scree: RMix,
    shore: RMix,
    beds: [RMix; 3],
    topsoil: Mat,
    subsoil: Mat,
    soil_depth: f32,
    stony_slope: f32,
    scree_slope: f32,
    rock_slope: (f32, f32),
    snow: bool,
    mossy_rock: bool,
}

/// Where a column is, for choosing its ground (and, in `decor`, what grows there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zone {
    Open,
    Wood,
    Wet,
    Stony,
    Scree,
    Rock,
    Shore,
    /// Under water: 0 shallow, 1 medium, 2 deep.
    Bed(u8),
}

/// What a column's top is made of.
#[derive(Clone, Copy)]
pub(crate) struct Top {
    pub zone: Zone,
    pub biome: Biome,
    pub mat: Mat,
    pub strata: Strata,
    /// A low river bank: earth down to the water rather than exposed rock.
    pub bank: bool,
}

/// How far a dry column looks for water, in blocks: shores and river banks lie within it.
pub(crate) const NEAR: i32 = 5;

/// The nearest water to a dry column, within [`NEAR`] blocks.
#[derive(Clone, Copy)]
struct Near {
    level: i32,
    /// Distance to it, in blocks.
    dist: f32,
    river: bool,
}

/// The nearest waterfall curtain to a column.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fall {
    /// Distance to it, in blocks.
    pub dist: f32,
    /// Height of the drop, in blocks.
    pub drop: i32,
    /// Water level of the plunge pool at its foot.
    pub pool: i32,
}

/// The nearest waterfall curtain within `radius` blocks of a column, if any.
pub(crate) fn nearest_fall(x: i32, z: i32, grid: &ColumnGrid, radius: i32) -> Option<Fall> {
    let mut best: Option<(i32, Fall)> = None;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let d = dx * dx + dz * dz;
            if d > radius * radius || best.is_some_and(|(bd, _)| d >= bd) {
                continue;
            }
            let Some(c) = grid.try_get(x + dx, z + dz) else {
                continue;
            };
            if let (Some(w), Some(f)) = (c.water_level, c.fall) {
                let fall = Fall {
                    dist: (d as f32).sqrt(),
                    drop: f - w,
                    pool: w,
                };
                best = Some((d, fall));
            }
        }
    }
    best.map(|(_, f)| f)
}

/// A dry column within reach of a river and less than four blocks above it.
fn low_bank(col: &ColumnInfo, near: Option<Near>) -> bool {
    near.is_some_and(|n| n.river && col.height < n.level as f32 + 4.0)
}

pub struct Surface {
    seed: u64,
    pub(crate) kit: Kit,
    pub(crate) geology: Geology,
    recipes: Vec<RRecipe>,
    river_beds: [RMix; 3],
    swamp_beds: [RMix; 3],
    fall_bed: RMix,
    fall_bank: RMix,
    snow: Mat,
    bedrock: BlockId,
    deep: BlockId,
    water: BlockId,
    falling: BlockId,
    /// Scales the share of accent patches (the "detail" slider).
    accent: f32,
}

impl Surface {
    pub fn new(b: &Blocks, settings: &WorldSettings) -> Result<Self> {
        let kit = Kit::new(b)?;
        let geology = Geology::new(settings.seed, &kit);
        let rmix = |m: &Mix| {
            let res = |list: &[(&str, u32)]| {
                list.iter()
                    .map(|&(k, w)| (if k == "@debris" { DEBRIS } else { kit.get(k) }, w))
                    .collect::<Vec<_>>()
            };
            RMix {
                base: res(m.base),
                scale: m.scale,
                accent: res(m.accent),
                accent_scale: m.accent_scale,
                accent_share: m.accent_share,
            }
        };
        let beds = |b: &[Mix; 3]| [rmix(&b[0]), rmix(&b[1]), rmix(&b[2])];
        let recipes = Biome::ALL
            .iter()
            .map(|&biome| {
                let r = recipe(biome);
                RRecipe {
                    open: rmix(&r.open),
                    wood: rmix(&r.wood),
                    wet: rmix(&r.wet),
                    stony: rmix(&r.stony),
                    scree: rmix(&r.scree),
                    shore: rmix(&r.shore),
                    beds: beds(&r.beds),
                    topsoil: kit.get(r.topsoil),
                    subsoil: kit.get(r.subsoil),
                    soil_depth: r.soil_depth,
                    stony_slope: r.stony_slope,
                    scree_slope: r.scree_slope,
                    rock_slope: r.rock_slope,
                    snow: r.snow,
                    mossy_rock: r.mossy_rock,
                }
            })
            .collect();
        Ok(Self {
            seed: settings.seed,
            river_beds: beds(recipes::river_beds()),
            swamp_beds: beds(recipes::swamp_beds()),
            fall_bed: rmix(&recipes::FALL_BED),
            fall_bank: rmix(&recipes::FALL_BANK),
            recipes,
            geology,
            snow: kit.get("snow"),
            bedrock: b.default_id("minecraft:bedrock")?,
            deep: b.default_id("minecraft:stone")?,
            water: b.id("minecraft:water", &[("level", "0")])?,
            falling: b.id("minecraft:water", &[("level", "8")])?,
            accent: (settings.sliders.detail.clamp(0.0, 1.0) * 2.0).max(0.0),
            kit,
        })
    }

    fn recipe(&self, biome: Biome) -> &RRecipe {
        &self.recipes[biome as usize]
    }

    /// Which zone a column is in. `near` is the closest water, when it is known.
    fn zone(&self, x: i32, z: i32, col: &ColumnInfo, biome: Biome, near: Option<Near>) -> Zone {
        let s = self.seed;
        if col.water_level.is_some() {
            let d = col.depth() + dither(s, 11, x, z, 0.5);
            return Zone::Bed(if d < 1.2 {
                0
            } else if d < 3.5 {
                1
            } else {
                2
            });
        }
        let r = self.recipe(biome);
        let slope = col.slope;
        // Low river banks are cut into earth: they take a steeper slope before turning to bare
        // rock or scree.
        let steeper = if low_bank(col, near) { 0.5 } else { 0.0 };
        let rock = (col.rock - 0.7 * steeper).max(smoothstep(
            r.rock_slope.0 + steeper,
            r.rock_slope.1 + steeper,
            slope,
        ));
        if rock + dither(s, 12, x, z, 0.3) > 0.5 {
            return Zone::Rock;
        }
        // The terrain marks river banks with strong flow. Here they stay soft, damp ground that
        // runs into the land around; only their slope makes them stony.
        let flow = if near.is_some() || col.biome2 == Biome::River {
            0.0
        } else {
            col.flow
        };
        if slope + dither(s, 13, x, z, 0.25) > r.scree_slope + steeper
            || flow + dither(s, 14, x, z, 0.15) > 0.65
        {
            return Zone::Scree;
        }
        if let Some(n) = near {
            // A ragged strip: wider in places, and gone where the land meets the water.
            let reach = 2.0 + dither(s, 21, x, z, 1.3);
            if n.dist <= reach && col.height < n.level as f32 + 2.2 + dither(s, 15, x, z, 0.6) {
                return Zone::Shore;
            }
        }
        if slope + dither(s, 16, x, z, 0.25) > r.stony_slope
            || flow + dither(s, 17, x, z, 0.15) > 0.4
        {
            return Zone::Stony;
        }
        if col.moisture + dither(s, 18, x, z, 0.12) > 0.74 {
            return Zone::Wet;
        }
        if woodland(s, x, z, biome) + dither(s, 19, x, z, 0.25) > 0.5 {
            return Zone::Wood;
        }
        Zone::Open
    }

    fn choose(&self, mix: &RMix, salt: u64, x: i32, z: i32, suite: usize) -> Mat {
        let s = self.seed ^ salt.wrapping_mul(0x9E37_79B9);
        let accent = (!mix.accent.is_empty())
            .then(|| patch(s, 1, x, z, mix.accent_scale))
            .filter(|&h| hash_unit(h) < mix.accent_share * self.accent);
        let m = match accent {
            Some(h) => pick(&mix.accent, hash_unit(hash64(h))),
            None => pick(&mix.base, hash_unit(patch(s, 2, x, z, mix.scale))),
        };
        if m == DEBRIS {
            self.geology.debris(suite)
        } else {
            m
        }
    }

    /// The biome whose ground a column shows: its local biome, except that rivers and their banks
    /// in swamps and marshes keep the wetland's mud, peat and moss.
    fn ground_biome(&self, x: i32, z: i32, col: &ColumnInfo) -> Biome {
        let biome = local_biome(self.seed, x, z, col);
        let land = if col.biome == Biome::River {
            col.biome2
        } else {
            col.biome
        };
        if biome == Biome::River && wetland(land) {
            land
        } else {
            biome
        }
    }

    fn top(&self, x: i32, z: i32, col: &ColumnInfo, near: Option<Near>, fall: Option<Fall>) -> Top {
        let biome = self.ground_biome(x, z, col);
        let zone = self.zone(x, z, col, biome, near);
        // A waterfall's spray reaches further from a higher drop, and up to a little above its
        // lip: the pool under it is scoured to stone and the ground beside it stays wet.
        let spray = fall.is_some_and(|f| {
            let reach = 1.5 + f.drop.min(14) as f32 / 4.0 + dither(self.seed, 22, x, z, 1.0);
            f.dist <= reach && col.height < (f.pool + f.drop + 3) as f32
        });
        let zone = match zone {
            Zone::Open | Zone::Wet | Zone::Stony | Zone::Scree | Zone::Shore if spray => Zone::Wet,
            z => z,
        };
        let strata = self.geology.strata(x, z);
        let r = self.recipe(biome);
        let mat = match zone {
            Zone::Open => self.choose(&r.open, 1, x, z, strata.suite),
            Zone::Wood => self.choose(&r.wood, 2, x, z, strata.suite),
            Zone::Wet if spray => self.choose(&self.fall_bank, 23, x, z, strata.suite),
            Zone::Wet => self.choose(&r.wet, 3, x, z, strata.suite),
            Zone::Stony => self.choose(&r.stony, 4, x, z, strata.suite),
            Zone::Scree => self.choose(&r.scree, 5, x, z, strata.suite),
            Zone::Shore => {
                // River banks are damp and mossy wherever they run; lakes and the sea keep their
                // biome's beaches.
                let shore = match near {
                    Some(n) if n.river && !wetland(biome) => &self.recipe(Biome::River).shore,
                    _ => &r.shore,
                };
                self.choose(shore, 6, x, z, strata.suite)
            }
            Zone::Bed(_) if spray => self.choose(&self.fall_bed, 24, x, z, strata.suite),
            Zone::Bed(d) => {
                let beds = match col.water {
                    WaterKind::River if biome != Biome::River && !wetland(biome) => {
                        &self.river_beds
                    }
                    WaterKind::Swamp if !wetland(biome) => &self.swamp_beds,
                    _ => &r.beds,
                };
                self.choose(&beds[d as usize], 7 + d as u64, x, z, strata.suite)
            }
            Zone::Rock => {
                let (y, _) = col.ground();
                let rock = self.geology.rock(&strata, y - 1);
                let damp = spray || col.moisture + dither(self.seed, 20, x, z, 0.3) > 0.5;
                match self.geology.mossy(&strata, rock) {
                    Some(m) if r.mossy_rock && damp => m,
                    _ => rock,
                }
            }
        };
        Top {
            zone,
            biome,
            mat,
            strata,
            bank: zone != Zone::Rock && low_bank(col, near),
        }
    }

    /// The biome a column shows and its zone, without looking at its neighbours (so shores
    /// aren't told apart from the land behind them) and without working out the material.
    pub(crate) fn biome_zone(&self, x: i32, z: i32, col: &ColumnInfo) -> (Biome, Zone) {
        let biome = self.ground_biome(x, z, col);
        (biome, self.zone(x, z, col, biome, None))
    }

    /// What a column's top is made of, exactly as [`Surface::fill`] decides it. `None` when the
    /// column isn't in the grid.
    pub(crate) fn top_at(&self, x: i32, z: i32, grid: &ColumnGrid) -> Option<Top> {
        let col = grid.try_get(x, z)?;
        let near = if col.is_wet() {
            None
        } else {
            Self::near_water(x, z, grid)
        };
        // Waterfalls are on rivers, so only rivers and their banks look for one.
        let fall = (col.water == WaterKind::River || near.is_some_and(|n| n.river))
            .then(|| nearest_fall(x, z, grid, NEAR))
            .flatten();
        Some(self.top(x, z, col, near, fall))
    }

    /// The nearest water within [`NEAR`] blocks of a dry column.
    fn near_water(x: i32, z: i32, grid: &ColumnGrid) -> Option<Near> {
        let mut best: Option<(i32, &ColumnInfo)> = None;
        for dx in -NEAR..=NEAR {
            for dz in -NEAR..=NEAR {
                let d = dx * dx + dz * dz;
                if d > NEAR * NEAR || best.is_some_and(|(bd, _)| d >= bd) {
                    continue;
                }
                if let Some(c) = grid.try_get(x + dx, z + dz).filter(|c| c.is_wet()) {
                    best = Some((d, c));
                }
            }
        }
        best.map(|(d, c)| Near {
            level: c.water_level.unwrap_or_default(),
            dist: (d as f32).sqrt(),
            river: c.water == WaterKind::River,
        })
    }

    /// The lowest ground among the eight neighbours: rock is visible down to there.
    fn lowest_neighbour(x: i32, z: i32, own: i32, grid: &ColumnGrid) -> i32 {
        let mut low = own;
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(c) = grid.try_get(x + dx, z + dz) {
                    let (y, _) = c.ground();
                    // Under water the bed is hidden anyway.
                    low = low.min(y);
                }
            }
        }
        low
    }

    pub fn fill(&self, x: i32, z: i32, grid: &ColumnGrid, sink: &mut dyn BlockSink) {
        let col = grid.get(x, z);
        let Some(top) = self.top_at(x, z, grid) else {
            return;
        };
        let r = self.recipe(top.biome);
        let (y, layers) = col.ground();
        let wet = col.is_wet();
        let m = self.kit.mat(top.mat);

        // The thin top layer, if any.
        let mut surface = y;
        if layers > 0 {
            let l = layers as usize - 1;
            let thin = if top.zone == Zone::Rock {
                let rock = self.kit.mat(self.geology.rock(&top.strata, y));
                if r.snow && col.slope < 1.4 {
                    self.kit.mat(self.snow).thin[l]
                } else {
                    rock.thin[l]
                }
            } else if wet {
                m.thin_wet[l]
            } else {
                m.thin[l]
            };
            if thin != AIR {
                sink.set(x, y, z, thin);
                surface = y + 1;
            }
        }

        // The top full block, then soil.
        let organic = m.kind.organic();
        let topsoil = self.kit.mat(r.topsoil).full;
        let subsoil = self.kit.mat(r.subsoil).full;
        let full = if layers > 0 && organic {
            topsoil
        } else {
            m.full
        };
        sink.set(x, y - 1, z, full);
        let depth = match m.kind {
            Kind::Rock => 0,
            _ => {
                let flat = 1.0 - smoothstep(0.3, 1.4, col.slope);
                let n = value2(self.seed ^ 0x501, x as f64 / 11.0, z as f64 / 11.0) as f32;
                let d = ((r.soil_depth * flat * (1.0 + 0.35 * n)).round() as i32).max(1);
                // Banks show earth down to the water.
                if top.bank {
                    d.max(4)
                } else {
                    d
                }
            }
        };
        for k in 1..depth {
            let id = match m.kind {
                Kind::Sand | Kind::Gravel | Kind::Mud | Kind::Ice if k < 3 => m.full,
                _ if k == 1 && organic && layers == 0 => topsoil,
                _ => subsoil,
            };
            sink.set(x, y - 1 - k, z, id);
        }

        // Rock: strata where a cliff or bank could show them, plain stone below.
        let rock_top = y - 1 - depth;
        let shown = (Self::lowest_neighbour(x, z, y, grid) - 3).min(rock_top);
        let strata_from = shown.max(MIN_Y + 1);
        sink.set(x, MIN_Y, z, self.bedrock);
        sink.fill_column(x, z, MIN_Y + 1, strata_from, self.deep);
        for ry in strata_from..=rock_top {
            let rock = self.geology.rock(&top.strata, ry);
            sink.set(x, ry, z, self.kit.mat(rock).full);
        }

        if let Some(w) = col.water_level {
            sink.fill_column(x, z, surface, w + 1, self.water);
            if let Some(f) = col.fall {
                sink.fill_column(x, z, w + 1, f + 1, self.falling);
            }
        }
    }

    /// The Minecraft biome written for a column. Water gets ocean biomes, whose water colours
    /// are much lighter than plains or swamp water: Conquest's water texture is dark, and under
    /// the plains colour lakes look nearly black.
    pub fn minecraft_biome(&self, col: &ColumnInfo) -> &'static str {
        if col.is_wet() {
            return water_biome(col).0;
        }
        land_biome(col.biome).0
    }

    /// The Minecraft biome for the 4×4×4 biome cell of a column whose lowest block is at `y`.
    /// Biomes are three-dimensional, so only the cells holding water need the water biome:
    /// above them the land biome keeps the grass and foliage colours of trees and reeds that
    /// stand in the water, which the ocean biomes would turn a brighter green. A waterfall's
    /// curtain is water too, so it keeps the colour of the pools above and below it.
    pub fn minecraft_biome_at(&self, col: &ColumnInfo, y: i32) -> &'static str {
        match col.water_level {
            Some(w) if y <= col.fall.unwrap_or(w).max(w) => water_biome(col).0,
            _ => land_biome(col.biome).0,
        }
    }

    pub fn preview_color(&self, x: i32, z: i32, col: &ColumnInfo) -> [u8; 3] {
        let top = self.top(x, z, col, None, None);
        let m = self.kit.mat(top.mat);
        let mut c = m.color.map(|v| v as f32);
        if m.tinted {
            let g = land_biome(top.biome).1;
            for i in 0..3 {
                c[i] = c[i] * g[i] as f32 / 255.0 * 1.25;
            }
        }
        if col.is_wet() {
            // Conquest's grey water texture under the biome's water colour, over the bed,
            // which gets darker with depth.
            let tint = water_biome(col).1;
            let tex = [110.0, 120.0, 125.0];
            let t = (0.5 + col.depth() * 0.06).min(0.85);
            for i in 0..3 {
                let water = tex[i] * tint[i] as f32 / 255.0 * 1.6;
                c[i] = c[i] * (1.0 - t) * (1.0 - 0.04 * col.depth()).max(0.6) + water * t;
            }
            // White water where a waterfall comes down.
            if col.fall.is_some() {
                for v in &mut c {
                    *v = 0.4 * *v + 0.6 * 235.0;
                }
            }
        }
        c.map(|v| v.clamp(0.0, 255.0) as u8)
    }
}

/// The Minecraft biome for land of a biome, and its grass colour.
fn land_biome(b: Biome) -> (&'static str, [u8; 3]) {
    match b {
        Biome::Plains | Biome::Lake => ("minecraft:plains", [0x91, 0xBD, 0x59]),
        Biome::Meadow | Biome::AlpineMeadow => ("minecraft:meadow", [0x83, 0xBB, 0x6D]),
        Biome::Forest => ("minecraft:forest", [0x79, 0xC0, 0x5A]),
        Biome::BirchForest => ("minecraft:birch_forest", [0x88, 0xBB, 0x67]),
        Biome::Taiga => ("minecraft:taiga", [0x86, 0xB7, 0x83]),
        Biome::PineForest => ("minecraft:old_growth_pine_taiga", [0x86, 0xB8, 0x7F]),
        Biome::Swamp | Biome::Marsh => ("minecraft:swamp", [0x6A, 0x70, 0x39]),
        Biome::Highlands => ("minecraft:windswept_hills", [0x8A, 0xB6, 0x89]),
        Biome::Mountains => ("minecraft:windswept_gravelly_hills", [0x8A, 0xB6, 0x89]),
        Biome::SnowyPeaks => ("minecraft:snowy_slopes", [0x80, 0xB4, 0x97]),
        Biome::Beach => ("minecraft:beach", [0x91, 0xBD, 0x59]),
        Biome::River => ("minecraft:river", [0x8E, 0xB9, 0x71]),
    }
}

/// The Minecraft biome for water, and its water colour: turquoise in the shallows, light blue
/// further out. Swamp water too: the swamp's own murky colour is nearly black under Conquest's
/// water texture; the mud and peat beds, reeds and duckweed make swamps look swampy instead.
fn water_biome(col: &ColumnInfo) -> (&'static str, [u8; 3]) {
    if col.biome == Biome::SnowyPeaks {
        ("minecraft:snowy_slopes", [0x3D, 0x57, 0xD6])
    } else if col.depth() < 3.0 {
        ("minecraft:warm_ocean", [0x43, 0xD5, 0xEE])
    } else {
        ("minecraft:lukewarm_ocean", [0x45, 0xAD, 0xF2])
    }
}

#[cfg(test)]
pub(crate) mod tests;
