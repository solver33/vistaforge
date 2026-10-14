//! Decor: everything that stands on the ground — where trees grow and which species, and the
//! plants, flowers, loose rocks, fallen leaves, moss, boulders, fallen logs and stumps between
//! them.
//!
//! Every decision must be a function of the seed and the position (hash the coordinates, as
//! [`hash2`] does), never of a random sequence shared across the map: the engine plans the same
//! trees and plants again for each overlapping piece and they have to come out identical.
//! Nothing may be placed more than [`MAX_REACH`] blocks from the column it belongs to.
//!
//! - Trees (`forest`): one candidate per 4×4 cell, spaced by a hash-only rule, kept by the
//!   local density of the biome's open land and groves.
//! - Objects (`objects`): boulders, fallen logs and stumps, one candidate per 10×10 cell. An
//!   object's shape depends only on grid columns within [`MAX_REACH`] of its anchor and on the
//!   trees (worked out from the same columns); each of its blocks is written only into a free
//!   cell.
//! - Ground cover, column by column: loose rocks, fallen leaves under tree crowns, moss and
//!   lichen carpets, plant communities in drifts of one species, reeds in the shallows and
//!   lilies on still water. Each column looks only at its own cells in the sink, so it comes
//!   out the same in every piece.
//! - Waterfalls: plain falling water comes down the curtain (the surface fills it), and no reeds
//!   or lilies grow in the churning water at its foot.

pub(crate) mod flora;
pub(crate) mod forest;
pub(crate) mod objects;

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::biome::Biome;
use crate::column::{ColumnGrid, ColumnInfo, Rect, WaterKind};
use crate::noise::{hash2, hash64, value2};
use crate::settings::WorldSettings;
use crate::sink::{BlockId, BlockSink, Blocks, AIR};
use crate::surface::field::{hash_unit, patch, pick, unit};
use crate::surface::geology::SUITES;
use crate::surface::materials::Kind;
use crate::surface::{Surface, Top, Zone};
use crate::trees::TreeRequest;

use flora::{Community, Flora, Form, PlantId};
use forest::Forest;
use objects::Objects;

/// Furthest decor (other than trees) may reach from the column it belongs to.
pub const MAX_REACH: i32 = 8;

/// Salts of the ground-cover decisions.
mod salt {
    pub const RUBBLE: u64 = 0xD0C1;
    pub const LITTER: u64 = 0xD0C2;
    pub const CARPET: u64 = 0xD0C3;
    pub const PLANT: u64 = 0xD0C4;
    pub const DRIFT: u64 = 0xD0C5;
    pub const GRASS: u64 = 0xD0C6;
    pub const LUSH: u64 = 0xD0C7;
    pub const SHAPE: u64 = 0xD0C8;
    pub const REED: u64 = 0xD0C9;
    pub const FLOAT: u64 = 0xD0CA;
}

/// The zones a community is chosen by.
const ZONES: usize = 8;

fn zone_index(z: Zone) -> usize {
    match z {
        Zone::Open => 0,
        Zone::Wood => 1,
        Zone::Wet => 2,
        Zone::Stony => 3,
        Zone::Scree => 4,
        Zone::Rock => 5,
        Zone::Shore => 6,
        Zone::Bed(_) => 7,
    }
}

const ZONE_OF_INDEX: [Zone; ZONES] = [
    Zone::Open,
    Zone::Wood,
    Zone::Wet,
    Zone::Stony,
    Zone::Scree,
    Zone::Rock,
    Zone::Shore,
    Zone::Bed(0),
];

/// A community with its plants resolved.
struct Plants {
    density: f32,
    patch: f64,
    background: Vec<(PlantId, u32)>,
    background_share: f32,
    drifts: Vec<(PlantId, u32)>,
}

impl Plants {
    fn new(flora: &Flora, c: &Community) -> Self {
        Self {
            density: c.density,
            patch: c.patch,
            background: flora.list(c.background),
            background_share: c.background_share,
            drifts: flora.list(c.drifts),
        }
    }
}

/// Where something standing on a column's ground goes.
#[derive(Clone, Copy)]
struct Spot {
    /// The cell it goes in.
    y: i32,
    /// `layers` of what it stands on (8 for a full block).
    layers: u8,
    kind: Kind,
    /// The cell holds water.
    wet: bool,
}

/// The crown over a column: the litter it drops, and whether it is a conifer's.
#[derive(Clone, Copy)]
struct Shade {
    litter: Option<PlantId>,
    needles: bool,
}

/// What a block above the ground says about shade.
#[derive(Clone, Copy)]
enum Above {
    /// Tree leaves or needles: the ground under them takes their litter.
    Leaves(Shade),
    /// Wood: keep looking up.
    Wood,
    Other,
}

pub struct Decor {
    seed: u64,
    surface: Surface,
    forest: Forest,
    objects: Objects,
    flora: Flora,
    water: BlockId,
    /// The vegetation slider as a density factor (1 at the default).
    plants: f32,
    /// The detail slider as a factor (1 at the default).
    detail: f32,
    /// `[biome * ZONES + zone]`.
    communities: Vec<Plants>,
    emergent: Vec<Vec<(PlantId, u32)>>,
    floating: Vec<Vec<(PlantId, u32)>>,
    carpets: Vec<Vec<(PlantId, u32)>>,
    /// Loose rocks per geological suite.
    rubble: Vec<Vec<(PlantId, u32)>>,
    /// Fallen leaves by a word in the name of the tree leaves above, and the default.
    litter: Vec<(&'static str, PlantId)>,
    dead_branches: Option<PlantId>,
}

const LITTER: &[(&str, &str)] = &[
    ("birch", "conquest:birch_ground_leaves"),
    ("spruce", "conquest:spruce_ground_needles"),
    ("pine", "conquest:pine_ground_needles"),
    ("larch", "conquest:larch_ground_needles"),
    ("willow", "conquest:goat_willow_ground_leaves"),
    ("beech", "conquest:beech_tree_ground_leaves"),
    ("maple", "conquest:maple_ground_leaves"),
    ("aspen", "conquest:aspen_ground_leaves"),
    ("oak", "conquest:oak_ground_leaves"),
];
/// Litter words of the needle-bearing trees.
const CONIFERS: &[&str] = &["spruce", "pine", "larch"];
const DEAD_BRANCHES: &str = "conquest:dead_deciduous_ground_branches";

impl Decor {
    pub fn new(b: &Blocks, settings: &WorldSettings) -> Result<Self> {
        let surface = Surface::new(b, settings)?;
        let mut extra: Vec<&str> = LITTER.iter().map(|l| l.1).collect();
        extra.push(DEAD_BRANCHES);
        for s in SUITES {
            extra.extend(s.rubble.iter().copied());
        }
        let flora = Flora::new(b, &extra)?;
        let mut communities = Vec::with_capacity(Biome::ALL.len() * ZONES);
        for &biome in Biome::ALL {
            for zone in ZONE_OF_INDEX {
                communities.push(Plants::new(&flora, flora::community(biome, zone)));
            }
        }
        let per_biome = |f: fn(Biome) -> &'static [(&'static str, u32)]| {
            Biome::ALL
                .iter()
                .map(|&b| flora.list(f(b)))
                .collect::<Vec<_>>()
        };
        let sl = &settings.sliders;
        Ok(Self {
            seed: settings.seed,
            forest: Forest::new(settings.seed, sl.trees),
            objects: Objects::new(b, settings.seed, sl.detail)?,
            water: b.id("minecraft:water", &[("level", "0")])?,
            plants: sl.vegetation.clamp(0.0, 1.0) * 2.0,
            detail: sl.detail.clamp(0.0, 1.0) * 2.0,
            emergent: per_biome(flora::emergent),
            floating: per_biome(flora::floating),
            carpets: per_biome(flora::carpets),
            rubble: SUITES
                .iter()
                .map(|s| {
                    s.rubble
                        .iter()
                        .filter_map(|n| flora.find(n))
                        .map(|id| (id, 1))
                        .collect()
                })
                .collect(),
            litter: LITTER
                .iter()
                .filter_map(|&(word, name)| flora.find(name).map(|id| (word, id)))
                .collect(),
            dead_branches: flora.find(DEAD_BRANCHES),
            communities,
            flora,
            surface,
        })
    }

    /// Trees whose trunks stand in `rect`. `column` gives the terrain of any column.
    pub fn plan_trees(
        &self,
        rect: Rect,
        column: &dyn Fn(i32, i32) -> ColumnInfo,
    ) -> Vec<TreeRequest> {
        if rect.is_empty() {
            return Vec::new();
        }
        Forest::cells(rect.x0, rect.z0, rect.x1, rect.z1)
            .filter_map(|(gx, gz)| {
                self.forest
                    .tree(&self.surface, gx, gz, |x, z| Some(column(x, z)))
            })
            .filter(|t| rect.contains(t.x, t.z))
            .collect()
    }

    /// Plants and other ground detail for the columns of `rect`, after the trees are in.
    pub fn decorate(&self, rect: Rect, grid: &ColumnGrid, sink: &mut dyn BlockSink) {
        // Trunks known from the grid, for keeping objects clear of them.
        let g = grid.rect;
        let trunks: HashSet<(i32, i32)> = Forest::cells(g.x0, g.z0, g.x1, g.z1)
            .filter_map(|(gx, gz)| {
                self.forest
                    .tree(&self.surface, gx, gz, |x, z| grid.try_get(x, z).copied())
            })
            .map(|t| (t.x, t.z))
            .collect();
        let near_tree = |x: i32, z: i32, d: i32| {
            (-d..=d).any(|dx| (-d..=d).any(|dz| trunks.contains(&(x + dx, z + dz))))
        };

        let (cell, oc) = (objects::CELL, &self.objects);
        for gx in rect.x0.div_euclid(cell)..=(rect.x1 - 1).div_euclid(cell) {
            for gz in rect.z0.div_euclid(cell)..=(rect.z1 - 1).div_euclid(cell) {
                let (ax, az, _) = oc.anchor(gx, gz);
                if rect.contains(ax, az) {
                    oc.place(&self.surface, &self.flora, gx, gz, grid, &near_tree, sink);
                }
            }
        }

        let mut above = HashMap::new();
        for (x, z) in rect.columns() {
            self.cover(x, z, grid, sink, &mut above);
        }
    }

    /// Where something standing on a column's ground goes, if the cell is free.
    fn spot(&self, x: i32, z: i32, col: &ColumnInfo, sink: &dyn BlockSink) -> Option<Spot> {
        let kit = &self.surface.kit;
        let (y, _) = col.ground();
        let id = sink.get(x, y, z);
        let (mat, layers, py) = match kit.of_block(id) {
            Some((m, l)) => (m, if l == 0 { 8 } else { l }, y + 1),
            None if id == AIR || id == self.water => match kit.of_block(sink.get(x, y - 1, z)) {
                Some((m, 0)) => (m, 8, y),
                _ => return None,
            },
            None => return None,
        };
        let here = sink.get(x, py, z);
        if here != AIR && here != self.water {
            return None;
        }
        Some(Spot {
            y: py,
            layers,
            kind: kit.mat(mat).kind,
            wet: here == self.water,
        })
    }

    /// Litter for the ground under the first tree leaves above `y`, if there are leaves.
    fn canopy(
        &self,
        x: i32,
        y: i32,
        z: i32,
        sink: &dyn BlockSink,
        cache: &mut HashMap<BlockId, Above>,
    ) -> Option<Shade> {
        for dy in 1..=32 {
            let id = sink.get(x, y + dy, z);
            if id == AIR {
                continue;
            }
            let above = *cache.entry(id).or_insert_with(|| {
                sink.palette().with(id, |s| {
                    if s.name.contains("leaves") || s.name.contains("needles") {
                        let litter = self
                            .litter
                            .iter()
                            .find(|(word, _)| s.name.contains(word))
                            .or(self.litter.last());
                        Above::Leaves(Shade {
                            litter: litter.map(|l| l.1),
                            needles: litter.is_some_and(|l| CONIFERS.contains(&l.0)),
                        })
                    } else if s.name.contains("log") || s.name.contains("branch") {
                        Above::Wood
                    } else {
                        Above::Other
                    }
                })
            });
            match above {
                Above::Leaves(l) => return Some(l),
                Above::Wood => continue,
                Above::Other => return None,
            }
        }
        None
    }

    fn cover(
        &self,
        x: i32,
        z: i32,
        grid: &ColumnGrid,
        sink: &mut dyn BlockSink,
        cache: &mut HashMap<BlockId, Above>,
    ) {
        let Some(col) = grid.try_get(x, z) else {
            return;
        };
        let Some(spot) = self.spot(x, z, col, sink) else {
            return;
        };
        let Some(top) = self.surface.top_at(x, z, grid) else {
            return;
        };
        if col.is_wet() {
            self.aquatic(x, z, col, &top, spot, grid, sink);
            return;
        }
        if spot.wet {
            return;
        }
        let s = self.seed;
        let biome = top.biome;
        let bi = biome as usize;
        let shade = self.canopy(x, spot.y, z, sink, cache);
        let zone = match (shade, top.zone) {
            (Some(_), Zone::Open) => Zone::Wood,
            (_, z) => z,
        };
        let kind = spot.kind;

        // Loose rocks.
        let rocky = matches!(
            biome,
            Biome::Mountains | Biome::Highlands | Biome::AlpineMeadow | Biome::SnowyPeaks
        );
        let rubble = match zone {
            Zone::Scree => 0.2,
            Zone::Stony => 0.09,
            Zone::Rock => 0.05,
            Zone::Shore => 0.03,
            _ => 0.006,
        } * if rocky { 1.5 } else { 1.0 }
            * self.detail;
        if !matches!(kind, Kind::Snow | Kind::Ice) && unit(s, salt::RUBBLE, x, z) < rubble {
            self.rubble(x, z, spot, &top, grid, sink);
            return;
        }

        // Fallen leaves under crowns, in drifts, and dead twigs in the woods.
        let soft = kind.organic() || matches!(kind, Kind::Soil | Kind::Mud);
        if let Some(shade) = shade {
            // Conifers drop a fuller carpet of needles than broadleaf trees do of leaves.
            let (share, inside, outside) = if shade.needles {
                (0.6, 0.7, 0.15)
            } else {
                (0.45, 0.6, 0.07)
            };
            let drift = hash_unit(patch(s, salt::LITTER, x, z, 6.0)) < share;
            let p = if drift { inside } else { outside };
            if soft && unit(s, salt::LITTER, x, z) < p {
                if let Some(l) = shade.litter {
                    self.put(l, x, z, spot, 0, sink);
                    return;
                }
            }
        } else if zone == Zone::Wood && soft && unit(s, salt::LITTER, x, z) < 0.04 * self.detail {
            if let Some(d) = self.dead_branches {
                self.put(d, x, z, spot, 0, sink);
                return;
            }
        }

        // Moss and lichen carpets lie on full blocks.
        if spot.layers == 8 && !matches!(kind, Kind::Snow | Kind::Ice | Kind::Sand) {
            let base = match (zone, biome) {
                (Zone::Wood, Biome::Taiga | Biome::Forest | Biome::Swamp) => 0.16,
                (Zone::Wood, _) => 0.08,
                (Zone::Rock | Zone::Stony, _) if rocky => 0.12,
                (Zone::Rock, _) => 0.06,
                (Zone::Wet, _) => 0.1,
                _ => 0.015,
            };
            let clump = hash_unit(patch(s, salt::CARPET, x, z, 5.0)) < 0.4;
            let p = base * if clump { 2.2 } else { 0.3 } * self.detail;
            if unit(s, salt::CARPET, x, z) < p && !self.carpets[bi].is_empty() {
                let m = pick(
                    &self.carpets[bi],
                    hash_unit(patch(s, salt::CARPET ^ 1, x, z, 9.0)),
                );
                self.put(m, x, z, spot, 0, sink);
                return;
            }
        }

        // Plants.
        if matches!(kind, Kind::Rock | Kind::Snow | Kind::Ice) {
            return;
        }
        let comm = &self.communities[bi * ZONES + zone_index(zone)];
        if comm.background.is_empty() && comm.drifts.is_empty() {
            return;
        }
        let lush = 0.75 + 0.35 * value2(s ^ salt::LUSH, x as f64 / 19.0, z as f64 / 19.0) as f32;
        let density = comm.density * self.plants * lush * if shade.is_some() { 0.6 } else { 1.0 };
        if unit(s, salt::PLANT, x, z) >= density {
            return;
        }
        let ph = patch(s, salt::DRIFT ^ ((bi as u64) << 8), x, z, comm.patch);
        let drift = !comm.drifts.is_empty() && hash_unit(ph) >= comm.background_share;
        let plant = if drift && (comm.background.is_empty() || unit(s, salt::GRASS, x, z) < 0.75) {
            pick(&comm.drifts, hash_unit(hash64(ph)))
        } else if !comm.background.is_empty() {
            pick(&comm.background, unit(s, salt::GRASS ^ 1, x, z))
        } else {
            return;
        };
        let h = hash2(s ^ salt::SHAPE, x as i64, z as i64);
        self.put(plant, x, z, spot, h, sink);
    }

    /// Places a plant at a spot: picks its variant and height from `h`, stacks two-block plants
    /// where there is room.
    fn put(&self, id: PlantId, x: i32, z: i32, spot: Spot, h: u64, sink: &mut dyn BlockSink) {
        let p = self.flora.get(id);
        let (y, l, wet) = (spot.y, spot.layers, spot.wet);
        let free = |sink: &dyn BlockSink, y: i32| sink.get(x, y, z) == AIR;
        match p.form {
            Form::Tall(n) => {
                // One block; its `height` draws the model up into the empty cells above.
                let want = if n >= 3 {
                    2 + (h >> 8) % (n as u64 - 1)
                } else {
                    1 + (h >> 8) % n as u64
                } as i32;
                let mut height = 1;
                while height < want && free(sink, y + height) {
                    height += 1;
                }
                if let Some(b) = p.id(height as usize - 1, false, l, wet) {
                    sink.set(x, y, z, b);
                }
            }
            Form::Double => {
                if !free(sink, y + 1) {
                    return;
                }
                if let (Some(lo), Some(up)) = (p.id(0, false, l, wet), p.id(0, true, l, false)) {
                    sink.set(x, y, z, lo);
                    sink.set(x, y + 1, z, up);
                }
            }
            _ => {
                let v = (h % p.variants() as u64) as usize;
                let Some(b) = p.id(v, false, l, wet) else {
                    return;
                };
                sink.set(x, y, z, b);
                let tall = p.form.stacks() && (h >> 24) % 100 < 35;
                if tall && free(sink, y + 1) {
                    if let Some(up) = p.id(v, true, l, false) {
                        sink.set(x, y + 1, z, up);
                    }
                }
            }
        }
    }

    /// Loose rocks of the local geology.
    fn rubble(
        &self,
        x: i32,
        z: i32,
        spot: Spot,
        top: &Top,
        grid: &ColumnGrid,
        sink: &mut dyn BlockSink,
    ) {
        let list = &self.rubble[top.strata.suite];
        if list.is_empty() {
            return;
        }
        let h = hash2(self.seed ^ salt::RUBBLE, x as i64, z as i64);
        let id = pick(list, hash_unit(h));
        let density = match top.zone {
            Zone::Scree => 2 + (h >> 8) % 3,
            Zone::Stony | Zone::Rock => 1 + (h >> 8) % 3,
            _ => 1 + (h >> 8) % 2,
        } as usize;
        // An edge where the ground drops away next to the stone.
        let own = grid.get(x, z).height;
        let edge = [(0, -1), (1, 0), (0, 1), (-1, 0)].iter().any(|(dx, dz)| {
            grid.try_get(x + dx, z + dz)
                .is_none_or(|c| c.height < own - 0.3)
        });
        let variant = (density - 1) * 2 + edge as usize;
        if let Some(b) = self.flora.get(id).id(variant, false, spot.layers, spot.wet) {
            sink.set(x, spot.y, z, b);
        }
    }

    /// Reeds in the shallows, stones on river beds, lilies and duckweed on still water.
    #[allow(clippy::too_many_arguments)]
    fn aquatic(
        &self,
        x: i32,
        z: i32,
        col: &ColumnInfo,
        top: &Top,
        spot: Spot,
        grid: &ColumnGrid,
        sink: &mut dyn BlockSink,
    ) {
        let Some(w) = col.water_level else {
            return;
        };
        let s = self.seed;
        let bi = top.biome as usize;
        // Water cells from the spot up to the surface; 0 when only the waterlogged ground layer
        // under the spot holds water.
        let cells = w - spot.y + 1;
        if cells < 0 || (cells == 0) == spot.wet {
            return;
        }
        let swampy =
            matches!(top.biome, Biome::Swamp | Biome::Marsh) || col.water == WaterKind::Swamp;
        let river = col.water == WaterKind::River;

        if river
            && !swampy
            && matches!(top.zone, Zone::Bed(0 | 1))
            && unit(s, salt::RUBBLE, x, z) < 0.05 * self.detail
        {
            self.rubble(x, z, spot, top, grid, sink);
            return;
        }

        // Nothing grows in the churning water under a waterfall.
        if river && crate::surface::nearest_fall(x, z, grid, 2).is_some() {
            return;
        }

        let shallow =
            matches!(top.zone, Zone::Bed(0)) || (swampy && matches!(top.zone, Zone::Bed(1)));
        let reeds = &self.emergent[bi];
        if shallow && !reeds.is_empty() {
            let rp = patch(s, salt::REED, x, z, 6.0);
            let base = if swampy {
                0.55
            } else if river {
                0.2
            } else {
                0.32
            };
            let p = base * self.plants * if hash_unit(rp) < 0.5 { 1.0 } else { 0.12 };
            if unit(s, salt::REED, x, z) < p {
                let id = if unit(s, salt::REED ^ 1, x, z) < 0.7 {
                    pick(reeds, hash_unit(hash64(rp)))
                } else {
                    pick(reeds, unit(s, salt::REED ^ 2, x, z))
                };
                if self.emergent_plant(id, x, z, spot, cells, sink) {
                    return;
                }
            }
        }

        let lilies = &self.floating[bi];
        if !river && cells <= 4 && !lilies.is_empty() && sink.get(x, w + 1, z) == AIR {
            let fp = patch(s, salt::FLOAT, x, z, 7.0);
            let (share, p) = if swampy { (0.5, 0.55) } else { (0.22, 0.45) };
            if hash_unit(fp) < share && unit(s, salt::FLOAT, x, z) < p * self.plants {
                let id = pick(lilies, hash_unit(hash64(fp)));
                let p = self.flora.get(id);
                let v = (hash2(s ^ salt::FLOAT, x as i64, z as i64) % p.variants() as u64) as usize;
                if let Some(b) = p.id(v, false, 8, false) {
                    sink.set(x, w + 1, z, b);
                }
            }
        }
    }

    /// A reed standing in water `cells` deep (0: on ground whose layer is waterlogged),
    /// sticking out above it. False when it can't.
    fn emergent_plant(
        &self,
        id: PlantId,
        x: i32,
        z: i32,
        spot: Spot,
        cells: i32,
        sink: &mut dyn BlockSink,
    ) -> bool {
        let p = self.flora.get(id);
        let h = hash2(self.seed ^ salt::REED, x as i64, z as i64);
        let wet = cells > 0;
        match p.form {
            // One block on the bed, its model reaching through the water and at least a block
            // above it, into empty cells.
            Form::Tall(n) => {
                let need = cells + 1;
                if need > n as i32 {
                    return false;
                }
                let want = (need + (h % 3) as i32).min(n as i32);
                let mut height = need;
                while height < want && sink.get(x, spot.y + height, z) == AIR {
                    height += 1;
                }
                if (cells.max(1)..height).any(|dy| sink.get(x, spot.y + dy, z) != AIR) {
                    return false;
                }
                match p.id(height as usize - 1, false, spot.layers, wet) {
                    Some(b) => {
                        sink.set(x, spot.y, z, b);
                        true
                    }
                    None => false,
                }
            }
            Form::Height2 | Form::Stack if cells <= 1 => {
                let v = (h % p.variants() as u64) as usize;
                let (Some(lo), Some(up)) = (
                    p.id(v, false, spot.layers, wet),
                    p.id(v, true, spot.layers, false),
                ) else {
                    return false;
                };
                if sink.get(x, spot.y + 1, z) != AIR {
                    return false;
                }
                sink.set(x, spot.y, z, lo);
                sink.set(x, spot.y + 1, z, up);
                true
            }
            _ if cells == 0 => {
                self.put(id, x, z, spot, h, sink);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
