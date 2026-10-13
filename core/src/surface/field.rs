//! Noise fields shared by `surface` and `decor`, so ground cover, plants and trees agree on where
//! patches, biome borders, groves and clearings are. Every function here depends only on the
//! seed and the coordinates (and the column facts passed in).

use crate::biome::Biome;
use crate::column::ColumnInfo;
use crate::noise::{fbm2, hash2, unit2, value2};

/// Salts that keep the different fields of one seed independent.
pub(crate) mod salt {
    pub const BLEND: u64 = 0xB1E4_D000;
    pub const WOOD: u64 = 0x6A07_E000;
    pub const PATCH: u64 = 0x9A7C_4000;
    pub const DITHER: u64 = 0xD174_E000;
    pub const GEOLOGY: u64 = 0x6E01_0000;
}

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Uniform 0..1 for a column and a salt.
pub(crate) fn unit(seed: u64, salt: u64, x: i32, z: i32) -> f32 {
    unit2(seed ^ salt, x as i64, z as i64) as f32
}

/// A small, blotchy offset (about ±`amount`) for thresholds, so borders between ground types
/// are ragged and organic rather than following a contour line exactly.
pub(crate) fn dither(seed: u64, salt: u64, x: i32, z: i32, amount: f32) -> f32 {
    let (fx, fz) = (x as f64, z as f64);
    let blob = value2(seed ^ salt ^ salt::DITHER, fx / 5.0, fz / 5.0) as f32;
    let speck = unit(seed, salt ^ salt::DITHER, x, z) - 0.5;
    amount * (0.75 * blob + 0.5 * speck)
}

/// The biome a column shows. Near a border (`blend` > 0) the neighbouring biome takes over in
/// interlocking patches rather than along a straight line.
pub(crate) fn local_biome(seed: u64, x: i32, z: i32, col: &ColumnInfo) -> Biome {
    if col.blend <= 0.0 || col.biome2 == col.biome {
        return col.biome;
    }
    let (fx, fz) = (x as f64, z as f64);
    let t = 0.5
        + 0.4 * fbm2(seed ^ salt::BLEND, fx / 9.0, fz / 9.0, 2) as f32
        + 0.2 * (unit(seed, salt::BLEND, x, z) - 0.5);
    if t < col.blend {
        col.biome2
    } else {
        col.biome
    }
}

/// An id shared by all columns of one irregular patch about `scale` blocks across. Patches are
/// warped Voronoi cells with frayed edges; `salt` picks an independent pattern.
pub(crate) fn patch(seed: u64, salt: u64, x: i32, z: i32, scale: f64) -> u64 {
    let s = seed ^ salt ^ salt::PATCH;
    let (fx, fz) = (x as f64, z as f64);
    let w = scale * 0.8;
    let fray = 1.4 * (unit2(s ^ 0xF1, x as i64, z as i64) - 0.5);
    let wx = fx + scale * 0.5 * value2(s ^ 0x3A1, fx / w, fz / w) + fray;
    let wz = fz + scale * 0.5 * value2(s ^ 0x3A2, fx / w, fz / w) - fray;
    let (cx, cz) = ((wx / scale).floor() as i64, (wz / scale).floor() as i64);
    let mut best = (f64::MAX, 0u64);
    for gx in cx - 1..=cx + 1 {
        for gz in cz - 1..=cz + 1 {
            let h = hash2(s, gx, gz);
            let px = (gx as f64 + (h & 0xFFFF) as f64 / 65536.0) * scale;
            let pz = (gz as f64 + ((h >> 16) & 0xFFFF) as f64 / 65536.0) * scale;
            let d = (px - wx).powi(2) + (pz - wz).powi(2);
            if d < best.0 {
                best = (d, h);
            }
        }
    }
    crate::noise::hash64(best.1)
}

/// Picks an entry by weight with a uniform `u` in 0..1.
pub(crate) fn pick<T: Copy>(items: &[(T, u32)], u: f32) -> T {
    let total: u32 = items.iter().map(|i| i.1).sum();
    let mut target = (u.clamp(0.0, 0.999_999) * total as f32) as u32;
    for &(item, w) in items {
        if target < w {
            return item;
        }
        target -= w;
    }
    items[items.len() - 1].0
}

/// Uniform 0..1 from the high bits of a hash.
pub(crate) fn hash_unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// How wooded a biome is at a column, 0 (open) .. 1 (under a closed canopy): forests have
/// clearings, open land has copses. Trees use it for their density and the ground for forest
/// floor versus grass, so both agree.
pub(crate) fn woodland(seed: u64, x: i32, z: i32, biome: Biome) -> f32 {
    let (fx, fz) = (x as f64, z as f64);
    let n = fbm2(seed ^ salt::WOOD, fx / 110.0, fz / 110.0, 3) as f32
        + 0.25 * value2(seed ^ salt::WOOD ^ 1, fx / 23.0, fz / 23.0) as f32;
    let (lo, hi) = match biome {
        // Mostly closed canopy with glades.
        Biome::Forest | Biome::BirchForest | Biome::Taiga => (-0.62, -0.32),
        Biome::PineForest => (-0.5, -0.2),
        Biome::Swamp => (-0.45, -0.1),
        // Open land with copses and hedgerow clumps.
        Biome::Plains => (0.3, 0.5),
        Biome::Meadow => (0.45, 0.62),
        Biome::Highlands => (0.4, 0.6),
        Biome::Mountains => (0.0, 0.35),
        Biome::Marsh | Biome::AlpineMeadow => (0.55, 0.7),
        Biome::SnowyPeaks | Biome::Beach | Biome::River | Biome::Lake => return 0.0,
    };
    smoothstep(lo, hi, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_are_contiguous_and_varied() {
        let mut same = 0;
        let mut ids = std::collections::HashSet::new();
        for x in 0..200 {
            for z in 0..200 {
                let a = patch(7, 1, x, z, 12.0);
                if patch(7, 1, x + 1, z, 12.0) == a {
                    same += 1;
                }
                ids.insert(a);
            }
        }
        // Neighbours mostly share a patch, and there are many patches.
        assert!(same > 200 * 200 * 3 / 4, "{same}");
        assert!(ids.len() > 150, "{}", ids.len());
    }

    #[test]
    fn pick_follows_weights() {
        let items = [(0, 1), (1, 3)];
        let ones = (0..1000)
            .filter(|i| pick(&items, *i as f32 / 1000.0) == 1)
            .count();
        assert_eq!(ones, 750);
    }

    #[test]
    fn forests_are_mostly_wooded_and_plains_mostly_open() {
        let mean = |b| {
            (0..4000)
                .map(|i| woodland(3, (i % 64) * 23, (i / 64) * 29, b))
                .sum::<f32>()
                / 4000.0
        };
        assert!(mean(Biome::Forest) > 0.75, "{}", mean(Biome::Forest));
        assert!(mean(Biome::Plains) < 0.3, "{}", mean(Biome::Plains));
    }
}
