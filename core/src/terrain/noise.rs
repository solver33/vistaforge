//! Gradient noise for landforms. Value noise (in `crate::noise`) shows its lattice on large
//! smooth shapes; gradient noise with rotated octaves does not.

use crate::noise::hash2;

/// 16 unit gradients at (k + 0.5) · 22.5°, so none lines up with the lattice.
const GRADS: [(f64, f64); 16] = [
    (0.980_785_280_403_230_4, 0.195_090_322_016_128_3),
    (0.831_469_612_302_545_2, 0.555_570_233_019_602_2),
    (0.555_570_233_019_602_2, 0.831_469_612_302_545_2),
    (0.195_090_322_016_128_3, 0.980_785_280_403_230_4),
    (-0.195_090_322_016_128_3, 0.980_785_280_403_230_4),
    (-0.555_570_233_019_602_2, 0.831_469_612_302_545_2),
    (-0.831_469_612_302_545_2, 0.555_570_233_019_602_2),
    (-0.980_785_280_403_230_4, 0.195_090_322_016_128_3),
    (-0.980_785_280_403_230_4, -0.195_090_322_016_128_3),
    (-0.831_469_612_302_545_2, -0.555_570_233_019_602_2),
    (-0.555_570_233_019_602_2, -0.831_469_612_302_545_2),
    (-0.195_090_322_016_128_3, -0.980_785_280_403_230_4),
    (0.195_090_322_016_128_3, -0.980_785_280_403_230_4),
    (0.555_570_233_019_602_2, -0.831_469_612_302_545_2),
    (0.831_469_612_302_545_2, -0.555_570_233_019_602_2),
    (0.980_785_280_403_230_4, -0.195_090_322_016_128_3),
];

#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// 2D gradient (Perlin) noise, roughly in [-1, 1].
#[inline]
pub fn perlin(seed: u64, x: f64, z: f64) -> f64 {
    let g = &GRADS;
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (ix, iz) = (x0 as i64, z0 as i64);
    let dot = |dx: i64, dz: i64| {
        let (gx, gz) = g[(hash2(seed, ix + dx, iz + dz) >> 60) as usize];
        gx * (fx - dx as f64) + gz * (fz - dz as f64)
    };
    let (u, v) = (fade(fx), fade(fz));
    let a = dot(0, 0) + u * (dot(1, 0) - dot(0, 0));
    let b = dot(0, 1) + u * (dot(1, 1) - dot(0, 1));
    (a + v * (b - a)) * std::f64::consts::SQRT_2
}

/// Each octave is turned by about 37° so the octaves' lattices never line up.
const ROT: (f64, f64) = (0.798_635_510_047_292_9, 0.601_815_023_152_048_3);
const LACUNARITY: f64 = 2.03;

#[inline]
fn rotate(x: f64, z: f64) -> (f64, f64) {
    (
        (x * ROT.0 - z * ROT.1) * LACUNARITY,
        (x * ROT.1 + z * ROT.0) * LACUNARITY,
    )
}

/// Fractal sum of gradient noise; `gain` is the amplitude ratio between octaves.
/// Roughly in [-1, 1].
pub fn fbm(seed: u64, x: f64, z: f64, octaves: u32, gain: f64) -> f64 {
    let (mut x, mut z) = (x, z);
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * perlin(seed.wrapping_add(o as u64 * 0x9E37), x, z);
        norm += amp;
        amp *= gain;
        (x, z) = rotate(x, z);
    }
    sum / norm
}

/// Ridged multifractal in [0, 1]: sharp crests where the noise crosses zero, and finer
/// ridges mostly on the crests (each octave is weighted by the one before).
pub fn ridged(seed: u64, x: f64, z: f64, octaves: u32) -> f64 {
    let (mut x, mut z) = (x, z);
    let (mut sum, mut amp, mut norm, mut weight) = (0.0, 1.0, 0.0, 1.0);
    for o in 0..octaves {
        let n = 1.0 - perlin(seed.wrapping_add(o as u64 * 0x7F4A), x, z).abs();
        let n = n * n * weight;
        weight = (n * 1.8).clamp(0.0, 1.0);
        sum += n * amp;
        norm += amp;
        amp *= 0.5;
        (x, z) = rotate(x, z);
    }
    (sum / norm).clamp(0.0, 1.0)
}

/// Domain warp: an offset of about ±`amount` blocks for a feature of size `scale`.
pub fn warp(seed: u64, x: f64, z: f64, scale: f64, amount: f64) -> (f64, f64) {
    let (sx, sz) = (x / scale, z / scale);
    (
        x + amount * fbm(seed ^ 0x5157, sx, sz, 3, 0.5),
        z + amount * fbm(seed ^ 0x8AD1, sx + 31.7, sz - 17.3, 3, 0.5),
    )
}

/// 1D gradient noise along a line (for river meanders), roughly in [-1, 1].
pub fn perlin1(seed: u64, t: f64) -> f64 {
    let t0 = t.floor();
    let f = t - t0;
    let i = t0 as i64;
    let g = |k: i64| (crate::noise::unit2(seed, i + k, 0) * 2.0 - 1.0) * (f - k as f64);
    let u = fade(f);
    (g(0) + u * (g(1) - g(0))) * 2.0
}

/// A cosine wave of period 1 made of parabolas (much cheaper, and as smooth as the eye
/// needs): the value and its derivative.
#[inline]
fn wave(t: f64) -> (f64, f64) {
    let u = t - (t + 0.5).floor();
    let a = u.abs();
    if a <= 0.25 {
        (1.0 - 16.0 * a * a, -32.0 * u)
    } else {
        let b = a - 0.5;
        (16.0 * b * b - 1.0, 32.0 * b * u.signum())
    }
}

/// One octave of an erosion filter: stripes that run along `(gx, gz)` (down the slope),
/// broken into short strokes around jittered points so they wander and fork. `(x, z)` is in
/// units of the stripe spacing. Returns the value (about -1..1) and its derivative.
pub fn gully(seed: u64, x: f64, z: f64, gx: f64, gz: f64) -> (f64, f64, f64) {
    let len = gx.hypot(gz);
    if len < 1e-9 {
        return (0.0, 0.0, 0.0);
    }
    // Across the slope: the stripes' phase changes this way.
    let (nx, nz) = (-gz / len, gx / len);
    let (ix, iz) = (x.floor() as i64, z.floor() as i64);
    let (fx, fz) = (x - ix as f64, z - iz as f64);
    let (mut v, mut dx, mut dz, mut wt) = (0.0, 0.0, 0.0, 0.0);
    for oz in -1..=1 {
        for ox in -1..=1 {
            let h = hash2(seed, ix + ox, iz + oz);
            let jx = (h & 0xFFFF) as f64 / 65536.0;
            let jz = ((h >> 16) & 0xFFFF) as f64 / 65536.0;
            let (px, pz) = (fx - ox as f64 - jx, fz - oz as f64 - jz);
            let d2 = px * px + pz * pz;
            if d2 >= 2.25 {
                continue;
            }
            let w = (1.0 - d2 / 2.25).powi(3);
            let (c, dc) = wave(px * nx + pz * nz);
            v += w * c;
            dx += w * dc * nx;
            dz += w * dc * nz;
            wt += w;
        }
    }
    if wt <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    (v / wt, dx / wt, dz / wt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_and_deterministic() {
        let mut lo: f64 = 0.0;
        let mut hi: f64 = 0.0;
        for i in 0..20_000 {
            let (x, z) = (i as f64 * 0.137, (i % 977) as f64 * 0.291 - 50.0);
            let p = perlin(3, x, z);
            lo = lo.min(p);
            hi = hi.max(p);
            assert_eq!(p, perlin(3, x, z));
            let f = fbm(9, x / 7.0, z / 7.0, 5, 0.5);
            assert!((-1.2..=1.2).contains(&f));
            let r = ridged(9, x / 7.0, z / 7.0, 5);
            assert!((0.0..=1.0).contains(&r));
            assert!(perlin1(4, x).abs() <= 1.01);
        }
        // Uses most of its range.
        assert!(lo < -0.6 && hi > 0.6, "{lo} {hi}");
    }
}
