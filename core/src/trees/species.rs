//! The species: how each kind of tree is laid out in a [`Plan`].
//!
//! Every tree is drawn the way Conquest builders build them by hand: a round trunk that tapers
//! from full logs (with vertical slabs and quarters around them on thick trunks, and roots
//! spreading from the biggest) to pillars, limbs that thin from bark blocks to large, normal
//! and small branches as they fork, and leaves in loose clumps on the twig ends.
//!
//! Broadleaf crowns are grown towards their outline: points spread over a dome, each reached
//! by a twig from the nearest limb and covered by a clump, so the crown is full and lumpy on
//! the outside and open inside, where the limbs show from below. Conifers stack whorls of
//! branches under flat sprays of needles.

use std::f32::consts::{PI, TAU};

use super::kit::{Leaf, Wood};
use super::plan::{
    add, angles, dir, length, scale, sub, unit3, Piece, Plan, Thick, FOOT, MOSSY, PILLAR_PX, STUMP,
    V3,
};
use super::{Species, TreeRequest, MAX_REACH};
use crate::noise::Rng;

/// Blocks a plan keeps below the trunk base, for trunks that reach down a slope.
pub(super) const BELOW: i32 = 10;

/// Rough height and crown radius in blocks of a species at a size, for previews.
pub(super) fn shape(species: Species, size: f32) -> (f32, f32) {
    let s = size.clamp(0.0, 1.0);
    match species {
        Species::Oak => (6.0 + 19.0 * s.powf(1.1) + 1.5, 2.8 + 9.2 * s + 1.5),
        Species::Birch => (7.0 + 15.0 * s + 1.0, 1.6 + 3.6 * s + 1.0),
        Species::Spruce => (
            6.0 + 26.0 * s + 0.5,
            (1.4 + 0.25 * (6.0 + 26.0 * s)).min(9.5),
        ),
        Species::Pine => (9.0 + 21.0 * s + 1.0, 2.5 + 5.5 * s),
        Species::Willow => (6.0 + 11.0 * s, 3.5 + 7.5 * s),
        Species::Dead => (6.0 + 16.0 * s + 1.0, 2.5 + 7.0 * s + 1.0),
        Species::Bush => (1.5 + 2.0 * s, 1.2 + 2.0 * s),
    }
}

/// Lays out a tree. The plan is relative to the trunk: see [`Plan`].
pub(super) fn plan(req: &TreeRequest) -> Plan {
    let s = req.size.clamp(0.0, 1.0);
    let wood = match req.species {
        Species::Oak | Species::Dead | Species::Bush => Wood::Oak,
        Species::Birch => Wood::Birch,
        Species::Spruce => Wood::Spruce,
        Species::Pine => Wood::Pine,
        Species::Willow => Wood::Willow,
    };
    let above = shape(req.species, s).0.ceil() as i32 + 6;
    let mut plan = Plan::new(MAX_REACH, BELOW, above, wood as u8);
    let mut tree = Tree {
        plan: &mut plan,
        rng: Rng::new(req.seed),
        seed: req.seed,
        x: 0.5,
        z: 0.5,
        cell: (0, 0),
        y: 0,
        s,
        tips: Vec::new(),
    };
    match req.species {
        Species::Oak => tree.oak(),
        Species::Birch => tree.birch(),
        Species::Spruce => tree.spruce(),
        Species::Pine => tree.pine(),
        Species::Willow => tree.willow(),
        Species::Dead => tree.dead(),
        Species::Bush => tree.bush(),
    }
    plan
}

/// A point on a limb or trunk that other limbs can branch from, with the radius there in
/// pixels.
#[derive(Clone, Copy, Debug)]
struct Node {
    p: V3,
    r: f32,
    /// On the trunk: limbs only leave it when no branch is close.
    trunk: bool,
}

/// Leaf clumps of a crown: radius range sideways in blocks, height as a share of the
/// radius, lift above the twig end, and share of holes.
#[derive(Clone, Copy)]
struct Clumps {
    rx: (f32, f32),
    flat: f32,
    lift: f32,
    holes: f32,
    /// Share of twig ends that get a clump.
    share: f32,
}

/// How a species grows its limbs.
#[derive(Clone, Copy)]
struct Style {
    /// Length of one straight stretch, in blocks.
    seg: f32,
    /// Largest random turn at each joint, in radians.
    wobble: f32,
    /// Change of elevation at each joint (negative bends down), in radians.
    droop: f32,
    /// Lowest elevation a drooping limb reaches.
    min_elev: f32,
    /// Chance of a side branch at each joint.
    forks: f32,
    /// Range of the side branch's turn away from its parent, in radians.
    fork_angle: (f32, f32),
    /// Extra elevation of a side branch.
    fork_rise: f32,
    /// Side branch length as a share of what is left of the parent.
    fork_len: f32,
    /// Side branch thickness as a share of the parent's at the fork.
    fork_r: f32,
    /// Levels of side branches.
    depth: u32,
}

struct Tree<'p> {
    plan: &'p mut Plan,
    rng: Rng,
    seed: u64,
    /// Trunk axis (block units).
    x: f32,
    z: f32,
    /// Trunk column.
    cell: (i32, i32),
    /// Y of the trunk's first cell.
    y: i32,
    s: f32,
    /// Limb ends recorded by `grow`, where leaves go.
    tips: Vec<V3>,
}

/// The pillar closest to a width in pixels, or a log for a full block.
fn pillar_for(width: f32) -> Piece {
    if width >= 14.0 {
        return Piece::Log;
    }
    let l = (0..5)
        .min_by(|&a, &b| {
            let ea = (PILLAR_PX[a] as f32 - width).abs();
            let eb = (PILLAR_PX[b] as f32 - width).abs();
            ea.total_cmp(&eb)
        })
        .unwrap_or(0);
    Piece::Pillar(l as u8 + 1)
}

impl Tree<'_> {
    fn f(&mut self) -> f32 {
        self.rng.unit() as f32
    }

    fn between(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }

    /// Centres thick trunks: up to 1.5 blocks across on the column, a two-by-two trunk on a
    /// block corner, anything thicker on the column again.
    fn centre_trunk(&mut self, diameter_px: f32) {
        if (24.0..38.0).contains(&diameter_px) {
            let (cx, cz) = (self.rng.range(0, 1), self.rng.range(0, 1));
            self.x = (self.cell.0 + cx) as f32;
            self.z = (self.cell.1 + cz) as f32;
        }
    }

    /// The point on the trunk axis at a height above the base.
    fn axis(&self, h: f32) -> V3 {
        [self.x, self.y as f32 + h, self.z]
    }

    /// A round trunk `height` levels tall. `radius(t, level)` is the radius in blocks at
    /// `t = level / height`, `offset(t)` the drift of the axis, and the lowest `moss` levels
    /// are mossy. Returns the centre of the top level.
    fn trunk(
        &mut self,
        height: i32,
        radius: impl Fn(f32, f32) -> f32,
        offset: impl Fn(f32) -> (f32, f32),
        moss: i32,
    ) -> V3 {
        let mut top = self.axis(0.5);
        // Thick trunks flare into a few buttress roots: round lobes on the edge of the foot,
        // drawn with it as one shape of stump pieces.
        let mut foot = Vec::new();
        let base = radius(0.0, 0.0);
        if base >= 0.6 {
            let (ox, oz) = offset(0.0);
            let n = 3 + (base * 1.5).min(2.0) as i32 + self.rng.range(0, 1);
            let az0 = self.f() * TAU;
            for i in 0..n {
                let az = az0 + i as f32 * TAU / n as f32 + self.between(-0.35, 0.35);
                let r = (base * self.between(0.45, 0.6)).clamp(0.35, 1.0);
                let out = base + r * self.between(-0.1, 0.35);
                foot.push((
                    self.x + ox + az.cos() * out,
                    self.z + oz + az.sin() * out,
                    r,
                ));
            }
        }
        // The cell a trunk thinner than a pillar stands in, on the level below.
        let mut thin: Option<[i32; 3]> = None;
        for level in 0..height.max(1) {
            let t = level as f32 / height.max(1) as f32;
            let (ox, oz) = offset(t);
            let mut flags = 0;
            if level == 0 {
                flags |= FOOT | STUMP;
            }
            if level < moss {
                flags |= MOSSY;
            }
            self.plan.flags = flags;
            let (mut cx, mut cz) = (self.x + ox, self.z + oz);
            let r = radius(t, level as f32);
            if r < 0.4 {
                // Thinner than a pillar: one piece in one cell, not slivers in four. It stays
                // over the cell below until the trunk has leant well out of it, then steps
                // across with a joint on the level below, so the pieces always meet.
                let y = self.y + level;
                let mut c = [cx.floor() as i32, y, cz.floor() as i32];
                if let Some(below) = thin {
                    let off = |a: usize, v: f32| (v - (below[a] as f32 + 0.5)).abs();
                    if off(0, cx) <= 0.95 && off(2, cz) <= 0.95 {
                        c = [below[0], y, below[2]];
                    } else {
                        self.plan
                            .join(below, [c[0], below[1], c[2]], pillar_for(32.0 * r));
                    }
                }
                thin = Some(c);
                (cx, cz) = (c[0] as f32 + 0.5, c[2] as f32 + 0.5);
            } else {
                thin = None;
            }
            if level == 0 {
                foot.push((cx, cz, r));
                self.plan.blob(self.y, &foot);
            } else {
                self.plan.disc(self.y + level, cx, cz, r);
            }
            top = [cx, (self.y + level) as f32 + 0.5, cz];
        }
        self.plan.flags = 0;
        top
    }

    /// A thin upright stem through `points` (radius in pixels), standing on a stump.
    fn stem(&mut self, points: &[(V3, f32)], moss: bool) {
        let (base, r) = points[0];
        let c = super::plan::cell_of(base);
        self.plan.flags = FOOT | STUMP | if moss { MOSSY } else { 0 };
        self.plan.put(c[0], c[1], c[2], pillar_for(2.0 * r));
        self.plan.flags = 0;
        self.plan.limb(points);
    }

    /// A limb from `start` heading `d`, `len` blocks long and thinning from `r0` to `r1`
    /// pixels, with side branches as the style says. Records every end in `tips`.
    #[allow(clippy::too_many_arguments)]
    fn grow(&mut self, start: V3, d: V3, len: f32, r0: f32, r1: f32, st: &Style, depth: u32) {
        let (mut az, mut el) = angles(d);
        let mut p = start;
        let mut points = vec![(p, r0)];
        let mut done = 0.0;
        let mut forks = Vec::new();
        while done < len - 0.05 {
            let step = st.seg.min(len - done);
            az += (self.f() - 0.5) * 2.0 * st.wobble;
            el = (el + (self.f() - 0.5) * st.wobble + st.droop).clamp(st.min_elev, 1.45);
            p = add(p, scale(dir(az, el), step));
            done += step;
            let r = r0 + (r1 - r0) * (done / len);
            points.push((p, r));
            if depth < st.depth && done > len * 0.25 && done < len * 0.9 && self.chance(st.forks) {
                let side = if self.chance(0.5) { -1.0 } else { 1.0 };
                let turn = self.between(st.fork_angle.0, st.fork_angle.1);
                let rise = st.fork_rise + self.between(-0.15, 0.15);
                let flen = (len - done) * st.fork_len + self.between(0.5, 1.2);
                forks.push((
                    p,
                    dir(az + side * turn, el + rise),
                    flen,
                    (r * st.fork_r).max(r1),
                ));
            }
        }
        self.plan.limb(&points);
        self.tips.push(p);
        for (fp, fd, flen, fr) in forks {
            self.grow(fp, fd, flen, fr, r1, st, depth + 1);
        }
    }

    /// Where the leaves of a crown go: points spread evenly over the part of an ellipsoid
    /// around `c` (radii `rx` sideways, `ry` up) above `low` (−1 is its bottom, 1 its top),
    /// about `spacing` blocks apart, each pulled in by up to `depth` of the radius so the
    /// outline comes out lumpy.
    fn crown_points(
        &mut self,
        c: V3,
        rx: f32,
        ry: f32,
        low: f32,
        spacing: f32,
        depth: f32,
    ) -> Vec<V3> {
        // Surface of the whole ellipsoid (Thomsen's formula), to space the points.
        let (a, b) = (rx.powf(1.6), ry.powf(1.6));
        let area = 2.0 * TAU * ((a * a + 2.0 * a * b) / 3.0).powf(1.0 / 1.6);
        let full = ((area / (spacing * spacing)).round() as i32).max(3);
        let az0 = self.f() * TAU;
        let mut out = Vec::new();
        for i in 0..full {
            let v = 1.0 - 2.0 * (i as f32 + 0.5) / full as f32;
            if v < low {
                break;
            }
            let side = (1.0 - v * v).sqrt();
            let az = az0 + i as f32 * 2.399_963;
            let k = 1.0 - depth * self.f();
            let mut p = [
                c[0] + rx * side * az.cos() * k,
                c[1] + ry * v * k,
                c[2] + rx * side * az.sin() * k,
            ];
            for q in &mut p {
                *q += self.between(-0.25, 0.25) * spacing;
            }
            out.push(p);
        }
        out
    }

    /// A limb from `a` to `b`, thinning from `ra` to `rb` pixels, arched up in the middle by
    /// `bow` of its length and a little crooked. Returns its points as nodes.
    fn bough(&mut self, a: V3, ra: f32, b: V3, rb: f32, bow: f32) -> Vec<Node> {
        let d = sub(b, a);
        let len = length(d);
        let n = (len / 1.5).ceil().max(1.0) as usize;
        let mut points = vec![(a, ra)];
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let arc = (t * PI).sin();
            let mut p = add(a, scale(d, t));
            p[1] += bow * len * arc;
            if i < n {
                p[0] += self.between(-0.35, 0.35) * arc;
                p[1] += self.between(-0.2, 0.2) * arc;
                p[2] += self.between(-0.35, 0.35) * arc;
            }
            points.push((p, ra + (rb - ra) * t));
        }
        self.plan.limb(&points);
        points
            .into_iter()
            .map(|(p, r)| Node { p, r, trunk: false })
            .collect()
    }

    /// Fills a crown around existing limbs: side limbs from `nodes` out to groups of
    /// `targets`, a twig from the nearest limb to every target, and a clump of leaves on each
    /// twig end. `groups` is how many side limbs (0 for twigs straight off the limbs).
    fn crown(
        &mut self,
        nodes: &mut Vec<Node>,
        targets: &[V3],
        groups: usize,
        bow: f32,
        clumps: Clumps,
        leaf: impl Fn(u64) -> u8,
    ) {
        if targets.is_empty() {
            return;
        }
        // Group the targets (a few rounds of k-means) and send a side limb towards each group.
        let groups = groups.min(targets.len());
        if groups > 0 {
            let step = targets.len() as f32 / groups as f32;
            let mut centres: Vec<V3> = (0..groups)
                .map(|g| targets[(g as f32 * step) as usize])
                .collect();
            for _ in 0..4 {
                let mut sums = vec![([0.0f32; 3], 0usize); groups];
                for t in targets {
                    let g = nearest(&centres, *t);
                    sums[g].0 = add(sums[g].0, *t);
                    sums[g].1 += 1;
                }
                for (c, (sum, n)) in centres.iter_mut().zip(sums) {
                    if n > 0 {
                        *c = scale(sum, 1.0 / n as f32);
                    }
                }
            }
            for g in centres {
                let from = best_node(nodes, g);
                let end = add(from.p, scale(sub(g, from.p), 0.8));
                let r0 = (from.r * 0.75).clamp(2.6, 4.0);
                let more = self.bough(from.p, r0, end, 2.4, bow);
                nodes.extend(more.into_iter().skip(1));
            }
        }
        for (n, &t) in targets.iter().enumerate() {
            let from = best_node(nodes, t);
            let r0 = (from.r * 0.8).clamp(1.6, 2.4);
            self.bough(from.p, r0, t, 1.6, bow * 0.5);
            if !self.chance(clumps.share) {
                continue;
            }
            let seed = self.seed ^ (n as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let rx = self.between(clumps.rx.0, clumps.rx.1);
            self.plan.clump(
                add(t, [0.0, clumps.lift, 0.0]),
                rx,
                rx * clumps.flat,
                clumps.holes,
                leaf(seed),
                seed,
            );
        }
    }

    /// Leaves on every recorded tip: clumps of `rx` × `ry` blocks centred `lift` above it.
    fn leaf_tips(
        &mut self,
        rx: (f32, f32),
        ry: f32,
        lift: f32,
        holes: f32,
        leaf: impl Fn(u64) -> u8,
    ) {
        let tips = std::mem::take(&mut self.tips);
        for (n, tip) in tips.iter().enumerate() {
            let r = self.between(rx.0, rx.1);
            let seed = self.seed ^ (n as u64).wrapping_mul(0x9E37_79B9);
            self.plan.clump(
                add(*tip, [0.0, lift, 0.0]),
                r,
                ry * r / rx.1.max(0.1),
                holes,
                leaf(seed),
                seed,
            );
        }
        self.tips = tips;
    }

    /// Short dead branch stubs on the bare lower trunk, between `from` and `to` blocks up.
    fn stubs(&mut self, count: i32, from: f32, to: f32, r: f32) {
        for _ in 0..count {
            let h = self.between(from, to.max(from + 0.1));
            let az = self.f() * TAU;
            let start = self.axis(h);
            let len = r + self.between(0.6, 1.6);
            let end = add(start, scale(dir(az, self.between(-0.4, 0.2)), len));
            self.plan.limb(&[(start, 3.0), (end, 2.0)]);
        }
    }

    /// A young tree: a thin stem on a small stump with a tuft of leaves, or a little cone of
    /// needles.
    fn sapling(&mut self, leaf: Leaf, conifer: bool) {
        let h = 1 + (self.s * 18.0) as i32;
        let (x, z) = self.cell;
        let top = self.axis(h as f32 + 0.5);
        self.stem(&[(self.axis(0.5), 2.0), (top, 1.5)], false);
        if conifer {
            for k in 0..h {
                let ring = if k == h - 1 || k == 0 { 0.6 } else { 0.9 };
                for (dx, dz) in super::plan::SIDES {
                    if unit3(self.seed, dx, k, dz) < ring {
                        self.plan
                            .put_leaves(x + dx, self.y + k + 1, z + dz, leaf as u8);
                    }
                }
            }
            self.plan.put_leaves(x, self.y + h + 1, z, leaf as u8);
        } else {
            let seed = self.seed;
            self.plan
                .clump(add(top, [0.0, 0.2, 0.0]), 1.4, 1.1, 0.25, leaf as u8, seed);
            self.plan.put_leaves(x, self.y + h + 1, z, leaf as u8);
        }
    }

    /// Broad oak: a short, thick trunk that splits into crooked scaffold limbs, which fork
    /// again into branches and twigs; leaves sit in loose clumps on the twig ends.
    fn oak(&mut self) {
        let s = self.s;
        if s < 0.12 {
            return self.sapling(Leaf::Oak, false);
        }
        let h = 6.0 + 19.0 * s.powf(1.1);
        let crown = 2.8 + 9.2 * s;
        let d = 6.0 + 38.0 * s.powf(1.6);
        self.centre_trunk(d);
        let r0 = d / 32.0;
        let trunk_h = (h * self.between(0.3, 0.4)).round().max(2.0) as i32;
        let (lean_az, lean) = (self.f() * TAU, self.between(0.0, 0.1) * trunk_h as f32);
        let flare = d >= 12.0;
        let moss = if s > 0.55 && self.chance(0.6) {
            self.rng.range(1, 2)
        } else {
            0
        };
        let top = self.trunk(
            trunk_h,
            |t, level| {
                let r = r0 * (1.0 - 0.3 * t);
                if flare {
                    r * (1.0 + 0.45 * (-level / 0.9).exp())
                } else {
                    r
                }
            },
            |t| (lean * t * lean_az.cos(), lean * t * lean_az.sin()),
            moss,
        );
        let r_top = r0 * 0.7 * 16.0;
        // The crown: a dome, wider than tall, whose top is the tree's height.
        let ry = ((h - trunk_h as f32) * 0.47).max(1.6);
        let rx = (crown - 1.8).clamp(1.6, MAX_REACH as f32 - 3.5);
        let c = [top[0], self.y as f32 + h - ry - 1.0, top[2]];
        let mut nodes: Vec<Node> = (trunk_h * 2 / 3..trunk_h)
            .map(|level| Node {
                p: [top[0], (self.y + level) as f32 + 0.5, top[2]],
                r: r_top,
                trunk: true,
            })
            .collect();

        // Scaffold limbs from the top of the trunk out into the crown, the first a leader
        // that carries on up.
        let k = if s < 0.3 {
            2
        } else if s < 0.6 {
            3
        } else {
            self.rng.range(3, 5)
        };
        let az0 = self.f() * TAU;
        for i in 0..k {
            let az = az0 + i as f32 * TAU / k as f32 + self.between(-0.35, 0.35);
            let leader = i == 0;
            let (out, up) = if leader {
                (self.between(0.1, 0.3), self.between(0.4, 0.6))
            } else {
                (self.between(0.45, 0.7), self.between(-0.3, 0.25))
            };
            let end = add(c, [rx * out * az.cos(), ry * up, rx * out * az.sin()]);
            let start = add(top, scale(dir(az, 0.0), r0 * 0.4));
            let r = r_top * if leader { 0.75 } else { 0.62 };
            let limb = self.bough(start, r.max(3.0), end, 3.0, 0.1);
            nodes.extend(limb);
        }
        // Big oaks also send out long, low limbs from the trunk.
        if s > 0.55 {
            for _ in 0..self.rng.range(1, 2) {
                let az = self.f() * TAU;
                let at = trunk_h as f32 * self.between(0.6, 0.85);
                let start = self.axis(at);
                let end = add(c, [rx * 0.75 * az.cos(), -ry * 0.45, rx * 0.75 * az.sin()]);
                let limb = self.bough(start, r_top * 0.5, end, 2.8, 0.12);
                nodes.extend(limb);
            }
        }
        let targets = self.crown_points(c, rx, ry, -0.5, 2.6 + 0.8 * s, 0.4);
        let groups = targets.len() / 4;
        let clumps = Clumps {
            rx: (1.6 + 0.3 * s, 2.5 + 0.7 * s),
            flat: 0.65,
            lift: 0.4,
            holes: 0.26,
            share: 1.0,
        };
        self.crown(&mut nodes, &targets, groups, 0.12, clumps, |_| {
            Leaf::Oak as u8
        });
    }

    /// Birch: a slim white trunk (sometimes two or three from one root) with many thin,
    /// rising branches and small, light clumps of leaves.
    fn birch(&mut self) {
        let s = self.s;
        if s < 0.12 {
            return self.sapling(Leaf::Birch, false);
        }
        let h = 7.0 + 15.0 * s;
        let d = 5.0 + 8.5 * s;
        let stems = if s > 0.3 && self.chance(0.3) {
            self.rng.range(2, 3)
        } else {
            1
        };
        let az0 = self.f() * TAU;
        for i in 0..stems {
            let (hh, dd, lean_az, lean) = if stems == 1 {
                (h, d, self.f() * TAU, self.between(0.0, 0.08))
            } else {
                let az = az0 + i as f32 * TAU / stems as f32;
                let hh = h * if i == 0 {
                    1.0
                } else {
                    self.between(0.75, 0.92)
                };
                (hh, d * 0.8, az, self.between(0.08, 0.16))
            };
            let base = self.birch_stem(i, stems, hh, dd, lean_az, lean);
            // Stems of a clump grow from one root: join their feet.
            if i > 0 {
                let main = super::plan::cell_of(self.axis(0.5));
                self.plan.flags = FOOT | STUMP;
                self.plan.join(main, base, Piece::Branch(Thick::Large));
                self.plan.flags = 0;
            }
        }
    }

    /// One stem of a birch; returns the cell it stands in.
    fn birch_stem(
        &mut self,
        i: i32,
        stems: i32,
        h: f32,
        d: f32,
        lean_az: f32,
        lean: f32,
    ) -> [i32; 3] {
        let s = self.s;
        let base = if stems > 1 && i > 0 {
            let (dx, dz) =
                super::plan::SIDES[((lean_az / TAU * 4.0).round() as i32).rem_euclid(4) as usize];
            [self.x + dx as f32, self.y as f32 + 0.5, self.z + dz as f32]
        } else {
            self.axis(0.5)
        };
        let n = (h / 2.0).ceil() as i32;
        let mut points = Vec::new();
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let sway = if k == 0 {
                [0.0; 2]
            } else {
                [self.between(-0.2, 0.2), self.between(-0.2, 0.2)]
            };
            let off = lean * h * t;
            points.push((
                [
                    base[0] + off * lean_az.cos() + sway[0],
                    base[1] + h * t,
                    base[2] + off * lean_az.sin() + sway[1],
                ],
                ((d / 2.0) * (1.0 - 0.6 * t)).max(1.8),
            ));
        }
        let moss = s > 0.6 && self.chance(0.5);
        self.stem(&points, moss);
        // Thin branches straight off the stem, out to a narrow, tall crown.
        let y0 = h * self.between(0.3, 0.4);
        let top = points.last().unwrap().0;
        let mut nodes: Vec<Node> = points
            .iter()
            .filter(|(p, _)| p[1] - base[1] > y0 - 0.5)
            .map(|&(p, r)| Node { p, r, trunk: false })
            .collect();
        if nodes.is_empty() {
            nodes.push(Node {
                p: top,
                r: 2.0,
                trunk: false,
            });
        }
        let ry = (h - y0) * 0.52;
        let rx = (1.3 + 3.2 * s) * if stems > 1 { 0.75 } else { 1.0 };
        let c = [top[0], top[1] - ry * 0.95, top[2]];
        let targets = self.crown_points(c, rx, ry, -0.8, 2.0 + 0.4 * s, 0.35);
        let clumps = Clumps {
            rx: (1.1 + 0.3 * s, 1.6 + 0.5 * s),
            flat: 0.85,
            lift: 0.2,
            holes: 0.3,
            share: 1.0,
        };
        let groups = if s > 0.5 { targets.len() / 6 } else { 0 };
        self.crown(&mut nodes, &targets, groups, 0.1, clumps, |_| {
            Leaf::Birch as u8
        });
        let stubs = self.rng.range(0, 2);
        self.stubs(stubs, 1.0, y0 - 1.0, 0.0);
        super::plan::cell_of(base)
    }

    /// Spruce: a straight trunk to the very top, tapering to a thin pillar, with whorls of
    /// drooping branches under skirts of needles that get shorter towards the top, and dead
    /// stubs below the live crown.
    fn spruce(&mut self) {
        let s = self.s;
        if s < 0.1 {
            return self.sapling(Leaf::Spruce, true);
        }
        let h = 6.0 + 26.0 * s;
        let d = 6.0 + 30.0 * s.powf(1.4);
        self.centre_trunk(d);
        let r0 = d / 32.0;
        let ht = h.round() as i32;
        let flare = d >= 12.0;
        let moss = if s > 0.5 && self.chance(0.5) { 1 } else { 0 };
        self.trunk(
            ht,
            |t, level| {
                let r = r0 * (1.0 - t).powf(0.85) + 0.03;
                if flare {
                    r * (1.0 + 0.4 * (-level / 0.8).exp())
                } else {
                    r
                }
            },
            |_| (0.0, 0.0),
            moss,
        );
        let h0 = (h * self.between(0.12, 0.22) + 2.0 * s).round() as i32;
        let radius = ((1.4 + 0.25 * h) * self.between(0.9, 1.1)).min(9.5);
        let n = 4 + (s > 0.5) as i32;
        let gap = if s < 0.45 { 2 } else { 3 };
        self.tiers(h0, ht, radius, (n, n + 1), gap, 1.0, Leaf::Spruce as u8);
        let stubs = self.rng.range(1, 2 + (4.0 * s) as i32);
        self.stubs(stubs, 1.5, h0 as f32 - 0.5, r0);
    }

    /// A conifer crown on a trunk `ht` levels tall: whorls of branches from level `h0` up,
    /// every two levels (or up to `gap` low down), `branches` of them in a whorl, shorter
    /// towards the top, each under a flat spray of needles `pad` times the usual width; then
    /// a narrow spire of needles around the leader.
    #[allow(clippy::too_many_arguments)]
    fn tiers(
        &mut self,
        h0: i32,
        ht: i32,
        radius: f32,
        branches: (i32, i32),
        gap: i32,
        pad: f32,
        leaf: u8,
    ) {
        let s = self.s;
        let mut az = self.f() * TAU;
        let mut level = h0;
        while level < ht - 2 {
            let u = (level - h0) as f32 / (ht - h0).max(1) as f32;
            let reach = radius * (1.0 - u).powf(0.9) + 0.6;
            let n = self.rng.range(branches.0, branches.1);
            let r = 2.0 + 2.5 * s * (1.0 - u);
            for _ in 0..n {
                az += TAU / n as f32 + self.between(-0.35, 0.35);
                let len = reach * self.between(0.8, 1.05);
                let start = self.axis(level as f32 + 0.5);
                self.needle_branch(start, az, len, r, pad, leaf);
            }
            level += if u > 0.6 {
                2
            } else {
                self.rng.range(2, gap.max(2))
            };
        }
        let (tx, tz) = ((self.x - 0.5).round() as i32, (self.z - 0.5).round() as i32);
        let base = level.min(ht - 1);
        for k in 1..=3 {
            for (dx, dz) in super::plan::SIDES {
                if k < 3 || unit3(self.seed, dx, k, dz) < 0.5 {
                    self.plan
                        .put_leaves(tx + dx, self.y + base - 1 + k, tz + dz, leaf);
                }
            }
        }
        for k in base..ht + 1 {
            self.plan.put_leaves(tx, self.y + k, tz, leaf);
        }
    }

    /// One spruce branch: out from the trunk, sagging a little and turning up at the tip,
    /// under a flat spray of needles that is widest halfway out.
    #[allow(clippy::too_many_arguments)]
    fn needle_branch(&mut self, start: V3, az: f32, len: f32, r0: f32, pad: f32, leaf: u8) {
        let mut el = self.between(-0.1, 0.05);
        let mut p = start;
        let mut points = vec![(p, r0)];
        let mut done = 0.0;
        while done < len - 0.05 {
            let step = 1.0_f32.min(len - done);
            el = if done + step >= len - 0.05 && len > 2.5 {
                0.25
            } else {
                (el - 0.06 + self.between(-0.05, 0.05)).max(-0.35)
            };
            p = add(p, scale(dir(az, el), step));
            done += step;
            points.push((p, r0 + (1.8 - r0) * done / len));
        }
        self.plan.limb(&points);
        let seed = self.seed ^ (az.to_bits() as u64);
        let o = (self.cell.0, self.y, self.cell.1);
        for &(q, _) in &points {
            let out = length(sub([q[0], 0.0, q[2]], [start[0], 0.0, start[2]]));
            let t = out / len.max(0.5);
            if out < 0.8 {
                continue;
            }
            let w = (0.7 + 1.1 * (t * PI).sin().max(0.3) * (len / 4.0).min(1.0)) * pad;
            let c = super::plan::cell_of(q);
            let reach = w.ceil() as i32;
            for dx in -reach..=reach {
                for dz in -reach..=reach {
                    let (x, z) = (c[0] + dx, c[2] + dz);
                    let d =
                        ((x as f32 + 0.5 - q[0]).powi(2) + (z as f32 + 0.5 - q[2]).powi(2)).sqrt();
                    let n = |k: u64, y: i32| unit3(seed ^ k, x - o.0, y - o.1, z - o.2);
                    if d + (n(1, c[1]) - 0.5) * 0.7 < w {
                        self.plan.put_leaves(x, c[1], z, leaf);
                    }
                    if d + (n(2, c[1]) - 0.5) * 0.5 < w * 0.45 && n(3, c[1]) < 0.6 {
                        self.plan.put_leaves(x, c[1] - 1, z, leaf);
                    }
                    if d < 0.8 && n(4, c[1]) < 0.3 {
                        self.plan.put_leaves(x, c[1] + 1, z, leaf);
                    }
                }
            }
        }
        let tip = super::plan::cell_of(add(p, scale(dir(az, 0.0), 0.9)));
        self.plan.put_leaves(tip[0], tip[1], tip[2], leaf);
    }

    /// Pine: young pines are open cones; old ones have a tall, slightly bent bare trunk with
    /// broken stubs and a flat, irregular crown of crooked limbs ending in pads of needles.
    fn pine(&mut self) {
        let s = self.s;
        if s < 0.1 {
            return self.sapling(Leaf::Pine, true);
        }
        let leaf = Leaf::Pine as u8;
        if s < 0.32 {
            // A young pine: an open cone of short whorls.
            let h = 4.0 + 16.0 * s;
            let r0 = (5.0 + 10.0 * s) / 32.0;
            let ht = h.round() as i32;
            self.trunk(ht, |t, _| r0 * (1.0 - 0.8 * t) + 0.04, |_| (0.0, 0.0), 0);
            let radius = 0.8 + 0.25 * h;
            self.tiers(2, ht, radius, (3, 4), 2, 0.8, leaf);
            return;
        }
        let h = 9.0 + 21.0 * s;
        let d = 7.0 + 27.0 * s.powf(1.3);
        self.centre_trunk(d);
        let r0 = d / 32.0;
        let ht = (h - 1.0).round() as i32;
        let (bend_az, bend) = (self.f() * TAU, self.between(0.3, 1.4) * s);
        let offset = move |t: f32| {
            let o = bend * (t * 2.2).sin();
            (o * bend_az.cos(), o * bend_az.sin())
        };
        let flare = d >= 12.0;
        let moss = if self.chance(0.4) { 1 } else { 0 };
        self.trunk(
            ht,
            |t, level| {
                let r = r0 * (1.0 - 0.6 * t);
                if flare {
                    r * (1.0 + 0.45 * (-level / 0.8).exp())
                } else {
                    r
                }
            },
            offset,
            moss,
        );
        let h0 = (h * self.between(0.55, 0.68)).round() as i32;
        let st = Style {
            seg: 1.5,
            wobble: 0.45,
            droop: 0.02,
            min_elev: -0.2,
            forks: 0.45,
            fork_angle: (0.5, 1.2),
            fork_rise: 0.2,
            fork_len: 0.6,
            fork_r: 0.75,
            depth: if s > 0.7 { 2 } else { 1 },
        };
        let n = 3 + (4.0 * s) as i32;
        let mut az = self.f() * TAU;
        for i in 0..n {
            let u = i as f32 / n as f32;
            let level = h0 as f32 + (ht - 2 - h0) as f32 * u + self.between(0.0, 1.0);
            let t = level / ht as f32;
            let (ox, oz) = offset(t);
            let start = [
                self.x + ox,
                self.y as f32 + level.floor() + 0.5,
                self.z + oz,
            ];
            az += 2.4 + self.between(-0.4, 0.4);
            let len = (2.0 + 4.5 * s) * self.between(0.8, 1.2) * (1.0 - 0.4 * u);
            let r = (r0 * (1.0 - 0.6 * t) * 16.0 * 0.6).clamp(2.5, 7.0);
            let el = self.between(0.25, 0.75);
            self.grow(start, dir(az, el), len, r, 1.8, &st, 0);
        }
        let (ox, oz) = offset(1.0);
        self.tips
            .push([self.x + ox, (self.y + ht) as f32, self.z + oz]);
        self.leaf_tips((1.6 + 0.9 * s, 2.2 + 0.9 * s), 0.9, 0.6, 0.15, |_| {
            Leaf::Pine as u8
        });
        let stubs = self.rng.range(1, 3);
        self.stubs(stubs, 2.0, h0 as f32 - 1.0, r0);
    }

    /// Weeping willow: a short, thick, leaning trunk with arching limbs and long curtains of
    /// hanging leaves.
    fn willow(&mut self) {
        let s = self.s;
        let leaf = |seed: u64| {
            if seed & 1 == 0 {
                Leaf::Willow as u8
            } else {
                Leaf::Willow2 as u8
            }
        };
        if s < 0.12 {
            self.sapling(Leaf::Willow, false);
        } else {
            let d = 10.0 + 30.0 * s;
            self.centre_trunk(d);
            let r0 = d / 32.0;
            let ht = (2.5 + 4.0 * s).round() as i32;
            let (lean_az, lean) = (self.f() * TAU, self.between(0.1, 0.3) * ht as f32);
            let moss = if self.chance(0.6) { 1 } else { 0 };
            let top = self.trunk(
                ht,
                |t, level| r0 * (1.0 - 0.25 * t) * (1.0 + 0.45 * (-level / 0.8).exp()),
                |t| (lean * t * lean_az.cos(), lean * t * lean_az.sin()),
                moss,
            );
            // Arching limbs out to a low, wide dome of leaves.
            let crown = 3.5 + 7.5 * s;
            let rx = (crown - 1.5).clamp(1.5, MAX_REACH as f32 - 3.5);
            let ry = 1.8 + 2.6 * s;
            let c = add(top, [0.0, 1.0 + 2.5 * s, 0.0]);
            let r_top = r0 * 16.0 * 0.65;
            let mut nodes = vec![Node {
                p: top,
                r: r_top,
                trunk: true,
            }];
            let k = 3 + (3.0 * s) as i32;
            let az0 = self.f() * TAU;
            for i in 0..k {
                let az = az0 + i as f32 * TAU / k as f32 + self.between(-0.3, 0.3);
                let out = self.between(0.45, 0.7);
                let end = add(
                    c,
                    [
                        rx * out * az.cos(),
                        ry * self.between(-0.1, 0.4),
                        rx * out * az.sin(),
                    ],
                );
                let start = add(top, scale(dir(az, 0.0), r0 * 0.4));
                let limb = self.bough(start, r_top.max(3.0), end, 2.8, 0.25);
                nodes.extend(limb);
            }
            let targets = self.crown_points(c, rx, ry, -0.3, 2.3 + 0.5 * s, 0.3);
            let groups = targets.len() / 4;
            let clumps = Clumps {
                rx: (1.6 + 0.3 * s, 2.2 + 0.5 * s),
                flat: 0.6,
                lift: 0.2,
                holes: 0.2,
                share: 1.0,
            };
            self.crown(&mut nodes, &targets, groups, 0.2, clumps, leaf);
        }
        // Curtains: strands hanging from the underside of the crown, most and longest
        // round its rim, so the crown is a dome with a fringe rather than a box.
        let plan = &mut *self.plan;
        let (ox, oz) = (self.x, self.z);
        let dist =
            |x: i32, z: i32| ((x as f32 + 0.5 - ox).powi(2) + (z as f32 + 0.5 - oz).powi(2)).sqrt();
        let mut edge = 1.0f32;
        for x in plan.x0..plan.x0 + plan.sx {
            for z in plan.z0..plan.z0 + plan.sz {
                if plan.lowest_leaves(x, z).is_some() {
                    edge = edge.max(dist(x, z));
                }
            }
        }
        for x in plan.x0..plan.x0 + plan.sx {
            for z in plan.z0..plan.z0 + plan.sz {
                let Some(low) = plan.lowest_leaves(x, z) else {
                    continue;
                };
                let room = low - self.y - 1;
                let (rx, rz) = (x - self.cell.0, z - self.cell.1);
                let rim = (dist(x, z) / edge).clamp(0.0, 1.0);
                if room < 1 || unit3(self.seed, rx, 0, rz) > 0.12 + 0.6 * rim * rim {
                    continue;
                }
                let share = (0.25 + 0.6 * rim) * (0.6 + 0.4 * unit3(self.seed ^ 7, rx, 1, rz));
                let len = ((room as f32 * share).round() as i32).max(1);
                let seed = self.seed ^ ((rx as u64) << 32 | (rz as u32) as u64);
                plan.hang(x, low - 1, z, len.min(room), leaf(hash_bit(seed)));
            }
        }
    }

    /// Dead trees: a broken snag with stubs, or a bare, crooked skeleton of an oak or birch
    /// with a few tufts of bare twigs.
    fn dead(&mut self) {
        let s = self.s;
        if self.chance(0.35) {
            self.plan.wood = Wood::Birch as u8;
        }
        // Birch trunks stay slimmer.
        let thin = if self.plan.wood == Wood::Birch as u8 {
            0.6
        } else {
            1.0
        };
        let snag = s < 0.25 || self.chance(0.3);
        let moss = if self.chance(0.6) {
            self.rng.range(1, 2)
        } else {
            0
        };
        if snag {
            let h = (3.0 + 8.0 * s).round() as i32;
            let d = (10.0 + 24.0 * s) * thin;
            self.centre_trunk(d);
            let r0 = d / 32.0;
            let top = self.trunk(
                h,
                |t, level| r0 * (1.0 - 0.15 * t) * (1.0 + 0.4 * (-level / 0.8).exp()),
                |_| (0.0, 0.0),
                moss,
            );
            // A jagged break: a splinter or two on one side of the top.
            let c = super::plan::cell_of(top);
            let corner = self.rng.range(0, 3) as u8;
            self.plan
                .put(c[0], c[1] + 1, c[2], Piece::Quarter(4, corner));
            if self.chance(0.6) {
                self.plan
                    .put(c[0], c[1] + 2, c[2], Piece::Quarter(2, corner));
            }
            let stubs = self.rng.range(2, 4);
            self.stubs(stubs, 1.0, h as f32 - 0.5, r0);
            if s > 0.3 && self.chance(0.6) {
                let az = self.f() * TAU;
                let st = Style {
                    seg: 1.4,
                    wobble: 0.5,
                    droop: -0.05,
                    min_elev: -0.4,
                    forks: 0.0,
                    fork_angle: (0.5, 1.0),
                    fork_rise: 0.0,
                    fork_len: 0.5,
                    fork_r: 0.7,
                    depth: 0,
                };
                let start = self.axis(h as f32 * 0.7);
                self.grow(start, dir(az, 0.5), 2.0 + 3.0 * s, 3.5, 2.0, &st, 0);
            }
            self.tips.clear();
            return;
        }
        let h = 6.0 + 16.0 * s;
        let crown = 2.5 + 7.0 * s;
        let d = (8.0 + 30.0 * s.powf(1.5)) * thin;
        self.centre_trunk(d);
        let r0 = d / 32.0;
        let trunk_h = (h * self.between(0.35, 0.45)).round() as i32;
        let top = self.trunk(
            trunk_h,
            |t, level| r0 * (1.0 - 0.3 * t) * (1.0 + 0.4 * (-level / 0.9).exp()),
            |_| (0.0, 0.0),
            moss,
        );
        // The skeleton of a crown: crooked scaffolds, some snapped off short, and a sparse
        // spread of side limbs and twigs with a few tufts of bare twigs.
        let r_top = r0 * 0.7 * 16.0;
        let ry = ((h - trunk_h as f32) * 0.5).max(1.5);
        let rx = (crown - 1.0).clamp(1.5, MAX_REACH as f32 - 2.0);
        let c = [top[0], self.y as f32 + h - ry - 0.5, top[2]];
        let mut nodes = vec![Node {
            p: top,
            r: r_top,
            trunk: true,
        }];
        let k = self.rng.range(2, 4);
        let az0 = self.f() * TAU;
        for i in 0..k {
            let az = az0 + i as f32 * TAU / k as f32 + self.between(-0.4, 0.4);
            let (out, up) = if i == 0 {
                (self.between(0.1, 0.3), self.between(0.5, 0.8))
            } else {
                (self.between(0.5, 0.8), self.between(-0.3, 0.3))
            };
            let mut end = add(c, [rx * out * az.cos(), ry * up, rx * out * az.sin()]);
            if self.chance(0.3) {
                end = add(top, scale(sub(end, top), self.between(0.4, 0.7)));
            }
            let start = add(top, scale(dir(az, 0.0), r0 * 0.4));
            let limb = self.bough(start, (r_top * 0.65).max(3.0), end, 2.6, 0.06);
            nodes.extend(limb);
        }
        let mut targets = self.crown_points(c, rx, ry, -0.4, 2.8 + 0.4 * s, 0.45);
        let mut kept = Vec::new();
        for t in targets.drain(..) {
            if self.chance(0.6) {
                kept.push(t);
            }
        }
        let clumps = Clumps {
            rx: (0.8, 1.2),
            flat: 0.8,
            lift: 0.0,
            holes: 0.4,
            share: 0.3,
        };
        self.crown(&mut nodes, &kept, kept.len() / 3, 0.05, clumps, |_| {
            Leaf::Twigs as u8
        });
    }

    /// A shrub: a few short stems from one root under a low mound of leaves (oak, hawthorn
    /// or lilac).
    fn bush(&mut self) {
        let s = self.s;
        let leaf = match self.f() {
            f if f < 0.55 => Leaf::Oak,
            f if f < 0.85 => Leaf::Hawthorn,
            _ => Leaf::Lilac,
        } as u8;
        let rb = 1.1 + 2.0 * s;
        let hb = 1.0 + 1.8 * s;
        let base = self.axis(0.5);
        self.stem(
            &[(base, 2.0 + s), (add(base, [0.0, hb * 0.5, 0.0]), 1.8)],
            false,
        );
        let st = Style {
            seg: 1.0,
            wobble: 0.4,
            droop: -0.1,
            min_elev: -0.3,
            forks: 0.3,
            fork_angle: (0.5, 1.0),
            fork_rise: 0.1,
            fork_len: 0.6,
            fork_r: 0.8,
            depth: 1,
        };
        let n = 2 + (2.0 * s) as i32;
        let az0 = self.f() * TAU;
        // Stems leave from the second cell of the stump: one on the ground would sink into
        // the Conquest layer there and come apart from the bush.
        let fork = add(base, [0.0, 1.0, 0.0]);
        for i in 0..n {
            let az = az0 + i as f32 * TAU / n as f32 + self.between(-0.4, 0.4);
            let el = self.between(0.4, 0.9);
            self.grow(fork, dir(az, el), rb * 0.8, 2.4, 1.6, &st, 0);
        }
        let seed = self.seed;
        self.plan.clump(
            add(base, [0.0, hb * 0.55 - 0.5, 0.0]),
            rb,
            hb * 0.65 + 0.3,
            0.15,
            leaf,
            seed,
        );
        self.leaf_tips((1.0, 1.2), 0.9, 0.2, 0.2, move |_| leaf);
    }
}

/// Index of the point in `points` closest to `p`.
fn nearest(points: &[V3], p: V3) -> usize {
    (0..points.len())
        .min_by(|&a, &b| length(sub(points[a], p)).total_cmp(&length(sub(points[b], p))))
        .unwrap_or(0)
}

/// The node a limb towards `p` should leave from: the closest one, preferring nodes below
/// `p` (limbs climb as they go out) and branches over the trunk.
fn best_node(nodes: &[Node], p: V3) -> Node {
    let cost = |n: &Node| {
        length(sub(n.p, p)) + 0.6 * (n.p[1] - p[1]).max(0.0) + if n.trunk { 1.5 } else { 0.0 }
    };
    *nodes
        .iter()
        .min_by(|a, b| cost(a).total_cmp(&cost(b)))
        .expect("a crown has at least one node")
}

fn hash_bit(seed: u64) -> u64 {
    crate::noise::hash64(seed) & 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Species; 7] = [
        Species::Oak,
        Species::Birch,
        Species::Spruce,
        Species::Pine,
        Species::Willow,
        Species::Dead,
        Species::Bush,
    ];

    fn req(species: Species, size: f32, seed: u64) -> TreeRequest {
        TreeRequest {
            species,
            x: 100,
            y: 64,
            z: -40,
            size,
            seed,
        }
    }

    #[test]
    fn every_species_stays_in_reach_and_has_wood() {
        for species in ALL {
            for size in [0.0, 0.1, 0.2, 0.35, 0.5, 0.65, 0.8, 1.0] {
                for seed in 0..6 {
                    let r = req(species, size, seed * 7919 + 13);
                    let plan = plan(&r);
                    let mut wood = 0;
                    let mut top = 0;
                    for (x, y, z, c) in plan.cells() {
                        assert!(x.abs() <= MAX_REACH && z.abs() <= MAX_REACH);
                        assert!(y >= -BELOW);
                        if c.piece.is_wood() {
                            wood += 1;
                        }
                        top = top.max(y);
                    }
                    assert!(wood > 0, "{species:?} {size} has no wood");
                    let (h, _) = shape(species, size);
                    assert!(
                        top as f32 <= h + 5.0,
                        "{species:?} {size}: {top} tall, shape says {h}"
                    );
                }
            }
        }
    }

    #[test]
    fn plans_depend_only_on_the_request() {
        for species in ALL {
            let r = req(species, 0.7, 42);
            let a: Vec<_> = plan(&r).cells().collect();
            let b: Vec<_> = plan(&r).cells().collect();
            assert_eq!(a, b);
            let moved = TreeRequest { x: r.x + 37, ..r };
            let c: Vec<_> = plan(&moved).cells().collect();
            assert_eq!(a, c, "{species:?} changes shape when moved");
        }
    }

    #[test]
    fn big_trees_have_thick_trunks_and_leaves_clear_of_them() {
        let plan = plan(&req(Species::Oak, 1.0, 5));
        let base: Vec<_> = plan
            .cells()
            .filter(|c| c.1 == 0 && c.3.piece.is_wood())
            .collect();
        assert!(
            base.len() >= 9,
            "giant oak base is only {} cells",
            base.len()
        );
        assert!(base
            .iter()
            .any(|c| matches!(c.3.piece, Piece::Slab(..) | Piece::Quarter(..))));
        let leaves = plan
            .cells()
            .filter(|c| matches!(c.3.piece, Piece::Leaves(_)))
            .count();
        assert!(leaves > 300);
        // The lower half of the trunk shows: no leaves next to it.
        for (x, y, z, c) in plan.cells() {
            if matches!(c.piece, Piece::Leaves(_)) && y < 4 {
                assert!(x.abs() > 2 || z.abs() > 2);
            }
        }
    }

    #[test]
    fn branch_thickness_steps_down() {
        let plan = plan(&req(Species::Oak, 0.6, 9));
        let count = |t: Thick| {
            plan.cells()
                .filter(|c| c.3.piece == Piece::Branch(t))
                .count()
        };
        assert!(count(Thick::Large) > 0 && count(Thick::Medium) > 0 && count(Thick::Small) > 0);
    }
}
