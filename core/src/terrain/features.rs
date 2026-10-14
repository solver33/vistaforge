//! Map-scale landforms the sliders ask for: tiers of high plateaus standing on escarpments,
//! and volcanoes.

use crate::noise::unit2;

use super::grid::smoothstep;
use super::noise::{fbm, perlin, warp};

/// How many plateau levels a map has above its lowlands.
const LEVELS: f64 = 4.0;

/// Map-scale steps from the `plateaus` slider: the land rises in broad levels, each standing
/// above the next on a steep, ragged escarpment. Levels follow the climate's elevation
/// tendency in part, so wet lowland biomes keep to the lower steps, and they come down
/// towards the sea.
pub struct Tiers {
    seed: u64,
    scale: f64,
    /// Height of one step in blocks.
    step: f64,
    /// Share of each step taken by its escarpment.
    face: f64,
}

impl Tiers {
    pub fn new(seed: u64, size: f64, step: f64, face: f64) -> Self {
        Self {
            seed,
            scale: (size * 0.45).clamp(400.0, 1600.0),
            step,
            face,
        }
    }

    /// Height of the steps at (x, z), where the region's elevation tendency is `e` (0..1)
    /// and `inland` runs from 0 at the sea to 1 far from it.
    pub fn steps(&self, x: f64, z: f64, e: f64, inland: f64) -> f64 {
        if self.step <= 0.0 {
            return 0.0;
        }
        let (seed, scale) = (self.seed, self.scale);
        let (wx, wz) = warp(seed ^ 0x7E50, x, z, scale * 0.3, scale * 0.12);
        let noise = 0.5 + 0.75 * fbm(seed ^ 0x7E51, wx / scale, wz / scale, 3, 0.45);
        let t = ((0.6 * noise + 0.4 * e) * inland).clamp(0.0, 1.0);
        let q = t * LEVELS;
        let level = q.floor().min(LEVELS - 1.0);
        // The escarpment takes the end of its step: a talus foot, then a steep face.
        let rise = smoothstep(1.0 - self.face, 1.0, q - level).powf(1.6);
        self.step * (level + rise)
    }

    /// Plateau tops are not dead flat: a gentle roll to add on top of the steps.
    pub fn roll(&self, x: f64, z: f64) -> f64 {
        0.08 * self.step * perlin(self.seed ^ 0x7E52, x / 140.0, z / 140.0)
    }
}

/// One volcano: a cone with concave flanks, a crater and ribs running down its sides.
#[derive(Clone, Copy, Debug)]
pub struct Volcano {
    pub x: f64,
    pub z: f64,
    /// Radius of the base, in blocks.
    pub radius: f64,
    /// Height of the crater rim above the land around it.
    pub height: f64,
}

/// Volcanoes for the `volcanoes` slider: none at 0, one at a little above 0, up to a few on a
/// large map. The first stands where the land is furthest from the sea (the middle of an
/// island), or near the map centre on a landlocked map.
pub fn volcanoes(seed: u64, size: f64, amount: f64, inland: Option<(f64, f64)>) -> Vec<Volcano> {
    if amount <= 0.0 {
        return Vec::new();
    }
    let area = (size / 1024.0).powi(2);
    let count = (1.0 + amount * 2.5 * area.sqrt()).floor().clamp(1.0, 6.0) as usize;
    let u = |i: usize, salt: i64| unit2(seed ^ 0x0F01, i as i64, salt);
    let radius = (size * (0.11 + 0.1 * amount)).clamp(90.0, 700.0);
    let mut out: Vec<Volcano> = Vec::new();
    for i in 0..count * 8 {
        if out.len() == count {
            break;
        }
        let (x, z) = match (i, inland) {
            (0, Some(p)) => p,
            (0, None) => (size * (0.4 + 0.2 * u(0, 1)), size * (0.4 + 0.2 * u(0, 2))),
            _ => (size * (0.15 + 0.7 * u(i, 1)), size * (0.15 + 0.7 * u(i, 2))),
        };
        let r = radius * if i == 0 { 1.0 } else { 0.55 + 0.35 * u(i, 3) };
        // Keep cones apart.
        if out
            .iter()
            .any(|v| (v.x - x).hypot(v.z - z) < 0.9 * (v.radius + r))
        {
            continue;
        }
        out.push(Volcano {
            x,
            z,
            radius: r,
            height: r * (0.55 + 0.25 * u(i, 4)) * (0.7 + 0.6 * amount),
        });
    }
    out
}

impl Volcano {
    /// Distance from the centre as a share of the (ragged) base radius, the bearing, and the
    /// distance in blocks.
    fn reach(&self, seed: u64, x: f64, z: f64) -> (f64, f64, f64) {
        let (dx, dz) = (x - self.x, z - self.z);
        let r = dx.hypot(dz);
        let angle = dz.atan2(dx);
        let ragged = 1.0 + 0.12 * perlin(seed ^ 0x0F02, angle.cos() * 2.0, angle.sin() * 2.0);
        (r / (self.radius * ragged), angle, r)
    }

    /// How far up the cone (x, z) is: 0 at the foot and outside, 1 at the centre.
    pub fn cone(&self, seed: u64, x: f64, z: f64) -> f64 {
        (1.0 - self.reach(seed, x, z).0).max(0.0)
    }

    /// Height the volcano adds at (x, z).
    pub fn at(&self, seed: u64, x: f64, z: f64) -> f64 {
        let (u, angle, r) = self.reach(seed, x, z);
        if u >= 1.0 {
            return 0.0;
        }
        let cone = |u: f64| self.height * (1.0 - u).powf(1.7) / (1.0 - CRATER).powf(1.7);
        let h = if u < CRATER {
            // A bowl below the rim.
            let depth = (0.3 * self.height).min(45.0);
            cone(CRATER) - depth * (1.0 - (u / CRATER).powi(2))
        } else {
            cone(u)
        };
        // Ribs and gullies run down the flanks, strongest halfway down.
        let around = angle * self.radius / 45.0;
        let ribs = perlin(seed ^ 0x0F03, around, r / 70.0);
        let flank = smoothstep(CRATER, 0.45, u) * (1.0 - u);
        h + 0.12 * self.height * flank * ribs
    }
}

/// Crater radius as a share of the base radius.
const CRATER: f64 = 0.13;
