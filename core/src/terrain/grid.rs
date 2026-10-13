//! Square grids of samples over the map, with smooth lookups in block coordinates, and a
//! deterministic helper that fills them on every core.

/// `n × n` nodes; node (i, j) sits on block coordinates (i · cell, j · cell).
#[derive(Clone, Debug)]
pub struct Field {
    pub n: usize,
    pub cell: f64,
    pub data: Vec<f32>,
}

impl Field {
    pub fn new(n: usize, cell: f64, value: f32) -> Self {
        Self {
            n,
            cell,
            data: vec![value; n * n],
        }
    }

    /// Fills every node from `f(i, j)` on all cores.
    pub fn from_fn(n: usize, cell: f64, f: impl Fn(usize, usize) -> f32 + Sync) -> Self {
        let mut data = vec![0.0; n * n];
        par_rows(&mut data, n, |j, row| {
            for (i, v) in row.iter_mut().enumerate() {
                *v = f(i, j);
            }
        });
        Self { n, cell, data }
    }

    #[inline]
    pub fn idx(&self, i: usize, j: usize) -> usize {
        j * self.n + i
    }

    #[inline]
    pub fn at(&self, i: usize, j: usize) -> f32 {
        self.data[j * self.n + i]
    }

    #[inline]
    fn clamped(&self, i: i64, j: i64) -> f32 {
        let m = self.n as i64 - 1;
        self.data[(j.clamp(0, m) * self.n as i64 + i.clamp(0, m)) as usize]
    }

    /// Grid position of a block coordinate, and its integer and fractional parts.
    #[inline]
    fn locate(&self, x: f64, z: f64) -> (i64, i64, f64, f64) {
        let max = (self.n - 1) as f64;
        let (gx, gz) = (
            (x / self.cell).clamp(0.0, max),
            (z / self.cell).clamp(0.0, max),
        );
        let (fx, fz) = (gx.floor(), gz.floor());
        (fx as i64, fz as i64, gx - fx, gz - fz)
    }

    pub fn bilinear(&self, x: f64, z: f64) -> f64 {
        let (i, j, tx, tz) = self.locate(x, z);
        let a = self.clamped(i, j) as f64;
        let b = self.clamped(i + 1, j) as f64;
        let c = self.clamped(i, j + 1) as f64;
        let d = self.clamped(i + 1, j + 1) as f64;
        let top = a + (b - a) * tx;
        let bottom = c + (d - c) * tx;
        top + (bottom - top) * tz
    }

    /// Catmull-Rom interpolation: smooth (no creases along grid lines) and exact at nodes.
    pub fn cubic(&self, x: f64, z: f64) -> f64 {
        let (i, j, tx, tz) = self.locate(x, z);
        let wx = catmull_weights(tx);
        let wz = catmull_weights(tz);
        let mut sum = 0.0;
        for (b, wb) in wz.iter().enumerate() {
            let row = j + b as i64 - 1;
            let mut r = 0.0;
            for (a, wa) in wx.iter().enumerate() {
                r += wa * self.clamped(i + a as i64 - 1, row) as f64;
            }
            sum += wb * r;
        }
        sum
    }

    /// Box blur of `radius` nodes, `passes` times (three passes are close to a Gaussian).
    pub fn blur(&mut self, radius: usize, passes: usize) {
        if radius == 0 {
            return;
        }
        let n = self.n;
        let mut tmp = vec![0.0f32; n * n];
        for _ in 0..passes {
            box_pass(&self.data, &mut tmp, n, radius, true);
            box_pass(&tmp, &mut self.data, n, radius, false);
        }
    }
}

#[inline]
fn catmull_weights(t: f64) -> [f64; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        0.5 * (-t3 + 2.0 * t2 - t),
        0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
        0.5 * (-3.0 * t3 + 4.0 * t2 + t),
        0.5 * (t3 - t2),
    ]
}

/// One box pass along x (`horizontal`) or z, with edge values extended.
fn box_pass(src: &[f32], dst: &mut [f32], n: usize, r: usize, horizontal: bool) {
    let norm = 1.0 / (2 * r + 1) as f32;
    let get = |line: usize, k: i64| {
        let k = k.clamp(0, n as i64 - 1) as usize;
        if horizontal {
            src[line * n + k]
        } else {
            src[k * n + line]
        }
    };
    for line in 0..n {
        let mut acc: f32 = (-(r as i64)..=r as i64).map(|k| get(line, k)).sum();
        for k in 0..n {
            let out = if horizontal {
                line * n + k
            } else {
                k * n + line
            };
            dst[out] = acc * norm;
            acc += get(line, k as i64 + r as i64 + 1) - get(line, k as i64 - r as i64);
        }
    }
}

/// Number of worker threads for whole-map work.
pub fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(32)
}

/// Calls `f(row_index, row)` for every row of a row-major `width`-wide buffer, on all cores.
/// Each row is written by exactly one call, so the result does not depend on scheduling.
pub fn par_rows<T: Send>(data: &mut [T], width: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = data.len() / width.max(1);
    let per = rows.div_ceil(threads()).max(1);
    std::thread::scope(|s| {
        for (c, block) in data.chunks_mut(per * width).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (k, row) in block.chunks_mut(width).enumerate() {
                    f(c * per + k, row);
                }
            });
        }
    });
}

/// Maps `items` to results on all cores, keeping the order.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let per = items.len().div_ceil(threads()).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = items
            .chunks(per)
            .map(|chunk| {
                let f = &f;
                s.spawn(move || chunk.iter().map(f).collect::<Vec<R>>())
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("terrain worker panicked"))
            .collect()
    })
}

#[inline]
pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Two-pass chamfer distance (in blocks) from every node to the nearest node where
/// `source` is true. Nodes with no source anywhere get `f32::MAX`.
pub fn distance(source: &[bool], n: usize, cell: f64) -> Vec<f32> {
    let (a, b) = (cell as f32, (cell * std::f64::consts::SQRT_2) as f32);
    let mut d: Vec<f32> = source
        .iter()
        .map(|&s| if s { 0.0 } else { f32::MAX / 4.0 })
        .collect();
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            let mut v = d[k];
            if i > 0 {
                v = v.min(d[k - 1] + a);
            }
            if j > 0 {
                v = v.min(d[k - n] + a);
                if i > 0 {
                    v = v.min(d[k - n - 1] + b);
                }
                if i + 1 < n {
                    v = v.min(d[k - n + 1] + b);
                }
            }
            d[k] = v;
        }
    }
    for j in (0..n).rev() {
        for i in (0..n).rev() {
            let k = j * n + i;
            let mut v = d[k];
            if i + 1 < n {
                v = v.min(d[k + 1] + a);
            }
            if j + 1 < n {
                v = v.min(d[k + n] + a);
                if i + 1 < n {
                    v = v.min(d[k + n + 1] + b);
                }
                if i > 0 {
                    v = v.min(d[k + n - 1] + b);
                }
            }
            d[k] = v;
        }
    }
    for v in &mut d {
        if *v >= f32::MAX / 8.0 {
            *v = f32::MAX;
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_hits_nodes() {
        let f = Field::from_fn(9, 8.0, |i, j| (i * 10 + j) as f32);
        assert_eq!(f.bilinear(16.0, 24.0), 23.0);
        assert!((f.cubic(16.0, 24.0) - 23.0).abs() < 1e-9);
        // Linear data stays linear.
        assert!((f.cubic(20.0, 24.0) - 28.0).abs() < 1e-9);
        assert!((f.bilinear(-50.0, 1000.0) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn blur_keeps_constant_fields() {
        let mut f = Field::new(17, 1.0, 3.0);
        f.blur(3, 3);
        assert!(f.data.iter().all(|&v| (v - 3.0).abs() < 1e-5));
    }

    #[test]
    fn chamfer_distance() {
        let n = 5;
        let mut s = vec![false; n * n];
        s[0] = true;
        let d = distance(&s, n, 2.0);
        assert_eq!(d[4], 8.0);
        assert!((d[n * 4 + 4] - 8.0 * 2f32.sqrt()).abs() < 1e-4);
    }

    #[test]
    fn par_rows_covers_everything() {
        let mut v = vec![0usize; 37 * 11];
        par_rows(&mut v, 11, |j, row| {
            for (i, x) in row.iter_mut().enumerate() {
                *x = j * 11 + i;
            }
        });
        assert!(v.iter().enumerate().all(|(k, &x)| k == x));
        let m = par_map(&[1, 2, 3, 4, 5, 6, 7], |x| x * 2);
        assert_eq!(m, vec![2, 4, 6, 8, 10, 12, 14]);
    }
}
