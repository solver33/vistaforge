//! Rivers: traced down the macro drainage network, smoothed and given meanders, then cut
//! into the ground column by column with a channel, a floodplain and valley sides.
//!
//! Each river's water surface only ever goes down from source to mouth, and stays just below
//! the ground across its channel and banks, so a river never runs on a raised bank down a
//! slope: where the ground beside it falls away, the river cuts down instead. Water blocks
//! fill whole cells, so the surface drops in 1-block steps (small rapids) where it crosses a
//! whole number. Banks next to the water are kept at least as high as the water surface.

use super::grid::{par_map, smoothstep};
use super::hydro::{Flood, Water, NONE};
use super::noise::{perlin, perlin1};

/// A river course before smoothing, in block coordinates.
#[derive(Clone, Debug, Default)]
pub struct Trace {
    pub pts: Vec<(f64, f64)>,
    /// Water surface height (the integer part is the surface Y).
    pub surf: Vec<f64>,
    pub half: Vec<f64>,
    pub depth: Vec<f64>,
    /// The trace this one flows into (its end is moved onto that river's final course).
    pub joins: Option<usize>,
    /// Meander strength, 0..1.
    pub meander: f64,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    a: (f32, f32),
    b: (f32, f32),
    /// Water surface at each end.
    sa: f32,
    sb: f32,
    /// Level the banks follow: the water before it was stepped into pools and falls.
    ba: f32,
    bb: f32,
    wa: f32,
    wb: f32,
    da: f32,
    db: f32,
    reach: f32,
    /// The water falls from `sa` to `sb` here, over a lip at `a`.
    fall: bool,
    /// Extra depth of the plunge pool below a fall.
    plunge: f32,
}

/// The ground along a course, from [`fit`].
#[derive(Clone, Copy, Debug)]
struct Ground {
    /// Highest the water may stand: just below the lowest ground across channel and banks.
    low: f64,
    /// Highest ground at the edge of the usual valley reach on either side.
    high: f64,
    /// Level the water should be raised to (wetland pools), if the banks allow it.
    lift: f64,
}

/// What the rivers around a column do to it.
#[derive(Clone, Copy, Debug)]
pub struct Cut {
    /// Highest the ground may be (channel, floodplain and valley sides).
    pub cut: f32,
    /// Water surface of the channel the column is in (`None` outside channels).
    pub channel: Option<i32>,
    /// Water surface of a channel the column borders; the bank must reach it.
    pub bank: Option<i32>,
    /// Distance from the nearest water's edge (negative inside a channel).
    pub edge: f32,
    /// Water surface above the lip of a waterfall the column's channel falls from.
    pub fall: Option<i32>,
    /// 0..1: how fully the valley applies; it fades out at the edge of its reach, so the
    /// valley side never ends in a step.
    pub strength: f32,
}

impl Cut {
    /// The ground height `h` after the rivers have shaped it, with valley sides made uneven
    /// by `jitter` (about ±1) and a rounded edge where the valley side meets the land.
    pub fn apply_rough(&self, h: f32, jitter: f32) -> f32 {
        let limit = self.cut + jitter * (0.3 * self.edge).clamp(0.0, 4.0);
        let k = 2.5;
        let t = (k - (h - limit).abs()).max(0.0) / k;
        let v = h.min(limit) - t * t * k * 0.25;
        let v = h + (v - h) * self.strength;
        match (self.channel, self.bank) {
            (None, Some(level)) => v.max(level as f32),
            _ => v,
        }
    }
}

pub struct Rivers {
    seed: u64,
    segs: Vec<Segment>,
    origin: f64,
    bucket: f64,
    nb: usize,
    offsets: Vec<u32>,
    items: Vec<u32>,
}

/// Floodplain width beside a channel of half-width `w`.
#[inline]
fn floodplain(w: f32) -> f32 {
    2.0 + 0.8 * w
}

/// Ground height a river allows at distance `d` from its centre line. The channel is cut
/// below the whole-block water surface `level`; the floodplain and valley sides follow the
/// smooth `surface`, so they do not step down with the water.
///
/// `shape` is the local look of the river, from noise: `bank` 0..1 turns gentle point bars
/// into steeper cut banks, and `bar` 0..1 raises the bed towards the edges into shallow gravel
/// bars, which stay under the water.
#[inline]
fn profile(d: f32, w: f32, depth: f32, level: f32, surface: f32, shape: Shape) -> f32 {
    if d < w {
        let t = d / w;
        let bed = level - 0.3 - (depth - 0.3).max(0.0) * (1.0 - t * t);
        // Bars stay just under the water: a dry bar beside a step in the surface would let
        // the higher water out.
        (bed + shape.bar * t * (depth + 0.6)).min(level - 0.2)
    } else {
        let e = d - w;
        let fp = floodplain(w);
        let wall = (e - fp).max(0.0);
        let steep = (e - fp - 8.0).max(0.0);
        let rise = 0.12 + 0.55 * shape.bank;
        surface.max(level)
            + 0.05
            + 0.3 * shape.bank
            + rise * e.min(fp)
            + 0.9 * wall
            + 0.12 * steep * steep
    }
}

/// Local river look at a column (see [`profile`]).
#[derive(Clone, Copy, Debug, Default)]
struct Shape {
    /// Width multiplier: pools and narrows along the river.
    width: f32,
    bank: f32,
    bar: f32,
}

impl Shape {
    fn at(seed: u64, x: f32, z: f32) -> Self {
        let (x, z) = (x as f64, z as f64);
        let n = |k: u64, s: f64| perlin(seed ^ k, x / s, z / s) as f32;
        Self {
            width: (1.0 + 0.5 * n(0x31, 37.0) + 0.2 * n(0x32, 11.0)).clamp(0.5, 1.7),
            bank: (0.5 + 0.6 * n(0x33, 17.0)).clamp(0.0, 1.0),
            bar: ((n(0x34, 11.0) - 0.25) * 1.4).clamp(0.0, 0.6),
        }
    }
}

/// Chaikin corner cutting with fixed ends; attributes are interpolated alongside.
fn chaikin(t: &Trace, rounds: usize) -> Trace {
    let mut cur = t.clone();
    for _ in 0..rounds {
        let m = cur.pts.len();
        if m < 3 {
            break;
        }
        let mut next = Trace {
            joins: cur.joins,
            meander: cur.meander,
            ..Trace::default()
        };
        let push = |nx: &mut Trace, i: usize, j: usize, f: f64| {
            let l = |a: f64, b: f64| a + (b - a) * f;
            nx.pts
                .push((l(cur.pts[i].0, cur.pts[j].0), l(cur.pts[i].1, cur.pts[j].1)));
            nx.surf.push(l(cur.surf[i], cur.surf[j]));
            nx.half.push(l(cur.half[i], cur.half[j]));
            nx.depth.push(l(cur.depth[i], cur.depth[j]));
        };
        push(&mut next, 0, 0, 0.0);
        for i in 0..m - 1 {
            if i > 0 {
                push(&mut next, i, i + 1, 0.25);
            }
            if i + 1 < m - 1 {
                push(&mut next, i, i + 1, 0.75);
            }
        }
        push(&mut next, m - 1, m - 1, 0.0);
        cur = next;
    }
    cur
}

/// Pushes points sideways along a smooth noise, more on flat ground and for wide rivers,
/// never near the ends.
fn meander(t: &mut Trace, seed: u64) {
    let m = t.pts.len();
    if m < 5 || t.meander <= 0.0 {
        return;
    }
    let mut s = vec![0.0; m];
    for i in 1..m {
        let (a, b) = (t.pts[i - 1], t.pts[i]);
        s[i] = s[i - 1] + ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    }
    let total = s[m - 1];
    let orig = t.pts.clone();
    for i in 1..m - 1 {
        let (p, q) = (orig[i - 1], orig[i + 1]);
        let (tx, tz) = (q.0 - p.0, q.1 - p.1);
        let len = (tx * tx + tz * tz).sqrt();
        if len < 1e-6 {
            continue;
        }
        let (nx, nz) = (-tz / len, tx / len);
        // Local gradient of the water surface over about ±16 blocks.
        let (mut lo, mut hi) = (i, i);
        while lo > 0 && s[i] - s[lo] < 16.0 {
            lo -= 1;
        }
        while hi < m - 1 && s[hi] - s[i] < 16.0 {
            hi += 1;
        }
        let grad = (t.surf[lo] - t.surf[hi]) / (s[hi] - s[lo]).max(1.0);
        let flat = 1.0 - smoothstep(0.02, 0.12, grad);
        let w = t.half[i];
        let taper = smoothstep(0.0, 20.0, s[i]) * smoothstep(0.0, 20.0, total - s[i]);
        let amp = (1.3 * w + 2.5).min(10.0) * (0.4 + 0.6 * flat) * taper * t.meander;
        let wave = 26.0 + 6.0 * w;
        let off = amp * perlin1(seed, s[i] / wave);
        t.pts[i] = (orig[i].0 + nx * off, orig[i].1 + nz * off);
    }
}

/// Closest point of a polyline to `p`: (segment index, fraction along it, distance).
fn closest(t: &Trace, p: (f64, f64)) -> (usize, f64, f64) {
    let mut best = (0, 0.0, f64::MAX);
    for i in 0..t.pts.len().saturating_sub(1) {
        let (a, b) = (t.pts[i], t.pts[i + 1]);
        let (dx, dz) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dz * dz;
        let f = if len2 > 0.0 {
            (((p.0 - a.0) * dx + (p.1 - a.1) * dz) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cz) = (a.0 + dx * f, a.1 + dz * f);
        let d = ((p.0 - cx).powi(2) + (p.1 - cz).powi(2)).sqrt();
        if d < best.2 {
            best = (i, f, d);
        }
    }
    best
}

/// Steps the water of a course into pools and waterfalls where it runs steeply: each pool
/// is held at the level its tail end allows, so it sinks into the slope towards its head,
/// where the pool above falls into it. Very steep ground falls in one drop. Returns the
/// water levels and, per point, whether the water falls onto it from the point before.
fn steps(pts: &[(f64, f64)], f: &[f64], seed: u64) -> (Vec<f64>, Vec<bool>) {
    let m = f.len();
    let mut s = vec![0.0; m];
    for i in 1..m {
        let (a, b) = (pts[i - 1], pts[i]);
        s[i] = s[i - 1] + (b.0 - a.0).hypot(b.1 - a.1);
    }
    let mut out = f.to_vec();
    let mut falls = vec![false; m];
    let mut i = 1;
    while i + 1 < m {
        // Slope of the water over about ±6 blocks.
        let (lo, hi) = (
            (0..i).rev().find(|&k| s[i] - s[k] >= 6.0).unwrap_or(0),
            (i..m).find(|&k| s[k] - s[i] >= 6.0).unwrap_or(m - 1),
        );
        let slope = (f[lo] - f[hi]) / (s[hi] - s[lo]).max(1.0);
        if slope < 0.08 {
            i += 1;
            continue;
        }
        let vary = 0.7 + 0.6 * (0.5 + 0.5 * perlin1(seed, s[i] / 40.0));
        let h = ((2.0 + 6.0 * slope.min(1.0)) * vary).max(2.0);
        let mut j = i;
        while j + 1 < m && f[i] - f[j] < h {
            j += 1;
        }
        if f[i] - f[j] < h {
            break;
        }
        while j + 1 < m && f[j] - f[j + 1] > 1.5 * (s[j + 1] - s[j]) {
            j += 1;
        }
        out[i..j].fill(f[j]);
        falls[i] = out[i - 1].floor() - out[i].floor() >= 2.0;
        i = j;
    }
    (out, falls)
}

/// The ground along a course: at each point, across the channel and its banks (at their
/// widest) and at the edges of the usual valley reach.
fn fit(
    t: &Trace,
    ground: &(dyn Fn(f64, f64) -> f64 + Sync),
    lift: &(dyn Fn(f64, f64) -> f64 + Sync),
) -> Vec<Ground> {
    let m = t.pts.len();
    (0..m)
        .map(|i| {
            let (p, q) = (t.pts[i.saturating_sub(1)], t.pts[(i + 1).min(m - 1)]);
            let (tx, tz) = (q.0 - p.0, q.1 - p.1);
            let len = tx.hypot(tz).max(1e-6);
            let (nx, nz) = (-tz / len, tx / len);
            let o = t.pts[i];
            let at = |d: f64| ground(o.0 + nx * d, o.1 + nz * d);
            let w = 1.7 * t.half[i];
            let bank = w + 1.5;
            let low = [0.0, 0.5 * w, -0.5 * w, bank, -bank]
                .iter()
                .map(|&d| at(d))
                .fold(f64::MAX, f64::min);
            let far = w + floodplain(w as f32) as f64 + 24.0;
            Ground {
                low: low - 0.25,
                high: at(far).max(at(-far)),
                lift: lift(o.0, o.1),
            }
        })
        .collect()
}

impl Rivers {
    /// Smooths and joins the traces, fits their water to the `ground` (the land before any
    /// water shapes it), and indexes their segments for column lookups. Traces must come
    /// before every trace that joins them.
    pub fn build(
        traces: Vec<Trace>,
        seed: u64,
        size: i32,
        ground: &(dyn Fn(f64, f64) -> f64 + Sync),
        lift: &(dyn Fn(f64, f64) -> f64 + Sync),
    ) -> Self {
        // Smooth courses with meanders; ends that join a river move onto its course.
        let mut done: Vec<Trace> = Vec::with_capacity(traces.len());
        let mut joins: Vec<Option<(usize, usize, f64)>> = Vec::with_capacity(traces.len());
        for (k, t) in traces.into_iter().enumerate() {
            let mut t = chaikin(&t, 3);
            meander(&mut t, seed ^ (k as u64).wrapping_mul(0x51ED));
            let mut join = None;
            if let Some(target) = t.joins.filter(|&j| j < done.len()) {
                let main = &done[target];
                let last = t.pts.len() - 1;
                let (i, f, _) = closest(main, t.pts[last]);
                let j = (i + 1).min(main.pts.len() - 1);
                let l = |a: f64, b: f64| a + (b - a) * f;
                t.pts[last] = (
                    l(main.pts[i].0, main.pts[j].0),
                    l(main.pts[i].1, main.pts[j].1),
                );
                join = Some((target, i, f));
            }
            done.push(t);
            joins.push(join);
        }
        let grounds: Vec<Vec<Ground>> = par_map(&done, |t| fit(t, ground, lift));

        // Water levels, main rivers first: never rising downstream, below the ground beside
        // the channel, and never below the water the river ends in.
        let mut banks: Vec<Vec<f64>> = Vec::with_capacity(done.len());
        let mut lips: Vec<Vec<bool>> = Vec::with_capacity(done.len());
        let edge = |p: (f64, f64)| p.0 < 0.0 || p.1 < 0.0 || p.0 > size as f64 || p.1 > size as f64;
        for k in 0..done.len() {
            if let Some((target, i, f)) = joins[k] {
                let main = &done[target].surf;
                let j = (i + 1).min(main.len() - 1);
                let v = main[i] + (main[j] - main[i]) * f;
                *done[k].surf.last_mut().unwrap() = v;
            }
            let t = &mut done[k];
            let m = t.surf.len();
            let floor = if edge(t.pts[m - 1]) {
                f64::MIN
            } else {
                t.surf[m - 1]
            };
            let mut prev = f64::MAX;
            for (i, g) in grounds[k].iter().enumerate() {
                let v = t.surf[i].min(prev).min(g.low).max(floor);
                // Raised to the level of wetland pools where the banks allow it.
                let v = v.max(g.lift.min(g.low).min(prev));
                t.surf[i] = v;
                prev = v;
            }
            let fitted = std::mem::take(&mut t.surf);
            let (surf, falls) = steps(&t.pts, &fitted, seed ^ (k as u64).wrapping_mul(0xFA11));
            t.surf = surf;
            banks.push(fitted);
            lips.push(falls);
        }

        let mut segs = Vec::new();
        for (k, (t, g)) in done.iter().zip(&grounds).enumerate() {
            let (bank, fall) = (&banks[k], &lips[k]);
            let m = t.pts.len();
            for i in 0..m.saturating_sub(1) {
                let (wa, wb) = (t.half[i] as f32, t.half[i + 1] as f32);
                // Widest the channel gets where it pools.
                let w = wa.max(wb) * 1.7;
                let fp = floodplain(w);
                // The valley side must climb out of the ground beside it before it ends.
                let climb = (g[i].high.max(g[i + 1].high) - bank[i + 1]) as f32 - 0.12 * fp;
                let side = if climb <= 7.2 {
                    climb / 0.9
                } else {
                    8.0 + (-0.9 + (0.81 + 0.48 * (climb - 7.2)).sqrt()) / 0.24
                };
                // Plunge pools: deeper water for a few blocks below each fall.
                let plunge = (i.saturating_sub(4)..=i + 1)
                    .filter(|&j| j > 0 && j < m && fall[j])
                    .map(|j| (1.0 + 0.35 * (t.surf[j - 1] - t.surf[j])).min(5.0))
                    .fold(0.0, f64::max);
                segs.push(Segment {
                    a: (t.pts[i].0 as f32, t.pts[i].1 as f32),
                    b: (t.pts[i + 1].0 as f32, t.pts[i + 1].1 as f32),
                    sa: t.surf[i] as f32,
                    sb: t.surf[i + 1] as f32,
                    ba: bank[i] as f32,
                    bb: bank[i + 1] as f32,
                    wa,
                    wb,
                    da: t.depth[i] as f32,
                    db: t.depth[i + 1] as f32,
                    reach: w + fp + (side + 6.0).clamp(24.0, 96.0),
                    fall: fall[i + 1],
                    plunge: plunge as f32,
                });
            }
        }
        let bucket = 16.0;
        let origin = -64.0;
        let nb = ((size as f64 + 128.0) / bucket).ceil() as usize + 1;
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nb * nb];
        for (k, s) in segs.iter().enumerate() {
            let x0 = s.a.0.min(s.b.0) - s.reach;
            let x1 = s.a.0.max(s.b.0) + s.reach;
            let z0 = s.a.1.min(s.b.1) - s.reach;
            let z1 = s.a.1.max(s.b.1) + s.reach;
            let cell =
                |v: f32| (((v as f64 - origin) / bucket).floor().max(0.0) as usize).min(nb - 1);
            for bz in cell(z0)..=cell(z1) {
                for bx in cell(x0)..=cell(x1) {
                    lists[bz * nb + bx].push(k as u32);
                }
            }
        }
        let mut offsets = Vec::with_capacity(nb * nb + 1);
        let mut items = Vec::new();
        offsets.push(0);
        for l in lists {
            items.extend(l);
            offsets.push(items.len() as u32);
        }
        Self {
            seed,
            segs,
            origin,
            bucket,
            nb,
            offsets,
            items,
        }
    }

    /// How the river valleys shape the column at (x, z), if any river is near.
    pub fn at(&self, x: f64, z: f64) -> Option<Cut> {
        let (bx, bz) = (
            ((x - self.origin) / self.bucket).floor(),
            ((z - self.origin) / self.bucket).floor(),
        );
        if bx < 0.0 || bz < 0.0 || bx >= self.nb as f64 || bz >= self.nb as f64 {
            return None;
        }
        let b = bz as usize * self.nb + bx as usize;
        let list = &self.items[self.offsets[b] as usize..self.offsets[b + 1] as usize];
        let (px, pz) = (x as f32, z as f32);
        let mut cut = f32::MAX;
        // Channel the column sits in: (d / w, surface).
        let mut channel: Option<(f32, i32)> = None;
        // Highest water surface whose bank the column is on.
        let mut bank: Option<i32> = None;
        let mut edge = f32::MAX;
        let mut strength = 1.0f32;
        let mut fall = None;
        let mut shape: Option<Shape> = None;
        for &k in list {
            let s = &self.segs[k as usize];
            let (dx, dz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let len2 = dx * dx + dz * dz;
            let f = if len2 > 0.0 {
                (((px - s.a.0) * dx + (pz - s.a.1) * dz) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (cx, cz) = (s.a.0 + dx * f, s.a.1 + dz * f);
            let d = ((px - cx).powi(2) + (pz - cz).powi(2)).sqrt();
            if d > s.reach {
                continue;
            }
            let sh = *shape.get_or_insert_with(|| Shape::at(self.seed, px, pz));
            let w = (s.wa + (s.wb - s.wa) * f) * sh.width;
            // Below a lip the water is already at the pool's level: the fall is vertical.
            let smooth = if s.fall {
                s.sb
            } else {
                s.sa + (s.sb - s.sa) * f
            };
            let surface = smooth.floor();
            let banks = s.ba + (s.bb - s.ba) * f;
            let depth = s.da + (s.db - s.da) * f + s.plunge;
            let c = profile(d, w, depth, surface, banks.max(smooth), sh);
            if c < cut {
                cut = c;
                strength = 1.0 - smoothstep(s.reach as f64 - 12.0, s.reach as f64, d as f64) as f32;
            }
            edge = edge.min(d - w);
            if d < w {
                let r = d / w;
                if channel.is_none_or(|c| r < c.0) {
                    channel = Some((r, surface as i32));
                    fall = s.fall.then_some(s.sa.floor() as i32);
                }
            } else if d - w < 1.5 {
                // Hold the water a block or two along the river too, where the surface
                // steps down beside the bank; beside a fall, the water above the lip.
                let level = if s.fall {
                    s.sa.floor() as i32
                } else {
                    let ahead = (s.sa - s.sb).abs() / len2.sqrt().max(1.0) * 2.0;
                    (smooth + ahead).floor() as i32
                };
                bank = Some(bank.map_or(level, |b| b.max(level)));
            }
        }
        (edge < f32::MAX).then_some(Cut {
            cut,
            channel: channel.map(|c| c.1),
            bank,
            edge,
            fall,
            strength,
        })
    }
}

/// River width and depth for a node draining `area` (in the units of `a_min`).
pub fn size_for(area: f32, a_min: f32, width: f32) -> (f64, f64) {
    let r = (area / a_min).max(1.0).sqrt() as f64 - 1.0;
    let half = (0.7 + 0.75 * r * width as f64).min(2.5 + 9.0 * width as f64);
    let depth = (0.8 + 0.32 * half).min(5.0);
    (half, depth)
}

/// Inputs for tracing rivers on the macro grid.
pub struct Network<'a> {
    pub n: usize,
    pub cell: f64,
    pub flood: &'a Flood,
    pub area: &'a [f32],
    pub water: &'a Water,
    pub sea_surface: f32,
    /// Smallest drained area (in nodes) that makes a river.
    pub a_min: f32,
    /// Width multiplier (0..1).
    pub width: f32,
    /// Hand-drawn river each node belongs to, if any (natural rivers end on those).
    pub drawn: &'a [u32],
    /// Number of hand-drawn traces that come first in the list.
    pub drawn_count: usize,
}

/// Traces natural rivers down the drainage network. Returned traces are numbered after
/// the `drawn_count` hand-drawn ones, and each comes after the trace it joins.
pub fn trace(net: &Network) -> Vec<Trace> {
    let n = net.n;
    let total = n * n;
    let f = net.flood;
    let w = net.water;
    let river = |k: usize| {
        net.area[k] >= net.a_min && w.lake[k] == NONE && !w.sea[k] && net.drawn[k] == NONE
    };
    let size = |k: usize| size_for(net.area[k], net.a_min, net.width);

    // Water surface per node: under the valley floor, never rising downstream.
    let mut surf = vec![0.0f64; total];
    for &k in &f.order {
        let k = k as usize;
        surf[k] = if w.lake[k] != NONE {
            w.surface[w.lake[k] as usize] as f64 + 0.01
        } else if w.sea[k] {
            net.sea_surface as f64 + 0.01
        } else {
            let (half, _) = size(k);
            let own = f.filled[k] as f64 - 0.6 - 0.12 * half;
            match f.recv[k] {
                NONE => own,
                r => own.max(surf[r as usize]),
            }
        };
    }

    let mut donors = vec![0u8; total];
    let mut lake_donor = vec![NONE; total];
    for k in 0..total {
        let r = f.recv[k];
        if r == NONE {
            continue;
        }
        if river(k) {
            donors[r as usize] = donors[r as usize].saturating_add(1);
        } else if w.lake[k] != NONE {
            let cur = lake_donor[r as usize];
            if cur == NONE || net.area[k] > net.area[cur as usize] {
                lake_donor[r as usize] = k as u32;
            }
        }
    }

    let pos = |k: usize| ((k % n) as f64 * net.cell, (k / n) as f64 * net.cell);
    let mut owner = vec![NONE; total];
    let mut traces: Vec<Trace> = Vec::new();
    // Heads in flood order: the network is walked from sources the same way every time.
    let heads: Vec<usize> = f
        .order
        .iter()
        .map(|&k| k as usize)
        .filter(|&k| river(k) && donors[k] == 0)
        .collect();
    for head in heads {
        let id = (net.drawn_count + traces.len()) as u32;
        let mut t = Trace {
            meander: 1.0,
            ..Trace::default()
        };
        let add = |t: &mut Trace, k: usize| {
            // Lake and sea ends take the size of the river next to them.
            let (half, depth) = if river(k) {
                size(k)
            } else {
                match (t.half.last(), t.depth.last()) {
                    (Some(&h), Some(&d)) => (h, d),
                    _ => size(head),
                }
            };
            t.pts.push(pos(k));
            t.surf.push(surf[k]);
            t.half.push(half);
            t.depth.push(depth);
        };
        if lake_donor[head] != NONE {
            add(&mut t, lake_donor[head] as usize);
        }
        let mut k = head;
        loop {
            add(&mut t, k);
            owner[k] = id;
            let r = f.recv[k];
            if r == NONE {
                // Leaves the map: run on past the edge.
                let m = t.pts.len();
                if m >= 2 {
                    let (a, b) = (t.pts[m - 2], t.pts[m - 1]);
                    let (dx, dz) = (b.0 - a.0, b.1 - a.1);
                    let len = (dx * dx + dz * dz).sqrt().max(1e-6);
                    let ext = 3.0 * net.cell;
                    t.pts.push((b.0 + dx / len * ext, b.1 + dz / len * ext));
                    let last = t.surf[m - 1];
                    t.surf.push(last);
                    let (hw, dp) = (t.half[m - 1], t.depth[m - 1]);
                    t.half.push(hw);
                    t.depth.push(dp);
                }
                break;
            }
            let r = r as usize;
            if !river(r) {
                if net.drawn[r] != NONE {
                    t.joins = Some(net.drawn[r] as usize);
                }
                add(&mut t, r);
                break;
            }
            if owner[r] != NONE {
                t.joins = Some(owner[r] as usize);
                add(&mut t, r);
                break;
            }
            k = r;
        }
        if t.pts.len() >= 2 {
            traces.push(t);
        } else {
            // A lone node: let a later trace through it own it.
            owner[head] = NONE;
        }
    }
    traces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(len: usize, surf0: f64) -> Trace {
        Trace {
            pts: (0..len).map(|i| (i as f64 * 8.0, 100.0)).collect(),
            surf: (0..len).map(|i| surf0 - i as f64 * 0.2).collect(),
            half: vec![3.0; len],
            depth: vec![2.0; len],
            joins: None,
            meander: 1.0,
        }
    }

    #[test]
    fn carves_a_channel_with_dry_banks() {
        let r = Rivers::build(vec![straight(30, 70.5)], 3, 256, &|_, _| 100.0, &|_, _| {
            f64::MIN
        });
        // Centre of the channel: under water.
        let hit = r.at(80.0, 100.0).unwrap();
        let h = hit.apply_rough(75.0, 0.0);
        let level = hit.channel.expect("in channel");
        assert!(h < level as f32 - 0.5, "{h} {level}");
        // Far away: untouched.
        assert!(r
            .at(80.0, 180.0)
            .is_none_or(|c| c.apply_rough(75.0, 0.0) == 75.0));
        // A dip right beside the water is raised to the water surface.
        let mut found_bank = false;
        for dz in 0..12 {
            if let Some(hit) = r.at(80.0, 100.0 + dz as f64) {
                let b = hit.apply_rough(60.0, 0.0);
                if hit.channel.is_none() && hit.edge < 1.5 && hit.edge >= 0.0 {
                    assert!(b >= 65.0, "{b}");
                    found_bank = true;
                }
            }
        }
        assert!(found_bank);
    }

    #[test]
    fn surface_never_rises_downstream() {
        let mut t = straight(20, 70.0);
        t.surf[10] = 90.0; // a bump the water would have to climb
        let r = Rivers::build(vec![t], 1, 256, &|_, _| 100.0, &|_, _| f64::MIN);
        for s in &r.segs {
            assert!(s.sb <= s.sa + 1e-4);
        }
    }

    #[test]
    fn tributary_end_lands_on_the_main_river() {
        let main = straight(30, 70.0);
        let trib = Trace {
            pts: vec![(80.0, 20.0), (80.0, 60.0), (80.0, 100.0)],
            surf: vec![80.0, 75.0, 72.0],
            half: vec![1.0; 3],
            depth: vec![1.0; 3],
            joins: Some(0),
            meander: 0.0,
        };
        let r = Rivers::build(vec![main, trib], 1, 256, &|_, _| 100.0, &|_, _| f64::MIN);
        let last = r.segs.last().unwrap();
        let hit = r.at(last.b.0 as f64, last.b.1 as f64).unwrap();
        assert!(hit.channel.is_some());
    }
}
