//! The ground of roads drawn with the road brush, which the terrain marks with
//! [`ColumnInfo::road`]: trodden earth from the dirt family (coarse and loamy dirt, packed and
//! dry mud, brown earth), darker packed earth and mud in the wheel ruts, the odd stone, and
//! grass growing in over the edges, so the road's border frays into the land rather than
//! ending along a line. Wet ground makes muddier roads, dry ground dustier ones. Everything is
//! about as dark as the soil around it: no light band runs along a road.
//!
//! Decor asks [`bare`] whether anything may grow on a road column.

use crate::column::{ColumnGrid, ColumnInfo};

use super::field::{dither, hash_unit, patch, pick, smoothstep, unit};
use super::materials::{Kit, Mat};

/// Salts of the road decisions.
mod salt {
    pub const EDGE: u64 = 0x40AD_0001;
    pub const VERGE: u64 = 0x40AD_0002;
    pub const RUT: u64 = 0x40AD_0003;
    pub const PATCH: u64 = 0x40AD_0004;
    pub const ACCENT: u64 = 0x40AD_0005;
    pub const HUMP: u64 = 0x40AD_0006;
    pub const BARE: u64 = 0x40AD_0007;
}

/// Distance of the outer wheel ruts from a road's edge, and between ruts further in.
const RUT_EDGE: f32 = 1.3;
const RUT_GAUGE: f32 = 2.6;

/// How strongly a wheel rut runs `inside` blocks from a road's edge, 0..1. Carts keep to a
/// track a little in from each edge; wide roads have a few tracks side by side, fainter
/// towards the middle.
pub(crate) fn rut(inside: f32) -> f32 {
    if inside <= 0.3 {
        return 0.0;
    }
    let k = ((inside - RUT_EDGE) / RUT_GAUGE).round().max(0.0);
    if k > 2.0 {
        return 0.0;
    }
    let d = (inside - RUT_EDGE - k * RUT_GAUGE).abs();
    (1.0 - smoothstep(0.15, 0.75, d)) * (1.0 - 0.25 * k)
}

/// Whether nothing grows on a column `inside` blocks within a road's edge: almost never on
/// the road, less and less often towards the edge, and as usual a couple of blocks out.
pub(crate) fn bare(seed: u64, x: i32, z: i32, inside: f32) -> bool {
    let p = 0.98 * smoothstep(-1.8, 0.8, inside + dither(seed, salt::BARE, x, z, 0.8));
    unit(seed, salt::BARE, x, z) < p
}

/// Whether a road runs within `r` blocks of a column (for keeping boulders and logs off it).
pub(crate) fn near(grid: &ColumnGrid, x: i32, z: i32, r: i32) -> bool {
    (-r..=r).any(|dx| {
        (-r..=r).any(|dz| {
            grid.try_get(x + dx, z + dz)
                .is_some_and(|c| c.road.is_some_and(|i| i > -1.5))
        })
    })
}

type List = Vec<(Mat, u32)>;

/// Road materials, resolved in the ground kit.
pub(crate) struct RoadKit {
    /// The trodden surface and the ruts, for dry, ordinary and wet ground.
    trodden: [List; 3],
    ruts: [List; 3],
    accent: List,
    /// Grass growing through the earth at the edges and between the ruts.
    grassy: Mat,
}

impl RoadKit {
    pub(crate) fn new(kit: &Kit) -> Self {
        let res = |l: &[(&str, u32)]| l.iter().map(|&(k, w)| (kit.get(k), w)).collect::<List>();
        Self {
            trodden: [
                res(&[
                    ("coarse_dirt", 4),
                    ("dry_mud", 3),
                    ("loamy_dirt", 2),
                    ("rocky_soil", 1),
                    ("coarse_sandy_soil", 1),
                ]),
                res(&[
                    ("coarse_dirt", 4),
                    ("loamy_dirt", 3),
                    ("brown_earth", 2),
                    ("packed_mud", 1),
                    ("dry_mud", 1),
                ]),
                res(&[
                    ("packed_mud", 3),
                    ("loamy_dirt", 2),
                    ("mud", 2),
                    ("coarse_dirt", 1),
                ]),
            ],
            ruts: [
                res(&[("coarse_dirt", 3), ("packed_mud", 2), ("dry_mud", 1)]),
                res(&[("packed_mud", 3), ("coarse_dirt", 2), ("mud", 1)]),
                res(&[("mud", 3), ("thick_mud", 2), ("packed_mud", 1)]),
            ],
            accent: res(&[("small_stones", 2), ("brown_gravel", 1), ("rocky_soil", 2)]),
            grassy: kit.get("grassy_dirt"),
        }
    }

    /// The ground of a road column `inside` blocks within the road's edge, or `None` where the
    /// land's own ground shows: past the frayed edge.
    pub(crate) fn ground(
        &self,
        seed: u64,
        x: i32,
        z: i32,
        col: &ColumnInfo,
        inside: f32,
    ) -> Option<Mat> {
        // The edge frays in and out by about a block.
        let edge = inside - 0.3 - dither(seed, salt::EDGE, x, z, 1.2);
        if edge < 0.0 {
            return None;
        }
        // Grass grows in over the first block or so.
        let creep = 1.0 - smoothstep(0.0, 1.4, edge);
        if unit(seed, salt::VERGE, x, z) < 0.75 * creep {
            return Some(self.grassy);
        }
        let wet = col.moisture + dither(seed, salt::PATCH, x, z, 0.15);
        let w = if wet > 0.72 {
            2
        } else if wet < 0.38 {
            0
        } else {
            1
        };
        let r = rut(inside) + dither(seed, salt::RUT, x, z, 0.35);
        if r > 0.55 {
            return Some(pick(&self.ruts[w], hash_unit(patch(seed, salt::RUT, x, z, 2.5))));
        }
        // A grassy hump between the outer ruts here and there.
        if r < 0.1
            && inside > RUT_EDGE + 0.6
            && inside < RUT_EDGE + RUT_GAUGE - 0.6
            && hash_unit(patch(seed, salt::HUMP, x, z, 5.0)) < 0.35
            && unit(seed, salt::HUMP, x, z) < 0.5
        {
            return Some(self.grassy);
        }
        if hash_unit(patch(seed, salt::ACCENT, x, z, 2.0)) < 0.08 {
            return Some(pick(&self.accent, unit(seed, salt::ACCENT, x, z)));
        }
        Some(pick(
            &self.trodden[w],
            hash_unit(patch(seed, salt::PATCH, x, z, 3.0)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruts_sit_in_from_the_edges() {
        assert_eq!(rut(0.0), 0.0);
        assert!(rut(RUT_EDGE) > 0.95);
        assert!(rut(RUT_EDGE + RUT_GAUGE / 2.0) < 0.05);
        assert!(rut(RUT_EDGE + RUT_GAUGE) > 0.7);
        assert_eq!(rut(RUT_EDGE + 3.0 * RUT_GAUGE), 0.0);
    }

    #[test]
    fn roads_are_almost_bare() {
        let share = |inside: f32| {
            (0..4000)
                .filter(|&i| bare(5, i % 64, i / 64, inside))
                .count() as f32
                / 4000.0
        };
        assert!(share(3.0) > 0.95, "{}", share(3.0));
        assert!(share(-4.0) < 0.01, "{}", share(-4.0));
    }
}
