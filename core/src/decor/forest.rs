//! Where trees stand, which species and how big.
//!
//! Candidates sit one per 4×4 cell at a hashed spot. A candidate gives way to any neighbouring
//! candidate closer than [`SPACING2`] with a higher priority; that rule uses hashes only, so a
//! tree's existence never depends on columns other than its own. Each survivor then becomes a
//! tree when a hash falls under the local density: the biome's density between its open land
//! and its groves (the shared woodland field, which also decides forest floor versus grass), times
//! the trees slider. Species come in groves: one species leads in each patch of a few dozen
//! blocks, and the biome's others mix in.

use crate::biome::Biome;
use crate::column::ColumnInfo;
use crate::noise::{hash2, hash64};
use crate::surface::field::{hash_unit, patch, pick, woodland};
use crate::surface::{Surface, Zone};
use crate::trees::{Species, TreeRequest};

/// Side of a candidate cell.
pub(crate) const CELL: i32 = 4;
/// Trunks are never closer than √12 ≈ 3.5 blocks.
const SPACING2: i32 = 12;
/// Share of candidates that survive the spacing rule, on average. Densities are divided by it,
/// so a density is the share of cells that hold a tree, and this is the densest forest there is.
const SURVIVAL: f32 = 0.48;

const SALT: u64 = 0x7EE5_0000;

#[derive(Clone, Copy)]
struct Candidate {
    x: i32,
    z: i32,
    h: u64,
}

fn candidate(seed: u64, gx: i32, gz: i32) -> Candidate {
    let h = hash2(seed ^ SALT, gx as i64, gz as i64);
    Candidate {
        x: gx * CELL + (h % CELL as u64) as i32,
        z: gz * CELL + ((h >> 8) % CELL as u64) as i32,
        h,
    }
}

/// True when no neighbouring candidate with a higher priority is too close.
fn spaced(seed: u64, gx: i32, gz: i32, c: Candidate) -> bool {
    for dx in -1..=1 {
        for dz in -1..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            let n = candidate(seed, gx + dx, gz + dz);
            let d2 = (n.x - c.x).pow(2) + (n.z - c.z).pow(2);
            if d2 < SPACING2 && n.h > c.h {
                return false;
            }
        }
    }
    true
}

struct Stand {
    /// Density on open land and in groves: the share of cells that hold a tree, at most
    /// [`SURVIVAL`] at the default trees slider.
    open: f32,
    grove: f32,
    species: &'static [(Species, u32)],
    /// Typical size in open land and in groves (0..1, see [`TreeRequest::size`]).
    open_size: f32,
    grove_size: f32,
}

fn stand(biome: Biome) -> Stand {
    use Species::*;
    let s = |open, grove, species, open_size, grove_size| Stand {
        open,
        grove,
        species,
        open_size,
        grove_size,
    };
    match biome.fallback() {
        Biome::Plains => s(0.014, 0.45, &[(Oak, 8), (Birch, 1), (Bush, 3)], 0.75, 0.5),
        Biome::Meadow => s(0.008, 0.45, &[(Oak, 4), (Birch, 3), (Bush, 3)], 0.65, 0.45),
        Biome::Forest => s(
            0.06,
            0.38,
            &[(Oak, 10), (Birch, 3), (Bush, 2), (Dead, 1)],
            0.6,
            0.55,
        ),
        Biome::BirchForest => s(
            0.06,
            0.4,
            &[(Birch, 10), (Oak, 2), (Bush, 1), (Dead, 1)],
            0.55,
            0.5,
        ),
        Biome::Taiga => s(
            0.08,
            0.42,
            &[(Spruce, 10), (Pine, 2), (Birch, 2), (Dead, 1)],
            0.55,
            0.55,
        ),
        Biome::PineForest => s(
            0.06,
            0.3,
            &[(Pine, 10), (Spruce, 2), (Birch, 1), (Dead, 1)],
            0.6,
            0.55,
        ),
        Biome::Swamp => s(
            0.1,
            0.38,
            &[(Willow, 8), (Oak, 3), (Dead, 3), (Bush, 2)],
            0.55,
            0.5,
        ),
        Biome::Marsh => s(
            0.01,
            0.2,
            &[(Willow, 3), (Bush, 4), (Dead, 2), (Birch, 1)],
            0.45,
            0.4,
        ),
        Biome::Highlands => s(
            0.005,
            0.35,
            &[(Birch, 3), (Pine, 3), (Bush, 4), (Dead, 1)],
            0.45,
            0.4,
        ),
        Biome::Mountains => s(0.0, 0.4, &[(Spruce, 6), (Pine, 4), (Dead, 1)], 0.45, 0.45),
        Biome::AlpineMeadow => s(0.004, 0.25, &[(Spruce, 5), (Bush, 3), (Pine, 2)], 0.4, 0.4),
        Biome::SnowyPeaks => s(0.0, 0.0, &[(Spruce, 1)], 0.3, 0.3),
        Biome::Beach => s(0.004, 0.0, &[(Pine, 3), (Bush, 2)], 0.45, 0.4),
        Biome::River => s(
            0.04,
            0.0,
            &[(Willow, 5), (Oak, 3), (Birch, 2), (Bush, 2)],
            0.6,
            0.5,
        ),
        Biome::Lake => s(
            0.025,
            0.0,
            &[(Willow, 3), (Oak, 3), (Birch, 2), (Bush, 2)],
            0.6,
            0.5,
        ),
        Biome::Tropical | Biome::SnowyTaiga | Biome::Tundra | Biome::Volcanic => {
            unreachable!("fallback() returns one of the original biomes")
        }
    }
}

pub(crate) struct Forest {
    seed: u64,
    /// The trees slider as a density factor (1 at the default).
    amount: f32,
}

impl Forest {
    pub fn new(seed: u64, trees_slider: f32) -> Self {
        Self {
            seed,
            amount: trees_slider.clamp(0.0, 1.0) * 2.0,
        }
    }

    /// The cells whose candidates can stand in `x0..x1` × `z0..z1`.
    pub fn cells(x0: i32, z0: i32, x1: i32, z1: i32) -> impl Iterator<Item = (i32, i32)> {
        let (gx0, gz0) = (x0.div_euclid(CELL), z0.div_euclid(CELL));
        let (gx1, gz1) = ((x1 - 1).div_euclid(CELL), (z1 - 1).div_euclid(CELL));
        (gx0..=gx1).flat_map(move |gx| (gz0..=gz1).map(move |gz| (gx, gz)))
    }

    /// The tree of a candidate cell, if there is one. `column` gives the terrain of the
    /// candidate's column (`None` when it isn't known; then there is no tree).
    pub fn tree(
        &self,
        surface: &Surface,
        gx: i32,
        gz: i32,
        column: impl FnOnce(i32, i32) -> Option<ColumnInfo>,
    ) -> Option<TreeRequest> {
        let c = candidate(self.seed, gx, gz);
        if self.amount <= 0.0 || !spaced(self.seed, gx, gz, c) {
            return None;
        }
        let col = column(c.x, c.z)?;
        let (biome, zone) = surface.biome_zone(c.x, c.z, &col);
        let st = stand(biome);
        let wood = woodland(self.seed, c.x, c.z, biome);
        let mut density = st.open + (st.grove - st.open) * wood;
        // Willows and the odd dead tree stand in shallow swamp water; nothing else grows wet.
        let swampy = matches!(biome, Biome::Swamp | Biome::Marsh);
        match zone {
            Zone::Rock | Zone::Scree | Zone::Bed(1..) => return None,
            Zone::Bed(_) if !swampy => return None,
            Zone::Bed(_) => density *= 0.3,
            Zone::Stony => density *= 0.4,
            _ => {}
        }
        if col.slope > 1.3 {
            return None;
        }
        let u = hash_unit(hash64(c.h ^ 0xD3));
        if u * SURVIVAL >= density * self.amount {
            return None;
        }
        // Groves: the patch's leading species, or any of the biome's.
        let lead = patch(self.seed, SALT, c.x, c.z, 28.0);
        let mut species = if hash_unit(hash64(c.h ^ 0x5E)) < 0.6 {
            pick(st.species, hash_unit(lead))
        } else {
            pick(st.species, hash_unit(hash64(c.h ^ 0x5F)))
        };
        if matches!(zone, Zone::Bed(_)) && !matches!(species, Species::Willow | Species::Dead) {
            species = Species::Willow;
        }
        // Broadleaf trees don't take steep ground.
        let conifer = matches!(species, Species::Spruce | Species::Pine);
        if !conifer && col.slope > 0.9 {
            return None;
        }
        let typical = st.open_size + (st.grove_size - st.open_size) * wood;
        let jitter = hash_unit(hash64(c.h ^ 0x51)) - 0.5;
        let ancient = hash_unit(hash64(c.h ^ 0x52)) < 0.03;
        let size = match species {
            Species::Bush => 0.2 + 0.3 * hash_unit(hash64(c.h ^ 0x53)),
            _ if ancient => 0.9 + 0.1 * hash_unit(hash64(c.h ^ 0x53)),
            _ => (typical + 0.35 * jitter).clamp(0.15, 0.95),
        };
        let (y, _) = col.ground();
        Some(TreeRequest {
            species,
            x: c.x,
            y,
            z: c.z,
            size,
            seed: hash64(c.h ^ 0x7EE),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_holds_between_any_two_survivors() {
        let mut pts = Vec::new();
        for gx in 0..40 {
            for gz in 0..40 {
                let c = candidate(9, gx, gz);
                if spaced(9, gx, gz, c) {
                    pts.push((c.x, c.z));
                }
            }
        }
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                assert!((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2) >= SPACING2);
            }
        }
        let share = pts.len() as f32 / 1600.0;
        assert!((share - SURVIVAL).abs() < 0.06, "{share}");
    }
}
