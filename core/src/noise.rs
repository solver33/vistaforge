//! Small deterministic noise and random helpers (no external crates, same output on every OS).

/// SplitMix64 finaliser: a good 64-bit hash.
pub fn hash64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

pub fn hash2(seed: u64, x: i64, z: i64) -> u64 {
    hash64(
        seed ^ hash64(
            (x as u64).wrapping_mul(0x632B_E59B_D9B4_E019)
                ^ (z as u64).wrapping_mul(0x8CB9_2BA7_2F3D_8DD7),
        ),
    )
}

/// Uniform value in [0, 1) for an integer lattice point.
pub fn unit2(seed: u64, x: i64, z: i64) -> f64 {
    (hash2(seed, x, z) >> 11) as f64 / (1u64 << 53) as f64
}

fn smooth(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise in [-1, 1] with quintic interpolation.
pub fn value2(seed: u64, x: f64, z: f64) -> f64 {
    let (x0, z0) = (x.floor(), z.floor());
    let (tx, tz) = (smooth(x - x0), smooth(z - z0));
    let (ix, iz) = (x0 as i64, z0 as i64);
    let v = |dx: i64, dz: i64| unit2(seed, ix + dx, iz + dz) * 2.0 - 1.0;
    let a = v(0, 0) + (v(1, 0) - v(0, 0)) * tx;
    let b = v(0, 1) + (v(1, 1) - v(0, 1)) * tx;
    a + (b - a) * tz
}

/// Fractal sum of `octaves` layers of value noise, roughly in [-1, 1].
pub fn fbm2(seed: u64, x: f64, z: f64, octaves: u32) -> f64 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * value2(seed.wrapping_add(o as u64 * 1013), x * freq, z * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Tiny sequential generator for placement decisions.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(hash64(seed))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        hash64(self.0)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform integer in [lo, hi].
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next_u64() % (hi - lo + 1) as u64) as i32
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next_u64() % items.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_bounded_and_deterministic() {
        for i in 0..1000 {
            let x = i as f64 * 0.37;
            let v = fbm2(7, x, -x * 0.5, 4);
            assert!((-1.0..=1.0).contains(&v));
            assert_eq!(v, fbm2(7, x, -x * 0.5, 4));
        }
    }
}
