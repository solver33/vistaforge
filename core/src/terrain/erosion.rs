//! Erosion. Two scales:
//!
//! - **Stream power** on the macro grid (8 blocks): rivers cut valleys in proportion to the
//!   area they drain and the slope, which turns noise into branching valley networks with
//!   sharp ridges between them.
//! - **Droplets** on a 2-block grid (full quality only): rain droplets run downhill, pick up
//!   ground on steep runs and drop it where they slow, carving rills and gullies and laying
//!   fans at the foot of slopes.
//!
//! Both are deterministic: the macro pass is sequential, and the droplet pass runs tiles in
//! parallel, each with its own seeded droplets, on its own copy of the ground.

use crate::noise::Rng;

use super::grid::{par_map, Field};
use super::hydro::{drainage, flood, NEIGHBOURS, NONE};

/// Cuts valleys with the stream power law (implicit scheme of Braun & Willett). `erode` is
/// the per-node erodibility; `seeds` builds the flood seeds for the current heights.
pub fn stream_power(
    h: &mut [f32],
    n: usize,
    erode: &[f32],
    rain: &[f32],
    iterations: usize,
    k: f32,
    seeds: impl Fn(&[f32]) -> Vec<(u32, f32)>,
) {
    let mut routing = None;
    for it in 0..iterations {
        // Drainage barely changes from one step to the next, so it is rebuilt every other
        // step (the order stays valid: receivers are still visited before their donors).
        if it % 2 == 0 {
            let f = flood(h, n, &seeds(h));
            let area = drainage(&f, rain);
            routing = Some((f, area));
        }
        let (f, area) = routing.as_ref().unwrap();
        for &c in &f.order {
            let c = c as usize;
            let r = f.recv[c];
            if r == NONE {
                continue;
            }
            let r = r as usize;
            if h[c] <= h[r] {
                continue; // inside a filled hollow: water stands, nothing is cut
            }
            let dist = if c % n != r % n && c / n != r / n {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let fct = k * erode[c] * area[c].sqrt() / dist;
            h[c] = (h[c] + fct * h[r]) / (1.0 + fct);
        }
    }
}

/// Moves ground off slopes steeper than `talus` (height per block between nodes `cell`
/// apart), like scree settling. Jacobi steps, so the result does not depend on order.
pub fn thermal(h: &mut [f32], n: usize, cell: f32, talus: f32, iterations: usize) {
    let mut delta = vec![0.0f32; n * n];
    for _ in 0..iterations {
        delta.iter_mut().for_each(|d| *d = 0.0);
        for j in 0..n {
            for i in 0..n {
                let k = j * n + i;
                for &(di, dj, dist) in &NEIGHBOURS[..4] {
                    let (a, b) = (i as i32 + di, j as i32 + dj);
                    if a < 0 || b < 0 || a >= n as i32 || b >= n as i32 {
                        continue;
                    }
                    let m = b as usize * n + a as usize;
                    let limit = talus * cell * dist;
                    let diff = h[k] - h[m];
                    if diff > limit {
                        let mv = (diff - limit) * 0.12;
                        delta[k] -= mv;
                        delta[m] += mv;
                    }
                }
            }
        }
        for (v, d) in h.iter_mut().zip(&delta) {
            *v += d;
        }
    }
}

/// Droplet erosion settings, in grid units (heights are divided by `scale` first).
#[derive(Clone, Copy, Debug)]
pub struct Droplets {
    /// Droplets per grid node.
    pub density: f64,
    /// Block height that counts as 1 unit inside the simulation.
    pub scale: f32,
    pub inertia: f32,
    pub capacity: f32,
    pub min_capacity: f32,
    pub erode: f32,
    pub deposit: f32,
    pub evaporate: f32,
    pub gravity: f32,
    pub steps: usize,
    /// Most a single brush stroke may remove from one node, in units.
    pub max_cut: f32,
}

impl Default for Droplets {
    fn default() -> Self {
        Self {
            density: 0.6,
            scale: 24.0,
            inertia: 0.08,
            capacity: 6.0,
            min_capacity: 0.002,
            erode: 0.25,
            deposit: 0.2,
            evaporate: 0.02,
            gravity: 4.0,
            steps: 56,
            max_cut: 0.04,
        }
    }
}

/// Erosion brush: offsets within `RADIUS` nodes and their normalised weights.
const RADIUS: i32 = 2;

fn brush() -> Vec<(i32, i32, f32)> {
    let mut out = Vec::new();
    let r = RADIUS as f32 + 0.5;
    for dj in -RADIUS..=RADIUS {
        for di in -RADIUS..=RADIUS {
            let d = ((di * di + dj * dj) as f32).sqrt();
            if d < r {
                out.push((di, dj, r - d));
            }
        }
    }
    let total: f32 = out.iter().map(|b| b.2).sum();
    for b in &mut out {
        b.2 /= total;
    }
    out
}

/// A window of the map simulated on its own. Droplets start inside the core
/// (`ci0..ci1`, `cj0..cj1`, window coordinates) and may run out into the apron around it.
struct Tile {
    i0: usize,
    j0: usize,
    w: usize,
    d: usize,
    core: [usize; 4],
    seed: u64,
}

/// Height and gradient at a point, bilinear over the 4 surrounding nodes.
#[inline]
fn sample(h: &[f32], w: usize, x: f32, z: f32) -> (f32, f32, f32) {
    let (i, j) = (x as usize, z as usize);
    let (u, v) = (x - i as f32, z - j as f32);
    let k = j * w + i;
    let (a, b, c, d) = (h[k], h[k + 1], h[k + w], h[k + w + 1]);
    let gx = (b - a) * (1.0 - v) + (d - c) * v;
    let gz = (c - a) * (1.0 - u) + (d - b) * u;
    let height = a * (1.0 - u) * (1.0 - v) + b * u * (1.0 - v) + c * (1.0 - u) * v + d * u * v;
    (height, gx, gz)
}

fn erode_tile(h: &mut [f32], visits: &mut [u32], t: &Tile, p: &Droplets) {
    let (w, d) = (t.w, t.d);
    let brush = brush();
    let margin = RADIUS as f32 + 1.0;
    let (xmax, zmax) = (w as f32 - margin - 1.0, d as f32 - margin - 1.0);
    let [ci0, cj0, ci1, cj1] = t.core;
    let (x0, z0) = ((ci0 as f32).max(margin), (cj0 as f32).max(margin));
    let (x1, z1) = ((ci1 as f32).min(xmax), (cj1 as f32).min(zmax));
    if x1 <= x0 || z1 <= z0 {
        return;
    }
    let count = (((ci1 - ci0) * (cj1 - cj0)) as f64 * p.density) as usize;
    let mut rng = Rng::new(t.seed);
    for _ in 0..count {
        let mut x = x0 + rng.unit() as f32 * (x1 - x0);
        let mut z = z0 + rng.unit() as f32 * (z1 - z0);
        let (mut dx, mut dz) = (0.0f32, 0.0f32);
        let (mut speed, mut water, mut sediment) = (1.0f32, 1.0f32, 0.0f32);
        for _ in 0..p.steps {
            let (i, j) = (x as usize, z as usize);
            let (u, v) = (x - i as f32, z - j as f32);
            let (height, gx, gz) = sample(h, w, x, z);
            dx = dx * p.inertia - gx * (1.0 - p.inertia);
            dz = dz * p.inertia - gz * (1.0 - p.inertia);
            let len = (dx * dx + dz * dz).sqrt();
            if len < 1e-6 {
                break;
            }
            dx /= len;
            dz /= len;
            x += dx;
            z += dz;
            visits[j * w + i] += 1;
            if x < margin || z < margin || x > xmax || z > zmax {
                break;
            }
            let dh = sample(h, w, x, z).0 - height;
            let capacity = (-dh * speed * water * p.capacity).max(p.min_capacity);
            if sediment > capacity || dh > 0.0 {
                let amount = if dh > 0.0 {
                    dh.min(sediment)
                } else {
                    (sediment - capacity) * p.deposit
                };
                sediment -= amount;
                let k = j * w + i;
                h[k] += amount * (1.0 - u) * (1.0 - v);
                h[k + 1] += amount * u * (1.0 - v);
                h[k + w] += amount * (1.0 - u) * v;
                h[k + w + 1] += amount * u * v;
            } else {
                let amount = ((capacity - sediment) * p.erode).min(-dh);
                let mut taken = 0.0;
                for &(di, dj, wt) in &brush {
                    let k = (j as i32 + dj) as usize * w + (i as i32 + di) as usize;
                    let cut = (amount * wt).min(p.max_cut);
                    h[k] -= cut;
                    taken += cut;
                }
                sediment += taken;
            }
            speed = (speed * speed - dh * p.gravity).max(0.0).sqrt();
            water *= 1.0 - p.evaporate;
        }
    }
}

/// Runs droplet erosion over `field` (heights in blocks). Returns how many droplet steps
/// passed through each node.
pub fn droplets(field: &mut Field, p: &Droplets, seed: u64) -> Vec<u32> {
    let n = field.n;
    let scale = p.scale;
    for v in &mut field.data {
        *v /= scale;
    }
    let mut visits = vec![0u32; n * n];
    // Tiles run in parallel on copies of the map. Each one simulates a window reaching
    // APRON nodes past its core, and the changes of all windows are added up, so gullies
    // run across tile borders instead of stopping at them.
    const TILE: usize = 160;
    const APRON: usize = 48;
    // Two passes with the tile grid shifted by half a tile, so no border stays in one place.
    let passes = [0usize, TILE / 2];
    let per_pass = Droplets {
        density: p.density / passes.len() as f64,
        ..*p
    };
    for (pass, &offset) in passes.iter().enumerate() {
        let mut starts = vec![0usize];
        let mut s = if offset == 0 { TILE } else { offset };
        while s < n {
            starts.push(s);
            s += TILE;
        }
        let mut tiles = Vec::new();
        for (tj, &j0) in starts.iter().enumerate() {
            for (ti, &i0) in starts.iter().enumerate() {
                let i1 = starts.get(ti + 1).copied().unwrap_or(n);
                let j1 = starts.get(tj + 1).copied().unwrap_or(n);
                let (wi0, wj0) = (i0.saturating_sub(APRON), j0.saturating_sub(APRON));
                let (wi1, wj1) = ((i1 + APRON).min(n), (j1 + APRON).min(n));
                tiles.push(Tile {
                    i0: wi0,
                    j0: wj0,
                    w: wi1 - wi0,
                    d: wj1 - wj0,
                    core: [i0 - wi0, j0 - wj0, i1 - wi0, j1 - wj0],
                    seed: seed ^ ((pass as u64) << 40) ^ ((ti as u64) << 20) ^ tj as u64,
                });
            }
        }
        let data = &field.data;
        let results = par_map(&tiles, |t| {
            let mut local = Vec::with_capacity(t.w * t.d);
            for j in t.j0..t.j0 + t.d {
                local.extend_from_slice(&data[j * n + t.i0..j * n + t.i0 + t.w]);
            }
            let mut v = vec![0u32; t.w * t.d];
            erode_tile(&mut local, &mut v, t, &per_pass);
            // Keep only the change, so overlapping windows can be added up.
            for j in 0..t.d {
                let row = (t.j0 + j) * n + t.i0;
                for (a, b) in local[j * t.w..(j + 1) * t.w]
                    .iter_mut()
                    .zip(&data[row..row + t.w])
                {
                    *a -= b;
                }
            }
            (local, v)
        });
        for (t, (delta, v)) in tiles.iter().zip(results) {
            for r in 0..t.d {
                let row = (t.j0 + r) * n + t.i0;
                for (a, b) in field.data[row..row + t.w]
                    .iter_mut()
                    .zip(&delta[r * t.w..(r + 1) * t.w])
                {
                    *a += b;
                }
                for (a, b) in visits[row..row + t.w]
                    .iter_mut()
                    .zip(&v[r * t.w..(r + 1) * t.w])
                {
                    *a += b;
                }
            }
        }
    }
    for v in &mut field.data {
        *v *= scale;
    }
    visits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::hydro::seeds;

    #[test]
    fn stream_power_cuts_valleys_but_keeps_peaks() {
        let n = 33;
        let mut h: Vec<f32> = (0..n * n)
            .map(|k| {
                let (i, j) = ((k % n) as f32, (k / n) as f32);
                40.0 - ((i - 16.0).powi(2) + (j - 16.0).powi(2)).sqrt()
                    + ((i * 1.7).sin() + (j * 1.3).cos())
            })
            .collect();
        let before = h.clone();
        let ones = vec![1.0; n * n];
        stream_power(&mut h, n, &ones, &ones, 10, 0.02, |h| {
            seeds(h, n, None, 0.0)
        });
        assert!(h.iter().zip(&before).all(|(a, b)| a <= b));
        let lowered: f32 = h.iter().zip(&before).map(|(a, b)| b - a).sum();
        assert!(lowered > 1.0);
        // Determinism.
        let mut again = before.clone();
        stream_power(&mut again, n, &ones, &ones, 10, 0.02, |h| {
            seeds(h, n, None, 0.0)
        });
        assert_eq!(again, h);
    }

    #[test]
    fn droplets_are_deterministic_and_conserve_roughly() {
        let n = 200;
        let mk = || {
            Field::from_fn(n, 2.0, |i, j| {
                60.0 + i as f32 * 0.4 + ((j as f32) * 0.21).sin() * 3.0
            })
        };
        let mut a = mk();
        let va = droplets(&mut a, &Droplets::default(), 7);
        let mut b = mk();
        let vb = droplets(&mut b, &Droplets::default(), 7);
        assert_eq!(a.data, b.data);
        assert_eq!(va, vb);
        let orig = mk();
        let changed = a
            .data
            .iter()
            .zip(&orig.data)
            .filter(|(x, y)| (*x - *y).abs() > 0.01)
            .count();
        assert!(changed > n * n / 4);
        let net: f32 = a.data.iter().zip(&orig.data).map(|(x, y)| x - y).sum();
        let moved: f32 = a
            .data
            .iter()
            .zip(&orig.data)
            .map(|(x, y)| (x - y).abs())
            .sum();
        assert!(net.abs() < moved, "{net} {moved}");
    }
}
