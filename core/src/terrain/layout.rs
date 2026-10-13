//! Where each biome goes. The biome mix is laid out in "climate space": two smooth fields,
//! an elevation tendency `e` and a wetness `w`, are ranked over the map, and every biome
//! claims the part of that space nearest its own (e, w) target. Each biome's claim is tuned
//! until it covers its share of the map, so swamps end up low and wet, pine woods dry, and
//! mountains next to highlands and taiga, while the mix the user chose is honoured.
//!
//! The three mountain biomes (alpine meadow, mountains, snowy peaks) share one region type,
//! split afterwards by altitude bands (tree line, rock band, snow line) and slope.

use crate::biome::Biome;
use crate::settings::{ShapeEdit, WorldSettings};

use super::grid::{lerp, par_map, Field};
use super::noise::{fbm, warp};

/// Mountain biomes from the lowest band to the highest.
pub const MOUNTAIN_FAMILY: [Biome; 3] = [Biome::AlpineMeadow, Biome::Mountains, Biome::SnowyPeaks];

pub fn is_mountain(b: Biome) -> bool {
    MOUNTAIN_FAMILY.contains(&b)
}

/// Terrain character of a region type. Heights are in blocks above the sea surface.
#[derive(Clone, Copy, Debug, Default)]
pub struct Params {
    /// Mean ground height.
    pub base: f32,
    /// Broad rolling hills (a few hundred blocks across).
    pub hills: f32,
    /// Smaller hills and knolls.
    pub bumps: f32,
    /// Ridged mountain relief.
    pub ridge: f32,
    /// Strength of small-scale ground detail.
    pub rough: f32,
    /// How readily erosion cuts valleys.
    pub erode: f32,
    /// Rock ledges on steep ground.
    pub terrace: f32,
    /// Wetland: flat ground around a water table with pools.
    pub swamp: f32,
    /// Share of a wetland that is open water.
    pub pools: f32,
    /// Width of the low coastal strip where land meets the sea.
    pub coast: f32,
    /// Elevation tendency the region is centred on (ground rises with `e` around it).
    pub e: f32,
}

pub const NPARAMS: usize = 11;

impl Params {
    pub fn to_array(self) -> [f32; NPARAMS] {
        [
            self.base,
            self.hills,
            self.bumps,
            self.ridge,
            self.rough,
            self.erode,
            self.terrace,
            self.swamp,
            self.pools,
            self.coast,
            self.e,
        ]
    }

    pub fn from_array(a: [f32; NPARAMS]) -> Self {
        Self {
            base: a[0],
            hills: a[1],
            bumps: a[2],
            ridge: a[3],
            rough: a[4],
            erode: a[5],
            terrace: a[6],
            swamp: a[7],
            pools: a[8],
            coast: a[9],
            e: a[10],
        }
    }
}

/// A region type: one biome, or the mountain family (`biome == None`).
#[derive(Clone, Debug)]
pub struct Category {
    pub biome: Option<Biome>,
    pub share: f64,
    /// Target in climate space: (elevation tendency, wetness), both 0..1.
    pub target: (f64, f64),
    pub params: Params,
}

fn category_for(b: Biome) -> (Option<Biome>, (f64, f64), Params) {
    let p = |base, hills, bumps, ridge, rough, erode, terrace, swamp, coast, e| Params {
        base,
        hills,
        bumps,
        ridge,
        rough,
        erode,
        terrace,
        swamp,
        pools: 0.0,
        coast,
        e,
    };
    match b {
        Biome::Plains => (
            Some(b),
            (0.30, 0.30),
            p(5.0, 6.0, 2.0, 0.0, 0.55, 0.5, 0.0, 0.0, 90.0, 0.30),
        ),
        Biome::Meadow => (
            Some(b),
            (0.38, 0.60),
            p(8.0, 9.0, 4.0, 0.0, 0.75, 0.6, 0.0, 0.0, 70.0, 0.38),
        ),
        Biome::Forest => (
            Some(b),
            (0.45, 0.75),
            p(9.0, 12.0, 5.0, 0.0, 0.9, 0.7, 0.0, 0.0, 60.0, 0.45),
        ),
        Biome::BirchForest => (
            Some(b),
            (0.50, 0.45),
            p(10.0, 12.0, 6.0, 0.0, 0.9, 0.7, 0.0, 0.0, 60.0, 0.50),
        ),
        Biome::Taiga => (
            Some(b),
            (0.66, 0.80),
            p(16.0, 18.0, 7.0, 12.0, 1.1, 0.9, 0.05, 0.0, 40.0, 0.66),
        ),
        Biome::PineForest => (
            Some(b),
            (0.62, 0.15),
            p(12.0, 14.0, 6.0, 8.0, 1.0, 0.8, 0.05, 0.0, 50.0, 0.62),
        ),
        Biome::Swamp => (
            Some(b),
            (0.12, 0.85),
            Params {
                pools: 0.42,
                ..p(1.6, 1.5, 0.6, 0.0, 0.3, 0.2, 0.0, 1.0, 120.0, 0.12)
            },
        ),
        Biome::Marsh => (
            Some(b),
            (0.06, 0.55),
            Params {
                pools: 0.55,
                ..p(1.0, 1.0, 0.4, 0.0, 0.25, 0.2, 0.0, 1.0, 140.0, 0.06)
            },
        ),
        Biome::Highlands => (
            Some(b),
            (0.78, 0.45),
            p(30.0, 26.0, 10.0, 28.0, 1.3, 1.0, 0.25, 0.0, 24.0, 0.78),
        ),
        // Beach, river and lake are made by the terrain; in a mix they count as plains.
        Biome::Beach | Biome::River | Biome::Lake => category_for(Biome::Plains),
        Biome::AlpineMeadow | Biome::Mountains | Biome::SnowyPeaks => (
            None,
            (0.95, 0.50),
            p(50.0, 30.0, 10.0, 150.0, 1.6, 1.2, 0.35, 0.0, 14.0, 0.95),
        ),
    }
}

/// The mix as region types, plus how the mountain family splits into bands.
#[derive(Clone, Debug)]
pub struct Mix {
    pub categories: Vec<Category>,
    /// Mountain biomes in the mix, lowest band first, with their share of the family.
    pub family: Vec<(Biome, f64)>,
}

impl Mix {
    pub fn new(settings: &WorldSettings) -> Self {
        let mut categories: Vec<Category> = Vec::new();
        let mut family: Vec<(Biome, f64)> = Vec::new();
        for s in settings.biome_mix() {
            let (biome, target, params) = category_for(s.biome);
            if is_mountain(s.biome) {
                family.push((s.biome, s.share as f64));
            }
            match categories.iter_mut().find(|c| c.biome == biome) {
                Some(c) => c.share += s.share as f64,
                None => categories.push(Category {
                    biome,
                    share: s.share as f64,
                    target,
                    params,
                }),
            }
        }
        if categories.is_empty() {
            let (biome, target, params) = category_for(Biome::Plains);
            categories.push(Category {
                biome,
                share: 1.0,
                target,
                params,
            });
        }
        // Zones may ask for region types the mix lacks; they get no share of their own.
        for edit in &settings.edits {
            if let ShapeEdit::BiomeZone { biome, .. } = edit {
                let (cat, target, params) = category_for(*biome);
                if !categories.iter().any(|c| c.biome == cat) {
                    categories.push(Category {
                        biome: cat,
                        share: 0.0,
                        target,
                        params,
                    });
                }
            }
        }
        family.sort_by_key(|(b, _)| MOUNTAIN_FAMILY.iter().position(|m| m == b));
        let total: f64 = family.iter().map(|f| f.1).sum();
        for f in &mut family {
            f.1 /= total;
        }
        Self { categories, family }
    }

    pub fn category_of(&self, b: Biome) -> Option<usize> {
        let (cat, _, _) = category_for(b);
        self.categories.iter().position(|c| c.biome == cat)
    }
}

/// Ranks values into 0..1 (uniform), keeping ties in index order.
fn rank_normalise(v: &mut [f32]) {
    let mut order: Vec<u32> = (0..v.len() as u32).collect();
    order.sort_by(|&a, &b| v[a as usize].total_cmp(&v[b as usize]));
    let last = (v.len().max(2) - 1) as f32;
    let mut out = vec![0.0; v.len()];
    for (rank, &k) in order.iter().enumerate() {
        out[k as usize] = rank as f32 / last;
    }
    v.copy_from_slice(&out);
}

/// Climate fields and the region type of every macro node.
pub struct Regions {
    /// Elevation tendency and wetness, each ranked to 0..1.
    pub e: Field,
    pub w: Field,
    /// Region type per node.
    pub cat: Vec<u8>,
}

/// Feature size of biome regions in blocks for a `biome_scale` slider value.
pub fn region_scale(biome_scale: f32) -> f64 {
    150.0 * (1000.0f64 / 150.0).powf(biome_scale.clamp(0.0, 1.0) as f64)
}

pub fn regions(settings: &WorldSettings, mix: &Mix, n: usize, cell: f64) -> Regions {
    let seed = settings.seed;
    let scale = region_scale(settings.sliders.biome_scale);
    let climate = |salt: u64| {
        let mut f = Field::from_fn(n, cell, |i, j| {
            let (x, z) = (i as f64 * cell, j as f64 * cell);
            let (wx, wz) = warp(seed ^ salt, x, z, scale * 0.6, scale * 0.35);
            let big = fbm(seed ^ salt ^ 0x11, wx / scale, wz / scale, 4, 0.5);
            // Smaller patches so borders are not just contour lines of one smooth field.
            let small = fbm(
                seed ^ salt ^ 0x22,
                x / (scale * 0.3),
                z / (scale * 0.3),
                2,
                0.5,
            );
            (big + 0.18 * small) as f32
        });
        rank_normalise(&mut f.data);
        f
    };
    let e = climate(0xE1E7);
    let w = climate(0x3E77);

    let cats = &mix.categories;
    let score = |k: usize, weights: &[f64]| {
        let (ev, wv) = (e.data[k] as f64, w.data[k] as f64);
        let mut best = (f64::MAX, 0usize);
        for (c, cat) in cats.iter().enumerate() {
            if cat.share <= 0.0 {
                continue;
            }
            let (de, dw) = (ev - cat.target.0, (wv - cat.target.1) * 0.8);
            let s = de * de + dw * dw - weights[c];
            if s < best.0 {
                best = (s, c);
            }
        }
        best.1
    };

    // Tune each region type's weight on a subsample until it covers its share.
    let mut weights = vec![0.0f64; cats.len()];
    let step = (n * n / 24_000).max(1);
    let sample: Vec<usize> = (0..n * n).step_by(step).collect();
    for it in 0..120 {
        let mut count = vec![0usize; cats.len()];
        for &k in &sample {
            count[score(k, &weights)] += 1;
        }
        let rate = lerp(0.6, 0.1, it as f64 / 120.0);
        for (c, cat) in cats.iter().enumerate() {
            if cat.share > 0.0 {
                weights[c] += rate * (cat.share - count[c] as f64 / sample.len() as f64);
            }
        }
    }
    let nodes: Vec<usize> = (0..n * n).collect();
    let mut cat: Vec<u8> = par_map(&nodes, |&k| score(k, &weights) as u8);

    // Biome zones paint their region type over the layout, with a ragged edge.
    for edit in &settings.edits {
        if let ShapeEdit::BiomeZone {
            x,
            z,
            radius,
            biome,
        } = edit
        {
            let Some(c) = mix.category_of(*biome) else {
                continue;
            };
            let (x, z, r) = (*x as f64, *z as f64, (*radius as f64).max(4.0));
            let reach = (r * 1.25 / cell).ceil() as i64 + 1;
            let (ci, cj) = ((x / cell).round() as i64, (z / cell).round() as i64);
            for j in (cj - reach).max(0)..=(cj + reach).min(n as i64 - 1) {
                for i in (ci - reach).max(0)..=(ci + reach).min(n as i64 - 1) {
                    let (px, pz) = (i as f64 * cell, j as f64 * cell);
                    let d = ((px - x).powi(2) + (pz - z).powi(2)).sqrt();
                    let ragged =
                        r * (1.0 + 0.15 * fbm(seed ^ 0x2087, px / 40.0, pz / 40.0, 2, 0.5));
                    if d <= ragged {
                        cat[j as usize * n + i as usize] = c as u8;
                    }
                }
            }
        }
    }
    Regions { e, w, cat }
}

/// The two strongest region types of a node after blending, and the second's weight.
#[derive(Clone, Copy, Debug, Default)]
pub struct Top2 {
    pub cat: [u8; 2],
    pub weight: [f32; 2],
}

/// Blurs each region type's footprint by `radius` nodes and keeps the two strongest per node.
/// Also returns the blurred weight of the mountain type (0 when the mix has none).
pub fn blend(
    regions: &Regions,
    mix: &Mix,
    n: usize,
    cell: f64,
    radius: usize,
) -> (Vec<Top2>, Field) {
    let mut top: Vec<Top2> = vec![Top2::default(); n * n];
    let mut mountain = Field::new(n, cell, 0.0);
    for c in 0..mix.categories.len() {
        let mut f = Field::new(n, cell, 0.0);
        for (k, v) in f.data.iter_mut().enumerate() {
            if regions.cat[k] as usize == c {
                *v = 1.0;
            }
        }
        f.blur(radius, 2);
        for (k, &v) in f.data.iter().enumerate() {
            let t = &mut top[k];
            if v > t.weight[0] {
                t.cat[1] = t.cat[0];
                t.weight[1] = t.weight[0];
                t.cat[0] = c as u8;
                t.weight[0] = v;
            } else if v > t.weight[1] {
                t.cat[1] = c as u8;
                t.weight[1] = v;
            }
        }
        if mix.categories[c].biome.is_none() {
            mountain = f;
        }
    }
    for t in &mut top {
        if t.weight[1] <= 0.0 {
            t.cat[1] = t.cat[0];
        }
    }
    (top, mountain)
}

/// Terrain parameters per node, blended across region borders over `radius` nodes.
pub fn params(regions: &Regions, mix: &Mix, n: usize, cell: f64, radius: usize) -> Vec<Field> {
    let table: Vec<[f32; NPARAMS]> = mix.categories.iter().map(|c| c.params.to_array()).collect();
    (0..NPARAMS)
        .map(|p| {
            let mut f = Field::new(n, cell, 0.0);
            for (k, v) in f.data.iter_mut().enumerate() {
                *v = table[regions.cat[k] as usize][p];
            }
            f.blur(radius, 3);
            f
        })
        .collect()
}

/// Altitude bands of the mountain family: the Y where each band (after the first) starts.
#[derive(Clone, Debug, Default)]
pub struct Bands {
    pub members: Vec<Biome>,
    pub starts: Vec<f32>,
    /// How far band edges wander and blend, relative to a standard world (more in a tall
    /// world, whose bands are further apart).
    pub spread: f32,
}

impl Bands {
    /// Bands from the mix's shares, placed at height quantiles of the mountain regions, so
    /// a low range is mostly alpine meadow and a high one mostly rock and snow.
    pub fn from_heights(family: &[(Biome, f64)], mut heights: Vec<f32>, sea: f32) -> Self {
        if family.is_empty() {
            // Mountains raised by hand in a mix without mountain biomes.
            return Self {
                members: vec![Biome::Mountains, Biome::SnowyPeaks],
                starts: vec![sea + 135.0],
                spread: 1.0,
            };
        }
        heights.sort_by(|a, b| a.total_cmp(b));
        let mut starts = Vec::new();
        let mut acc = 0.0;
        for (_, share) in &family[..family.len() - 1] {
            acc += share;
            let q = if heights.is_empty() {
                sea + 60.0 + 100.0 * acc as f32
            } else {
                heights[((heights.len() - 1) as f64 * acc).round() as usize]
            };
            starts.push(q);
        }
        Self {
            members: family.iter().map(|f| f.0).collect(),
            starts,
            spread: 1.0,
        }
    }

    /// The band for a column, the neighbouring band it blends into, and the blend weight.
    /// `jitter` (about ±1) moves band edges so they are not contour lines.
    pub fn resolve(&self, h: f32, slope: f32, jitter: f32) -> (Biome, Biome, f32) {
        let h = h + jitter * 7.0 * self.spread;
        let edge = 8.0 * self.spread;
        let mut band = 0;
        while band < self.starts.len() && h >= self.starts[band] {
            band += 1;
        }
        let pick = |i: usize| self.members[i];
        let mut biome = pick(band);
        // Steep faces show rock; gentle shoulders in the rock band carry alpine grass.
        let has = |b: Biome| self.members.contains(&b);
        if biome == Biome::AlpineMeadow && slope > 1.15 && has(Biome::Mountains) {
            biome = Biome::Mountains;
        } else if biome == Biome::Mountains
            && slope < 0.3
            && has(Biome::AlpineMeadow)
            && band + 1 < self.members.len()
        {
            biome = Biome::AlpineMeadow;
        } else if biome == Biome::SnowyPeaks && slope > 1.7 && has(Biome::Mountains) {
            biome = Biome::Mountains;
        }
        // Blend towards the nearest band edge within 8 blocks (times the spread).
        let mut other = biome;
        let mut blend = 0.0;
        if band > 0 {
            let d = h - self.starts[band - 1];
            if d < edge {
                other = pick(band - 1);
                blend = 0.5 * (1.0 - d / edge);
            }
        }
        if band < self.starts.len() {
            let d = self.starts[band] - h;
            if d < edge && 0.5 * (1.0 - d / edge) > blend {
                other = pick(band + 1);
                blend = 0.5 * (1.0 - d / edge);
            }
        }
        if other == biome {
            blend = 0.0;
        }
        (biome, other, blend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::BiomeShare;

    fn settings(mix: &[(Biome, f32)]) -> WorldSettings {
        WorldSettings {
            size: 1024,
            seed: 5,
            biomes: mix
                .iter()
                .map(|&(biome, share)| BiomeShare { biome, share })
                .collect(),
            ..WorldSettings::default()
        }
    }

    #[test]
    fn shares_are_honoured() {
        let s = settings(&[
            (Biome::Plains, 0.5),
            (Biome::Forest, 0.3),
            (Biome::Swamp, 0.2),
        ]);
        let mix = Mix::new(&s);
        let n = 129;
        let r = regions(&s, &mix, n, 8.0);
        for (c, cat) in mix.categories.iter().enumerate() {
            let got = r.cat.iter().filter(|&&k| k as usize == c).count() as f64 / (n * n) as f64;
            assert!((got - cat.share).abs() < 0.05, "{:?} {got}", cat.biome);
        }
        // Swamps sit at the low end of the elevation tendency.
        let mean_e = |c: usize| {
            let v: Vec<f32> = (0..n * n)
                .filter(|&k| r.cat[k] as usize == c)
                .map(|k| r.e.data[k])
                .collect();
            v.iter().sum::<f32>() / v.len() as f32
        };
        let swamp = mix.category_of(Biome::Swamp).unwrap();
        let forest = mix.category_of(Biome::Forest).unwrap();
        assert!(mean_e(swamp) < mean_e(forest));
    }

    #[test]
    fn mountain_family_is_one_region_type() {
        let s = settings(&[
            (Biome::Mountains, 0.3),
            (Biome::SnowyPeaks, 0.1),
            (Biome::Taiga, 0.6),
        ]);
        let mix = Mix::new(&s);
        assert_eq!(mix.categories.len(), 2);
        assert_eq!(mix.family.len(), 2);
        assert!((mix.family[0].1 - 0.75).abs() < 1e-6);
        let bands = Bands::from_heights(&mix.family, (0..100).map(|i| i as f32).collect(), 64.0);
        assert_eq!(bands.members, vec![Biome::Mountains, Biome::SnowyPeaks]);
        assert!((bands.starts[0] - 74.0).abs() < 1.5);
        assert_eq!(bands.resolve(10.0, 0.5, 0.0).0, Biome::Mountains);
        assert_eq!(bands.resolve(95.0, 0.5, 0.0).0, Biome::SnowyPeaks);
        let (_, other, blend) = bands.resolve(72.0, 0.5, 0.0);
        assert_eq!(other, Biome::SnowyPeaks);
        assert!(blend > 0.0 && blend <= 0.5);
    }
}
