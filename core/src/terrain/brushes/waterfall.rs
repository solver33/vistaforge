//! A waterfall placed by hand: a spring feeds a stream that runs to the lip of a cliff and falls
//! `drop` blocks into a round pool scoured out at its foot.
//!
//! In the waterfall's own frame, `s` runs downstream through the lip and `t` across it. The
//! cliff follows the upstream half of the pool, so the fall comes down a horseshoe notch, and
//! runs on to both sides as an escarpment that fades into the land. Above it lies a terrace
//! with the stream; below it a basin with the pool. The stream's water stands at `top` and the
//! pool's at `low = top − drop`, and every dry column beside either is at least as high as its
//! water, so nothing spills. The columns of the pool next to the stream's end carry
//! [`ColumnInfo::fall`] up to `top`: that is the curtain of falling water.
//!
//! The levels are chosen from the ground above and below the lip, so on a slope the fall
//! needs little earthwork, and on flat land half the drop is raised and half dug. The terrace
//! and basin blend into the land over a distance that grows with the drop.
//!
//! [`ColumnInfo::fall`]: crate::column::ColumnInfo::fall

use crate::column::WaterKind;
use crate::world::MIN_Y;

use super::super::grid::{lerp, smoothstep};
use super::super::noise::{fbm, perlin, perlin1};
use super::super::{Sample, Terrain};
use super::{near_water, surface_of};

pub struct Waterfall {
    seed: u64,
    /// The lip, and the flow direction (unit).
    p: (f64, f64),
    u: (f64, f64),
    /// Half-width of the falling water.
    half: f64,
    /// Pool radius; its centre is `pool` blocks downstream of the lip.
    pool: f64,
    /// Length of the stream above the lip, to the spring's centre.
    run: f64,
    spring: f64,
    /// Water levels (Y of the topmost water block) above and below the fall.
    top: i32,
    low: i32,
    /// How far the terrace and basin take to blend into the land.
    fade: f64,
    /// Half the length of the escarpment across the flow.
    scarp: f64,
}

/// What a column is in the waterfall.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// Above the cliff: the stream or spring (`water`), or the terrace.
    Upper { water: bool, edge: f64 },
    /// Below it: the pool (`water`), or the basin.
    Lower { water: bool, edge: f64 },
}

impl Waterfall {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        p: (f64, f64),
        radius: f64,
        drop: f64,
        angle: f64,
        seed: u64,
        t: &Terrain,
        ground: &dyn Fn(i32, i32) -> Sample,
    ) -> Self {
        let a = angle.to_radians();
        let u = (a.cos(), a.sin());
        let half = radius.clamp(1.0, 20.0);
        let drop = drop.round().clamp(3.0, 1500.0);
        let pool = (half + 2.5 + 0.12 * drop).min(half + 14.0);
        let run = (3.0 * half + 12.0 + 0.2 * drop).clamp(14.0, 64.0);
        let spring = half + 2.0;
        let fade = (0.8 * drop + 8.0).clamp(10.0, 64.0);
        let scarp = pool + 0.35 * drop + 6.0;

        // Ground above the lip, along the stream, and below it, around the pool.
        let at = |s: f64, tt: f64| {
            let n = (-u.1, u.0);
            let x = p.0 + u.0 * s + n.0 * tt;
            let z = p.1 + u.1 * s + n.1 * tt;
            surface_of(&ground(x.round() as i32, z.round() as i32))
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let up = mean(&[
            at(-0.3 * run, 0.0),
            at(-0.6 * run, 0.0),
            at(-0.45 * run, half + 3.0),
            at(-0.45 * run, -half - 3.0),
        ]);
        let down = mean(&[
            at(pool, 0.0),
            at(2.0 * pool, 0.0),
            at(pool, pool),
            at(pool, -pool),
        ]);
        let mid = 0.5 * (up + down);
        let d = drop as i32;
        let ceiling = t.top as i32 - 4;
        let floor = MIN_Y + 10 + d;
        let top = ((mid + drop / 2.0 - 1.0).round() as i32).clamp(floor, ceiling.max(floor));
        Self {
            seed,
            p,
            u,
            half,
            pool,
            run,
            spring,
            top,
            low: top - d,
            fade,
            scarp,
        }
    }

    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let r = self.run + self.spring + self.pool * 2.0 + self.scarp + self.fade + 4.0;
        (self.p.0 - r, self.p.1 - r, self.p.0 + r, self.p.1 + r)
    }

    /// Position in the waterfall's frame.
    fn frame(&self, x: f64, z: f64) -> (f64, f64) {
        let (dx, dz) = (x - self.p.0, z - self.p.1);
        (dx * self.u.0 + dz * self.u.1, -dx * self.u.1 + dz * self.u.0)
    }

    /// Where the cliff is at `t` across: along the pool's upstream half, then on to the
    /// sides, bending a little downstream and wavering.
    fn cliff(&self, t: f64) -> f64 {
        let r = self.pool;
        let a = t.abs();
        let base = if a < r {
            r - (r * r - a * a).sqrt()
        } else {
            r + 0.3 * (a - r)
        };
        // Wavers only away from the falling water, so the lip stays clean.
        let waver = smoothstep(self.half, self.half + 3.0, a);
        base + waver * 1.6 * perlin1(self.seed ^ 0xC11F, t / 7.0)
    }

    /// Half-width of the stream at `s` (negative, upstream of the lip).
    fn stream_half(&self, s: f64) -> f64 {
        let near_lip = smoothstep(-4.0, -9.0, s);
        self.half * (1.0 + near_lip * 0.2 * perlin1(self.seed ^ 0x57EA, s / 9.0))
    }

    fn part(&self, s: f64, t: f64) -> Part {
        let cliff = self.cliff(t);
        if s < cliff {
            // The stream, from the spring to the lip.
            let hw = self.stream_half(s);
            let channel = if s > -self.run { t.abs() - hw } else { f64::MAX };
            let (ss, st) = (s + self.run, t);
            let wobble = 1.0 + 0.6 * (0.5 + 0.5 * perlin(self.seed ^ 0x5921, ss / 4.0, st / 4.0));
            let spring = ss.hypot(st) - (self.spring + wobble - 1.0);
            let edge = channel.min(spring);
            Part::Upper {
                water: edge < 0.0,
                edge,
            }
        } else {
            let (ps, pt) = (s - self.pool, t);
            let n = 0.5 + 0.5 * perlin(self.seed ^ 0x9001, ps / 6.0, pt / 6.0);
            let r = self.pool + 0.75 + 1.25 * n;
            let edge = ps.hypot(pt) - r;
            Part::Lower {
                water: edge < 0.0,
                edge,
            }
        }
    }

    /// 1 near the waterfall, fading to 0 where the land is untouched.
    fn weight(&self, s: f64, t: f64) -> f64 {
        // Distance to the stream's axis, the pool and the escarpment.
        let stream = {
            let ss = s.clamp(-self.run, 0.0);
            (s - ss).hypot(t) - (self.half + 4.0)
        };
        let spring = (s + self.run).hypot(t) - (self.spring + 4.0);
        let pool = (s - self.pool).hypot(t) - (self.pool + 5.0);
        let scarp = {
            let tt = t.clamp(-self.scarp, self.scarp);
            (s - self.cliff(tt)).hypot(t - tt) - 3.0
        };
        let d = stream.min(spring).min(pool).min(scarp).max(0.0);
        1.0 - smoothstep(0.0, self.fade, d)
    }

    pub fn apply(&self, t: &Terrain, x: i32, z: i32, s: &mut Sample) {
        let (fx, fz) = (x as f64, z as f64);
        let (ls, lt) = self.frame(fx, fz);
        let w = self.weight(ls, lt);
        if w <= 0.0 {
            return;
        }
        let part = self.part(ls, lt);
        let bumps = 0.6 * fbm(self.seed ^ 0xB4B5, fx / 11.0, fz / 11.0, 2, 0.5);
        let (top, low) = (self.top as f64, self.low as f64);
        let target = match part {
            Part::Upper { water: true, edge } => {
                // Deeper in the middle; shallow over the rock of the lip.
                let hw = self.stream_half(ls).max(1.0);
                let depth = (0.8 + 0.25 * self.half).min(2.5);
                let across = (1.0 - (-edge / hw).min(1.0)).powi(2);
                let lip = smoothstep(-3.0, -0.5, ls);
                top + 1.0 - 0.3 - (depth - 0.3) * (1.0 - across) * (1.0 - 0.7 * lip)
            }
            // Banks rise gently from the water to the terrace.
            Part::Upper { water: false, edge } => {
                top + 1.25 + (0.3 * edge).min(1.6) + bumps.max(-0.2) * smoothstep(2.0, 6.0, edge)
            }
            Part::Lower { water: true, edge } => {
                let r = self.pool + 1.5;
                let depth = (1.5 + 0.12 * (top - low)).min(7.0);
                let bowl = (1.0 - ((r + edge) / r).clamp(0.0, 1.0).powi(2)).powf(0.7);
                low + 1.0 - 0.3 - (depth - 0.3) * bowl
            }
            Part::Lower { water: false, edge } => {
                low + 1.3 + (0.35 * edge).min(4.0) + bumps.max(-0.25) * smoothstep(2.0, 6.0, edge)
            }
        };
        let base = s.h as f64;
        let core = w >= 1.0;
        // The land's own rivers and lakes stay as they are around the waterfall, and the
        // ground beside them is only ever raised, so their water stays in.
        if !core && s.water != WaterKind::None {
            return;
        }
        let mut h = lerp(base, target, w);
        if near_water(t, x, z, s) {
            h = h.max(base);
        }

        match part {
            Part::Upper { water: true, edge } | Part::Lower { water: true, edge } if core => {
                let level = match part {
                    Part::Upper { .. } => self.top,
                    _ => self.low,
                };
                s.water = WaterKind::River;
                s.level = Some(level);
                s.h = h.min(level as f64 + 1.0 - 0.07) as f32;
                s.beach = 0.0;
                s.river_edge = s.river_edge.min(edge as f32);
                // The curtain: pool columns beside the end of the stream.
                let lower = matches!(part, Part::Lower { .. });
                let feeds = |dx: f64, dz: f64| {
                    let (a, b) = self.frame(fx + dx, fz + dz);
                    matches!(self.part(a, b), Part::Upper { water: true, .. })
                };
                s.fall = (lower
                    && (feeds(1.0, 0.0) || feeds(-1.0, 0.0) || feeds(0.0, 1.0) || feeds(0.0, -1.0)))
                .then_some(self.top);
            }
            _ => {
                let edge = match part {
                    Part::Upper { edge, .. } | Part::Lower { edge, .. } => edge,
                };
                if s.water != WaterKind::None {
                    // The land's own water where the waterfall's ground goes is filled in, up
                    // to its surface at least, so the rest of it stays in.
                    if let Some(l) = s.level {
                        h = h.max(l as f64 + 1.05);
                    }
                    s.water = WaterKind::None;
                    s.level = None;
                    s.fall = None;
                }
                if core {
                    s.beach = 0.0;
                    s.river_edge = s.river_edge.min(edge as f32);
                }
                s.h = h.clamp(MIN_Y as f64 + 4.0, t.top as f64) as f32;
            }
        }
    }
}
