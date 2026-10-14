//! Roads and waterfalls drawn by hand ([`ShapeEdit::Road`], [`ShapeEdit::Waterfall`]).
//!
//! They are laid over the finished land column by column, after rivers and lakes, in the order
//! they were drawn: each one is fitted to the ground that the land and the brushes before it
//! left. Fitting happens once, in [`Brushes::build`]; [`Brushes::apply`] is then cheap and
//! pure, like the rest of [`Terrain::sample`].
//!
//! - [`road`]: a road follows the ground smoothed along its course, cut into rises and laid
//!   over dips, with a verge on each side that blends into the land around. Columns on it get
//!   [`ColumnInfo::road`], which `surface` and `decor` read.
//! - [`waterfall`]: a stream from a spring runs to a cliff and falls `drop` blocks into a
//!   pool. The water is ordinary river water, and the curtain is the terrain's usual
//!   [`ColumnInfo::fall`].
//!
//! [`ColumnInfo::road`]: crate::column::ColumnInfo::road
//! [`ColumnInfo::fall`]: crate::column::ColumnInfo::fall

mod road;
mod waterfall;

#[cfg(test)]
mod tests;

use crate::column::WaterKind;
use crate::settings::ShapeEdit;

use super::{Sample, Terrain};

use road::Road;
use waterfall::Waterfall;

/// Side of the buckets brushes are indexed by, in blocks.
const BUCKET: f64 = 32.0;
/// The index covers this far around the map, for the margins pieces are generated with.
const PAD: f64 = 64.0;

enum Brush {
    Road(Road),
    Waterfall(Waterfall),
}

impl Brush {
    /// The columns the brush may change: (x0, z0, x1, z1).
    fn bounds(&self) -> (f64, f64, f64, f64) {
        match self {
            Brush::Road(r) => r.bounds(),
            Brush::Waterfall(w) => w.bounds(),
        }
    }

    fn apply(&self, t: &Terrain, x: i32, z: i32, s: &mut Sample) {
        match self {
            Brush::Road(r) => r.apply(t, x, z, s),
            Brush::Waterfall(w) => w.apply(t, x, z, s),
        }
    }
}

#[derive(Default)]
pub struct Brushes {
    list: Vec<Brush>,
    /// Buckets per side, and for each bucket the brushes reaching into it, in drawing order.
    nb: usize,
    buckets: Vec<Vec<u16>>,
}

impl Brushes {
    /// Fits every road and waterfall in `edits` to the land of `t`.
    pub fn build(t: &Terrain, edits: &[ShapeEdit], seed: u64) -> Self {
        let nb = ((t.size as f64 + 2.0 * PAD) / BUCKET).ceil() as usize + 1;
        let mut out = Brushes {
            list: Vec::new(),
            nb,
            buckets: vec![Vec::new(); nb * nb],
        };
        for (k, edit) in edits.iter().enumerate() {
            let s = seed ^ (k as u64 + 1).wrapping_mul(0xB2B5_11E5);
            let ground = |x: i32, z: i32| out.ground(t, x, z);
            let brush = match edit {
                ShapeEdit::Road { points, width } => {
                    Road::build(points, *width as f64, s, &ground).map(Brush::Road)
                }
                ShapeEdit::Waterfall {
                    x,
                    z,
                    radius,
                    drop,
                    angle,
                } => Some(Brush::Waterfall(Waterfall::build(
                    (*x as f64, *z as f64),
                    *radius as f64,
                    *drop as f64,
                    *angle as f64,
                    s,
                    t,
                    &ground,
                ))),
                _ => None,
            };
            if let Some(b) = brush {
                out.push(b);
            }
        }
        out
    }

    fn push(&mut self, b: Brush) {
        if self.list.len() >= u16::MAX as usize {
            return;
        }
        let (x0, z0, x1, z1) = b.bounds();
        let k = self.list.len() as u16;
        let (lo0, lo1) = (self.cell(x0), self.cell(z0));
        let (hi0, hi1) = (self.cell(x1), self.cell(z1));
        for j in lo1..=hi1 {
            for i in lo0..=hi0 {
                self.buckets[j * self.nb + i].push(k);
            }
        }
        self.list.push(b);
    }

    fn cell(&self, v: f64) -> usize {
        (((v + PAD) / BUCKET).floor().max(0.0) as usize).min(self.nb - 1)
    }

    /// A column with the brushes so far applied.
    fn ground(&self, t: &Terrain, x: i32, z: i32) -> Sample {
        let mut s = t.base_sample(x, z);
        self.apply(t, x, z, &mut s);
        s
    }

    /// Lays the brushes reaching (x, z) over its sample, in drawing order.
    pub fn apply(&self, t: &Terrain, x: i32, z: i32, s: &mut Sample) {
        if self.list.is_empty() {
            return;
        }
        let b = self.cell(z as f64) * self.nb + self.cell(x as f64);
        for &k in &self.buckets[b] {
            self.list[k as usize].apply(t, x, z, s);
        }
    }
}

/// Whether a column is in or near water the land made (rivers, lakes, the sea, wetland
/// pools): a brush may raise such ground but not lower it, so it never lets that water out.
fn near_water(t: &Terrain, x: i32, z: i32, s: &Sample) -> bool {
    let (fx, fz) = (x as f64, z as f64);
    s.water != WaterKind::None
        || s.river_edge < 4.0
        || s.swampy > 0.3
        || t.lake_dist.bilinear(fx, fz) < 14.0
        || t.sea_dist.bilinear(fx, fz) < 14.0
}

/// Ground height of a sample for fitting a brush to: the ground, or just above the water.
fn surface_of(s: &Sample) -> f64 {
    match s.level {
        Some(l) => (s.h as f64).max(l as f64 + 1.2),
        None => s.h as f64,
    }
}
