//! A road drawn as a polyline. Its course is rounded and made to wander a little, then fitted to
//! the ground: the level along it is the ground across the road, smoothed over a few dozen
//! blocks, so it climbs and dips gently, cut into rises and laid over hollows. A verge on each
//! side blends that level back into the land, and the road surface sits a little lower than
//! its edges, worn in like an old track, with shallow wheel ruts.

use crate::surface::road::rut;
use crate::world::MIN_Y;

use super::super::grid::{lerp, smoothstep};
use super::super::noise::perlin1;
use super::super::{Sample, Terrain};
use super::{near_water, surface_of};
use crate::column::WaterKind;

/// Spacing of course points, in blocks.
const STEP: f64 = 1.0;
/// Side of the buckets the course's segments are indexed by.
const BUCKET: f64 = 16.0;

pub struct Road {
    seed: u64,
    half: f64,
    pts: Vec<(f64, f64)>,
    /// Ground level the road follows, per point.
    level: Vec<f64>,
    /// Bounds of the columns the road changes.
    lo: (f64, f64),
    hi: (f64, f64),
    nbx: usize,
    nbz: usize,
    offsets: Vec<u32>,
    items: Vec<u32>,
}

/// How wide the levelled verge beside a road of half-width `half` is.
fn verge(half: f64) -> f64 {
    (1.5 + 0.45 * half).clamp(2.0, 9.0)
}

/// Chaikin corner cutting with fixed ends.
fn rounded(points: &[(f64, f64)], rounds: usize) -> Vec<(f64, f64)> {
    let mut cur = points.to_vec();
    for _ in 0..rounds {
        if cur.len() < 3 {
            break;
        }
        let mut next = vec![cur[0]];
        for w in cur.windows(2) {
            let (a, b) = (w[0], w[1]);
            next.push((lerp(a.0, b.0, 0.25), lerp(a.1, b.1, 0.25)));
            next.push((lerp(a.0, b.0, 0.75), lerp(a.1, b.1, 0.75)));
        }
        // The first and last cuts sit next to the fixed ends.
        next.remove(1);
        next.pop();
        next.push(*cur.last().unwrap());
        cur = next;
    }
    cur
}

/// Points every `step` blocks along a polyline.
fn resample(points: &[(f64, f64)], step: f64) -> Vec<(f64, f64)> {
    let mut out = vec![points[0]];
    let mut carry = 0.0;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b.0 - a.0).hypot(b.1 - a.1);
        let mut t = step - carry;
        while t <= len {
            let f = t / len;
            out.push((lerp(a.0, b.0, f), lerp(a.1, b.1, f)));
            t += step;
        }
        carry = len - (t - step);
    }
    let last = *points.last().unwrap();
    if out
        .last()
        .is_none_or(|p| (p.0 - last.0).hypot(p.1 - last.1) > 0.3 * step)
    {
        out.push(last);
    }
    out
}

/// Box blur along a sequence, `passes` times (about a Gaussian over `radius`).
fn blur(v: &mut [f64], radius: usize, passes: usize) {
    let m = v.len();
    if m < 3 || radius == 0 {
        return;
    }
    for _ in 0..passes {
        let src = v.to_vec();
        let mut sum: f64 = 0.0;
        // Ends are held: the window is clamped to the sequence.
        let at = |i: i64| src[i.clamp(0, m as i64 - 1) as usize];
        let r = radius as i64;
        for k in -r..=r {
            sum += at(k);
        }
        for i in 0..m as i64 {
            v[i as usize] = sum / (2 * r + 1) as f64;
            sum += at(i + r + 1) - at(i - r);
        }
    }
}

impl Road {
    /// Fits a road along `points` (blocks) of the given width to `ground`. `None` when the
    /// course has no length.
    pub fn build(
        points: &[[f32; 2]],
        width: f64,
        seed: u64,
        ground: &dyn Fn(i32, i32) -> Sample,
    ) -> Option<Self> {
        let half = (width / 2.0).clamp(0.5, 20.0);
        let mut course: Vec<(f64, f64)> = Vec::with_capacity(points.len());
        for p in points {
            let p = (p[0] as f64, p[1] as f64);
            if course
                .last()
                .is_none_or(|q: &(f64, f64)| (q.0 - p.0).hypot(q.1 - p.1) > 0.5)
            {
                course.push(p);
            }
        }
        if course.len() < 2 {
            return None;
        }
        // Corners are rounded over a few blocks, so the road still passes the drawn points.
        let mut pts = resample(&rounded(&resample(&course, 6.0), 2), STEP);
        let m = pts.len();
        if m < 2 {
            return None;
        }

        // Wander a little to the sides, never at the ends, where it meets what it was drawn to.
        let total = (m - 1) as f64 * STEP;
        let amp = (0.4 + 0.12 * half).min(2.0);
        let wave = 22.0 + 2.0 * half;
        let orig = pts.clone();
        for i in 1..m - 1 {
            let (p, q) = (orig[i - 1], orig[i + 1]);
            let (tx, tz) = (q.0 - p.0, q.1 - p.1);
            let len = tx.hypot(tz);
            if len < 1e-6 {
                continue;
            }
            let s = i as f64 * STEP;
            let taper = smoothstep(0.0, 12.0, s) * smoothstep(0.0, 12.0, total - s);
            let off = amp * taper * perlin1(seed ^ 0x40AD, s / wave);
            pts[i] = (orig[i].0 - tz / len * off, orig[i].1 + tx / len * off);
        }

        // The ground across the road, every other point, then smoothed along it.
        let across = |i: usize| {
            let (p, q) = (pts[i.saturating_sub(1)], pts[(i + 1).min(m - 1)]);
            let (tx, tz) = (q.0 - p.0, q.1 - p.1);
            let len = tx.hypot(tz).max(1e-6);
            let (nx, nz) = (-tz / len, tx / len);
            let o = pts[i];
            let at = |d: f64| {
                let s = ground((o.0 + nx * d).round() as i32, (o.1 + nz * d).round() as i32);
                surface_of(&s)
            };
            let side = 0.7 * half;
            0.5 * at(0.0) + 0.25 * at(side) + 0.25 * at(-side)
        };
        let mut level = vec![0.0; m];
        let mut i = 0;
        while i < m {
            level[i] = across(i);
            i += 2;
        }
        for i in (1..m).step_by(2) {
            level[i] = if i + 1 < m {
                0.5 * (level[i - 1] + level[i + 1])
            } else {
                across(i)
            };
        }
        blur(&mut level, (4.0 + 1.2 * half).round() as usize, 3);

        // Segments by bucket.
        let reach = half * 1.12 + verge(half * 1.12) + 1.0;
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for p in &pts {
            lo = (lo.0.min(p.0 - reach), lo.1.min(p.1 - reach));
            hi = (hi.0.max(p.0 + reach), hi.1.max(p.1 + reach));
        }
        let nbx = ((hi.0 - lo.0) / BUCKET).ceil() as usize + 1;
        let nbz = ((hi.1 - lo.1) / BUCKET).ceil() as usize + 1;
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nbx * nbz];
        let cell = |v: f64, o: f64, n: usize| (((v - o) / BUCKET).floor().max(0.0) as usize).min(n - 1);
        for k in 0..m - 1 {
            let (a, b) = (pts[k], pts[k + 1]);
            for bz in cell(a.1.min(b.1) - reach, lo.1, nbz)..=cell(a.1.max(b.1) + reach, lo.1, nbz) {
                for bx in cell(a.0.min(b.0) - reach, lo.0, nbx)..=cell(a.0.max(b.0) + reach, lo.0, nbx)
                {
                    lists[bz * nbx + bx].push(k as u32);
                }
            }
        }
        let mut offsets = Vec::with_capacity(lists.len() + 1);
        let mut items = Vec::new();
        offsets.push(0);
        for l in lists {
            items.extend(l);
            offsets.push(items.len() as u32);
        }
        Some(Self {
            seed,
            half,
            pts,
            level,
            lo,
            hi,
            nbx,
            nbz,
            offsets,
            items,
        })
    }

    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.lo.0, self.lo.1, self.hi.0, self.hi.1)
    }

    /// The nearest point of the course: (distance, distance along the road, level there).
    fn nearest(&self, x: f64, z: f64) -> Option<(f64, f64, f64)> {
        if x < self.lo.0 || z < self.lo.1 || x > self.hi.0 || z > self.hi.1 {
            return None;
        }
        let bx = (((x - self.lo.0) / BUCKET).floor() as usize).min(self.nbx - 1);
        let bz = (((z - self.lo.1) / BUCKET).floor() as usize).min(self.nbz - 1);
        let b = bz * self.nbx + bx;
        let mut best: Option<(f64, f64, f64)> = None;
        for &k in &self.items[self.offsets[b] as usize..self.offsets[b + 1] as usize] {
            let k = k as usize;
            let (a, c) = (self.pts[k], self.pts[k + 1]);
            let (dx, dz) = (c.0 - a.0, c.1 - a.1);
            let len2 = dx * dx + dz * dz;
            let f = if len2 > 0.0 {
                (((x - a.0) * dx + (z - a.1) * dz) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = (x - a.0 - dx * f).hypot(z - a.1 - dz * f);
            if best.is_none_or(|b| d < b.0) {
                let level = lerp(self.level[k], self.level[k + 1], f);
                best = Some((d, (k as f64 + f) * STEP, level));
            }
        }
        best
    }

    pub fn apply(&self, t: &Terrain, x: i32, z: i32, s: &mut Sample) {
        // Water stays as it is: a road crossing a river fords it.
        if s.water != WaterKind::None {
            return;
        }
        let Some((d, along, level)) = self.nearest(x as f64, z as f64) else {
            return;
        };
        // The road narrows and widens a little along its course.
        let half = self.half * (1.0 + 0.1 * perlin1(self.seed ^ 0x40AE, along / 27.0));
        let verge = verge(half);
        if d >= half + verge {
            return;
        }
        let inside = half - d;
        // Worn in: the middle a little below the edges, the wheel ruts lower still.
        let e = (d / half).min(1.0);
        let worn = level - 0.1 + 0.14 * e * e - 0.16 * rut(inside as f32) as f64;
        let w = if d <= half {
            1.0
        } else {
            1.0 - smoothstep(half, half + verge, d)
        };
        let mut h = lerp(s.h as f64, worn, w);
        if near_water(t, x, z, s) {
            h = h.max(s.h as f64);
        }
        s.h = h.clamp(MIN_Y as f64 + 4.0, t.top as f64) as f32;
        let inside = inside as f32;
        s.road = Some(s.road.map_or(inside, |r| r.max(inside)));
    }
}
