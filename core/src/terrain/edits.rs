//! Hand-placed landforms ([`ShapeEdit`]). Hills, mountains and ridges go in before erosion,
//! so rain cuts valleys into them like into the rest of the land. Plateaus, lakes and drawn
//! rivers go in after, so they keep the shape the user asked for, and before the water is
//! worked out, so rivers route around them and lakes fill them.

use crate::biome::Biome;
use crate::settings::ShapeEdit;

use super::grid::{lerp, smoothstep, Field};
use super::noise::{fbm, ridged};
use super::rivers::Trace;

/// Calls `f(i, j, x, z)` for every node within `reach` blocks of (x, z).
fn around(
    field: (usize, f64),
    x: f64,
    z: f64,
    reach: f64,
    mut f: impl FnMut(usize, usize, f64, f64),
) {
    let (n, c) = (field.0 as i64, field.1);
    let (i0, i1) = (
        ((x - reach) / c).floor() as i64,
        ((x + reach) / c).ceil() as i64,
    );
    let (j0, j1) = (
        ((z - reach) / c).floor() as i64,
        ((z + reach) / c).ceil() as i64,
    );
    for j in j0.max(0)..=j1.min(n - 1) {
        for i in i0.max(0)..=i1.min(n - 1) {
            f(i as usize, j as usize, i as f64 * c, j as f64 * c);
        }
    }
}

/// A ragged radius: ±`amount` of `r`, varying smoothly around the shape.
fn ragged(seed: u64, x: f64, z: f64, r: f64, amount: f64) -> f64 {
    r * (1.0 + amount * fbm(seed, x / (r * 0.6 + 8.0), z / (r * 0.6 + 8.0), 2, 0.5))
}

/// Closest point on a polyline: (distance, index of the segment, fraction along it,
/// fraction of the whole length).
fn polyline(points: &[[f32; 2]], x: f64, z: f64) -> (f64, usize, f64, f64) {
    let mut lengths = vec![0.0];
    for w in points.windows(2) {
        let d = ((w[1][0] - w[0][0]) as f64).hypot((w[1][1] - w[0][1]) as f64);
        lengths.push(lengths.last().unwrap() + d);
    }
    let total = lengths.last().copied().unwrap_or(0.0).max(1e-6);
    let mut best = (f64::MAX, 0, 0.0, 0.0);
    for (k, w) in points.windows(2).enumerate() {
        let (ax, az) = (w[0][0] as f64, w[0][1] as f64);
        let (dx, dz) = (w[1][0] as f64 - ax, w[1][1] as f64 - az);
        let len2 = dx * dx + dz * dz;
        let t = if len2 > 0.0 {
            (((x - ax) * dx + (z - az) * dz) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = (x - ax - dx * t).hypot(z - az - dz * t);
        if d < best.0 {
            best = (d, k, t, (lengths[k] + t * len2.sqrt()) / total);
        }
    }
    if points.len() == 1 {
        let d = (x - points[0][0] as f64).hypot(z - points[0][1] as f64);
        best = (d, 0, 0.0, 0.0);
    }
    best
}

/// Hills, mountains and ridges. `lift` records how much mountain was added, so the biome
/// layout can put rock and snow on hand-made peaks.
pub fn landforms(h: &mut Field, lift: &mut Field, edits: &[ShapeEdit], seed: u64) {
    for (e, edit) in edits.iter().enumerate() {
        let s = seed ^ (e as u64 + 1).wrapping_mul(0xED17);
        match edit {
            ShapeEdit::Hill {
                x,
                z,
                radius,
                height,
            } => {
                let (x, z, r) = (*x as f64, *z as f64, (*radius as f64).max(8.0));
                around((h.n, h.cell), x, z, r * 1.2, |i, j, px, pz| {
                    let d = (px - x).hypot(pz - z) / ragged(s, px, pz, r, 0.12);
                    if d < 1.0 {
                        let b = 1.0 - d * d;
                        let k = h.idx(i, j);
                        h.data[k] += (*height as f64 * b * b) as f32;
                    }
                });
            }
            ShapeEdit::Mountain {
                x,
                z,
                radius,
                height,
            } => {
                let (x, z, r) = (*x as f64, *z as f64, (*radius as f64).max(16.0));
                around((h.n, h.cell), x, z, r * 1.2, |i, j, px, pz| {
                    let d = (px - x).hypot(pz - z) / ragged(s, px, pz, r, 0.15);
                    if d < 1.0 {
                        let profile = (1.0 - d).powf(1.6) * (1.0 + 0.6 * d);
                        let crest = ridged(s ^ 0x41, px / (r * 0.45), pz / (r * 0.45), 4);
                        let add = *height as f64 * profile * (0.72 + 0.45 * (crest - 0.4));
                        let k = h.idx(i, j);
                        h.data[k] += add as f32;
                        lift.data[k] += add as f32;
                    }
                });
            }
            ShapeEdit::Ridge {
                points,
                width,
                height,
            } => {
                if points.is_empty() {
                    continue;
                }
                let half = (*width as f64 / 2.0).max(8.0);
                let (lo, hi) = bounds(points, half * 1.3);
                let (cx, cz) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
                let reach = (hi.0 - lo.0).max(hi.1 - lo.1) / 2.0 + 1.0;
                around((h.n, h.cell), cx, cz, reach, |i, j, px, pz| {
                    let (d, _, _, along) = polyline(points, px, pz);
                    let d = d / ragged(s, px, pz, half, 0.15);
                    if d < 1.0 {
                        let ends = smoothstep(0.0, 0.18, along) * smoothstep(1.0, 0.82, along);
                        let profile = (1.0 - d * d).powf(1.3) * (0.35 + 0.65 * ends);
                        let crest = ridged(s ^ 0x42, px / (half * 0.8), pz / (half * 0.8), 4);
                        let add = *height as f64 * profile * (0.75 + 0.4 * (crest - 0.4));
                        let k = h.idx(i, j);
                        h.data[k] += add as f32;
                        lift.data[k] += add as f32;
                    }
                });
            }
            _ => {}
        }
    }
}

fn bounds(points: &[[f32; 2]], pad: f64) -> ((f64, f64), (f64, f64)) {
    let mut lo = (f64::MAX, f64::MAX);
    let mut hi = (f64::MIN, f64::MIN);
    for p in points {
        lo = (lo.0.min(p[0] as f64), lo.1.min(p[1] as f64));
        hi = (hi.0.max(p[0] as f64), hi.1.max(p[1] as f64));
    }
    ((lo.0 - pad, lo.1 - pad), (hi.0 + pad, hi.1 + pad))
}

/// Carves a lake bowl with a watertight rim, so the water fills it to `surface`.
fn lake(h: &mut Field, s: u64, x: f64, z: f64, r: f64, depth: f64) {
    let surface = h.bilinear(x, z).round();
    let rim = (r * 0.35).max(2.5 * h.cell);
    around((h.n, h.cell), x, z, r * 1.2 + rim, |i, j, px, pz| {
        let rr = ragged(s, px, pz, r, 0.18);
        let d = (px - x).hypot(pz - z);
        let k = h.idx(i, j);
        let v = h.data[k] as f64;
        let v = if d < rr {
            let bowl = 1.0 - (d / rr).powi(2);
            v.min(surface - 0.6 - depth * bowl.powf(0.8))
        } else if d < rr + rim {
            v.max(surface + 0.6 + 0.08 * (d - rr))
        } else {
            v
        };
        h.data[k] = v as f32;
    });
}

/// Nodes inside the lakes the user placed, which erosion must not drain.
pub fn placed_lakes(edits: &[ShapeEdit], n: usize, cell: f64) -> Vec<bool> {
    let mut keep = vec![false; n * n];
    for edit in edits {
        let (x, z, r) = match edit {
            ShapeEdit::Lake { x, z, radius, .. } => (*x, *z, *radius),
            ShapeEdit::BiomeZone {
                x,
                z,
                radius,
                biome: Biome::Lake,
            } => (*x, *z, *radius),
            _ => continue,
        };
        let (x, z, r) = (x as f64, z as f64, r as f64 * 0.7);
        around((n, cell), x, z, r, |i, j, px, pz| {
            if (px - x).hypot(pz - z) < r {
                keep[j * n + i] = true;
            }
        });
    }
    keep
}

/// Plateaus and lakes, after erosion.
pub fn late(h: &mut Field, edits: &[ShapeEdit], seed: u64) {
    for (e, edit) in edits.iter().enumerate() {
        let s = seed ^ (e as u64 + 1).wrapping_mul(0x1A7E);
        match edit {
            ShapeEdit::Plateau {
                x,
                z,
                radius,
                level,
            } => {
                let (x, z, r) = (*x as f64, *z as f64, (*radius as f64).max(8.0));
                around((h.n, h.cell), x, z, r * 1.3, |i, j, px, pz| {
                    let d = (px - x).hypot(pz - z) / ragged(s, px, pz, r, 0.12);
                    let w = 1.0 - smoothstep(0.78, 1.08, d);
                    if w > 0.0 {
                        let top = *level as f64 + 0.8 * fbm(s ^ 3, px / 30.0, pz / 30.0, 2, 0.5);
                        let k = h.idx(i, j);
                        h.data[k] = lerp(h.data[k] as f64, top, w) as f32;
                    }
                });
            }
            ShapeEdit::Lake {
                x,
                z,
                radius,
                depth,
            } => lake(
                h,
                s,
                *x as f64,
                *z as f64,
                (*radius as f64).max(6.0),
                (*depth as f64).max(1.0),
            ),
            ShapeEdit::BiomeZone {
                x,
                z,
                radius,
                biome: Biome::Lake,
            } => {
                let r = (*radius as f64).max(6.0);
                lake(h, s, *x as f64, *z as f64, r, (r * 0.08).clamp(3.0, 10.0));
            }
            _ => {}
        }
    }
}

/// Drawn rivers: cuts a falling channel along each into the macro ground so the drainage
/// follows it, marks the nodes it covers, and returns its course.
pub fn rivers(h: &mut Field, edits: &[ShapeEdit], drawn: &mut [u32]) -> Vec<Trace> {
    let mut out = Vec::new();
    for edit in edits {
        let ShapeEdit::River {
            points,
            width,
            depth,
        } = edit
        else {
            continue;
        };
        if points.len() < 2 {
            continue;
        }
        let half = (*width as f64 / 2.0).clamp(0.8, 40.0);
        let depth = (*depth as f64).clamp(0.5, 12.0);
        // Points every few blocks along the drawn line.
        let mut pts = Vec::new();
        for w in points.windows(2) {
            let (a, b) = (
                (w[0][0] as f64, w[0][1] as f64),
                (w[1][0] as f64, w[1][1] as f64),
            );
            let steps = ((b.0 - a.0).hypot(b.1 - a.1) / 6.0).ceil().max(1.0) as usize;
            for k in 0..steps {
                let t = k as f64 / steps as f64;
                pts.push((lerp(a.0, b.0, t), lerp(a.1, b.1, t)));
            }
        }
        let last = points[points.len() - 1];
        pts.push((last[0] as f64, last[1] as f64));
        let mut surf = Vec::with_capacity(pts.len());
        for p in &pts {
            let ground = h.bilinear(p.0, p.1) - 0.6 - 0.12 * half;
            let prev = surf.last().copied().unwrap_or(f64::MAX);
            surf.push(ground.min(prev));
        }
        // Lower the macro ground to the channel, with gentle sides.
        let id = out.len() as u32;
        let reach = half + 24.0;
        let (lo, hi) = bounds(points, reach);
        let (cx, cz) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
        let r = (hi.0 - lo.0).max(hi.1 - lo.1) / 2.0 + 1.0;
        let course: Vec<[f32; 2]> = pts.iter().map(|p| [p.0 as f32, p.1 as f32]).collect();
        let cell = h.cell;
        around((h.n, h.cell), cx, cz, r, |i, j, px, pz| {
            let (d, seg, t, _) = polyline(&course, px, pz);
            if d > reach {
                return;
            }
            let level = lerp(surf[seg], surf[(seg + 1).min(surf.len() - 1)], t);
            let k = h.idx(i, j);
            let allowed = level + 0.3 + 0.35 * (d - half).max(0.0);
            h.data[k] = h.data[k].min(allowed as f32);
            if d <= half + cell {
                drawn[k] = id;
            }
        });
        out.push(Trace {
            half: vec![half; pts.len()],
            depth: vec![depth; pts.len()],
            pts,
            surf,
            joins: None,
            meander: 0.3,
        });
    }
    out
}

/// Biome zones, for painting biomes onto columns.
#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub x: f64,
    pub z: f64,
    pub r: f64,
    pub biome: Biome,
    pub seed: u64,
}

impl Zone {
    /// How far inside the zone a column is: 1 well inside, 0 at or past the edge.
    pub fn inside(&self, x: f64, z: f64) -> f64 {
        let d = (x - self.x).hypot(z - self.z) / ragged(self.seed, x, z, self.r, 0.15);
        smoothstep(1.0, 0.85, d)
    }
}

pub fn zones(edits: &[ShapeEdit], seed: u64) -> Vec<Zone> {
    edits
        .iter()
        .filter_map(|edit| match edit {
            ShapeEdit::BiomeZone {
                x,
                z,
                radius,
                biome,
            } => Some(Zone {
                x: *x as f64,
                z: *z as f64,
                r: (*radius as f64).max(4.0),
                biome: *biome,
                seed: seed ^ 0x2087,
            }),
            _ => None,
        })
        .collect()
}
