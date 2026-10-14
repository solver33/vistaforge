//! Terrain: the shape of the land, water, and where each biome is.
//!
//! [`Terrain::new`] does the whole-map work up front, on a macro grid of 8-block nodes:
//!
//! 1. **Layout** ([`layout`]): biome regions drawn from the user's mix in climate space,
//!    blended across borders.
//! 2. **Shape**: each region's character (plains, rolling hills, ridged mountains, flat
//!    wetland), a coast and sea when asked for, and hand-placed hills, mountains and ridges.
//! 3. **Erosion** ([`erosion`]): stream-power erosion cuts branching valleys; scree settles.
//! 4. **Water** ([`hydro`], [`rivers`]): plateaus, lakes and drawn rivers go in, hollows
//!    become lakes (small ones silt up), and rivers are traced down the drainage network.
//! 5. At full quality, droplet erosion on a 2-block grid carves rills and gullies.
//!
//! [`Terrain::column`] then works per column: smooth interpolation of the macro ground,
//! detail noise, rock ledges, wetland pools, lake shores and river channels, and finally
//! biome, moisture, rock and flow. It is cheap, thread-safe and pure, and [`Terrain::grid`]
//! gives exactly the same answers for the same columns.

pub mod presets;

mod brushes;
mod edits;
mod erosion;
mod grid;
mod hydro;
mod layout;
mod noise;
mod rivers;

use anyhow::Result;

use crate::biome::Biome;
use crate::column::{ColumnGrid, ColumnInfo, Rect, WaterKind};
use crate::noise::unit2;
use crate::settings::{Sliders, WorldHeight, WorldSettings};
use crate::world::MIN_Y;

use edits::Zone;
use grid::{lerp, smoothstep, Field};
use hydro::NONE;
use layout::{is_mountain, Bands, Mix, Params, Top2, NPARAMS};
use noise::{fbm, gully, perlin, ridged, warp};
use rivers::Rivers;

/// How much whole-map work to do: previews need an answer in about a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Preview,
    Full,
}

/// Macro grid spacing in blocks.
const CELL: f64 = 8.0;
/// Droplet erosion grid spacing in blocks.
const FINE: f64 = 2.0;
/// Highest ground allowed in a standard world (it ends at 319).
const TOP: f32 = 300.0;
/// How a tall world uses its height. The broad shape of the high ground rises `TALL_STRETCH`
/// times, its valleys and ridges `1 + TALL_DETAIL × (TALL_STRETCH − 1)` times, and mountain
/// ranges are `TALL_WIDE` times as wide, so the extra height makes massifs, not spires.
/// Lowlands keep their height.
const TALL_STRETCH: f64 = 10.0;
const TALL_DETAIL: f64 = 0.2;
const TALL_WIDE: f64 = 3.0;

struct Fine {
    /// Height change from droplet erosion.
    delta: Field,
    /// 0..1, how much droplet water ran through.
    flow: Field,
}

pub struct Terrain {
    seed: u64,
    size: i32,
    /// Highest ground allowed: [`TOP`], or 20 blocks under the top of a tall world.
    top: f32,
    /// 1, or how much a tall world raises its high ground ([`TALL_STRETCH`]).
    stretch: f64,
    /// Water fills every Y below this at sea.
    sea_surface: i32,
    sliders: Sliders,
    mix: Mix,
    bands: Bands,
    /// Bands for mountains raised by hand.
    edit_bands: Bands,
    n: usize,
    /// Macro ground height after erosion and edits.
    height: Field,
    rough: Field,
    terrace: Field,
    steep: Field,
    /// Height added by gullies on steep ground, on a 4-block grid.
    gullies: Field,
    swamp: Field,
    pools: Field,
    /// Smoothed ground: wetland water tables follow it.
    table: Field,
    /// Water table of wetland cores (they are flattened to it).
    wet_table: f64,
    /// Distance in blocks to a lake at the level of wetland pools.
    wet_lake_dist: Field,
    /// Climate wetness 0..1.
    wet: Field,
    /// log-scaled drained area 0..1.
    drain: Field,
    sea_dist: Field,
    lake_dist: Field,
    /// Height added by hand-placed mountains and ridges.
    lift: Field,
    top2: Vec<Top2>,
    lake: Vec<u32>,
    lake_surface: Vec<i32>,
    lake_nodes: Vec<u32>,
    sea: Vec<bool>,
    fine: Option<Fine>,
    rivers: Rivers,
    zones: Vec<Zone>,
    /// Roads and waterfalls drawn by hand, laid over the finished land.
    brushes: brushes::Brushes,
    /// Multiplier for ground detail (roughness slider).
    detail_amp: f64,
    /// How far biome borders wander, in blocks.
    blend_warp: f64,
}

/// Terrain at one column before biomes: ground, water, and what is nearby.
#[derive(Clone, Copy, Debug)]
struct Sample {
    h: f32,
    water: WaterKind,
    level: Option<i32>,
    /// Distance from the nearest river's water edge (large when none is near).
    river_edge: f32,
    /// 0..1 how strongly this is a beach.
    beach: f32,
    /// Wetland weight 0..1.
    swampy: f32,
    /// Top of a waterfall standing on this column's water.
    fall: Option<i32>,
    /// Blocks inside a road's edge (see [`ColumnInfo::road`]).
    road: Option<f32>,
}

/// How high dry ground must be beside standing water with `surface`, where the water's
/// nodes weigh `weight` around the column: the surface itself where water can touch it,
/// sloping away beyond that.
fn shore_floor(surface: i32, weight: f32) -> f32 {
    surface as f32 - (0.175 - weight).max(0.0) * 16.0
}

/// Gullies and spurs running down steep ground, a few octaves deep, each bending the next one
/// towards it (an erosion filter). They are as deep as the slope is steep, so high mountains
/// get deep ravines and sharp ridges between them, and gentle land none. `slope` is the macro
/// ground's slope along x and z; `octaves` from 96 blocks across, each half the last.
fn gullies(seed: u64, x: f64, z: f64, slope: (f64, f64), octaves: u64) -> f64 {
    let (mut gx, mut gz) = slope;
    let steep = gx.hypot(gz);
    let amount = smoothstep(0.12, 0.55, steep);
    if amount <= 0.0 {
        return 0.0;
    }
    let mut h = 0.0;
    let mut wave = 96.0;
    for o in 0..octaves {
        let amp = 0.075 * steep * wave;
        let (v, dx, dz) = gully(seed ^ (0x6011 + o), x / wave, z / wave, gx, gz);
        h += amp * v;
        gx += amp * dx / wave;
        gz += amp * dz / wave;
        wave *= 0.5;
    }
    h * amount
}

/// Height above the sea `d` stretched `stretch` times for a tall world: unchanged up to 15
/// blocks, then rising `stretch` times as fast from 110 blocks up, so lowlands and their
/// rivers stay as they are and mountains tower over them.
fn uplift(d: f64, stretch: f64) -> f64 {
    const A: f64 = 15.0;
    const B: f64 = 110.0;
    if stretch <= 1.0 || d <= A {
        return d;
    }
    // The extra height is `stretch - 1` times the integral of smoothstep(A, B, d).
    let extra = if d >= B {
        d - B + 0.5 * (B - A)
    } else {
        let u = (d - A) / (B - A);
        (B - A) * (u * u * u - 0.5 * u * u * u * u)
    };
    d + (stretch - 1.0) * extra
}

/// How many times steeper than in a standard world valleys and ridges are at height `d`
/// above the sea: 1 in the lowlands, up to `1 + TALL_DETAIL × (stretch − 1)` up high.
fn steepening(d: f64, stretch: f64) -> f64 {
    1.0 + TALL_DETAIL * (stretch - 1.0).max(0.0) * smoothstep(15.0, 110.0, d)
}

/// Rounds off peaks that would reach the top of the world, so they never end flat: heights
/// above the knee bend towards a ceiling just under `top`.
fn soft_cap(h: f64, top: f64, stretch: f64) -> f64 {
    let room = 50.0 * stretch;
    let knee = top - room - if stretch > 1.0 { 30.0 } else { 0.0 };
    if h > knee {
        knee + room * ((h - knee) / room).tanh()
    } else {
        h
    }
}

fn ground_y(h: f32) -> i32 {
    ColumnInfo::dry(h, Biome::Plains).ground().0
}

/// Sea mask: `share` of the nodes, along one side (`islands` 0), around one island (0.5),
/// or among many islands (1).
fn coast_mask(seed: u64, n: usize, size: f64, share: f64, islands: f64) -> Vec<bool> {
    let s = islands.clamp(0.0, 1.0);
    let angle = unit2(seed, 77, 3) * std::f64::consts::TAU;
    let dir = (angle.cos(), angle.sin());
    let scale = size * lerp(0.5, 0.11, s);
    let f = Field::from_fn(n, CELL, |i, j| {
        let (x, z) = (i as f64 * CELL, j as f64 * CELL);
        let (u, v) = (x / size - 0.5, z / size - 0.5);
        let side = -(u * dir.0 + v * dir.1);
        let radial = 0.42 - (u.powi(4) + v.powi(4)).powf(0.25) * 1.1;
        let edge = if s < 0.5 {
            lerp(side, radial, s * 2.0)
        } else {
            radial
        };
        let (wx, wz) = warp(seed ^ 0xC0A5, x, z, scale, scale * 0.35);
        let noise = fbm(seed ^ 0xC0A6, wx / scale, wz / scale, 5, 0.55);
        (edge + lerp(0.2, 0.7, s) * noise) as f32
    });
    let mut sorted = f.data.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let cut = sorted[((sorted.len() - 1) as f64 * share) as usize];
    f.data.iter().map(|&v| v < cut).collect()
}

impl Terrain {
    pub fn new(settings: &WorldSettings, quality: Quality) -> Result<Self> {
        let size = settings.size as i32;
        let seed = settings.seed;
        let sl = settings.sliders;
        let clamp = |v: f32| v.clamp(0.0, 1.0) as f64;
        let n = (size as f64 / CELL) as usize + 1;
        let sea_surface = settings.sea_level + 1;
        let sea_f = sea_surface as f32;
        let relief = 0.35 + 1.3 * clamp(sl.relief);
        let peaks = 0.4 + 1.2 * clamp(sl.mountains);
        let water = clamp(sl.water);
        let top = match settings.world_height {
            WorldHeight::Standard => TOP,
            tall => (tall.max_y() - 20) as f32,
        };
        let (stretch, wide) = match settings.world_height {
            WorldHeight::Standard => (1.0, 1.0),
            WorldHeight::Tall => (TALL_STRETCH, TALL_WIDE),
        };
        let steeper = steepening(f64::MAX, stretch);

        // 1. Layout.
        let mix = Mix::new(settings);
        let regions = layout::regions(settings, &mix, n, CELL);
        let blend_r = (1.0 + 5.0 * clamp(sl.biome_blend)).round() as usize;
        let (top2, mountain) = layout::blend(&regions, &mix, n, CELL, blend_r);
        let pf = layout::params(&regions, &mix, n, CELL, blend_r + 2);
        let param = |k: usize| {
            let mut a = [0.0; NPARAMS];
            for (p, v) in a.iter_mut().enumerate() {
                *v = pf[p].data[k];
            }
            Params::from_array(a)
        };

        // 2. Shape.
        let sea_share = 0.85 * clamp(sl.sea);
        let sea_mask = (sea_share > 0.01)
            .then(|| coast_mask(seed, n, size as f64, sea_share, clamp(sl.islands)));
        let (to_sea, to_land) = match &sea_mask {
            Some(m) => {
                let land: Vec<bool> = m.iter().map(|s| !s).collect();
                (grid::distance(m, n, CELL), grid::distance(&land, n, CELL))
            }
            None => (Vec::new(), Vec::new()),
        };
        let mscale = 520.0 * wide * (0.75 + 0.5 * clamp(sl.biome_scale));
        let e = &regions.e;
        let mut height = Field::from_fn(n, CELL, |i, j| {
            let k = j * n + i;
            let q = param(k);
            let (x, z) = (i as f64 * CELL, j as f64 * CELL);
            let (wx, wz) = warp(seed ^ 0xB1, x, z, 420.0, 110.0);
            let hills = fbm(seed ^ 0xB2, wx / 260.0, wz / 260.0, 4, 0.5);
            let bumps = fbm(seed ^ 0xB3, x / 75.0, z / 75.0, 3, 0.5);
            let mut h = q.base as f64
                + 16.0 * (e.data[k] - q.e) as f64
                + q.hills as f64 * hills
                + q.bumps as f64 * bumps;
            if q.ridge > 0.5 {
                let (mx, mz) = warp(seed ^ 0xB4, x, z, 300.0, 95.0);
                let r = ridged(seed ^ 0xB5, mx / mscale, mz / mscale, 6);
                let massif =
                    0.62 + 0.38 * fbm(seed ^ 0xB6, x / (mscale * 1.8), z / (mscale * 1.8), 2, 0.5);
                h += q.ridge as f64 * peaks * r * massif;
            }
            let mut h = h * relief;
            if q.swamp > 0.01 {
                // Wetlands are flat, just above the whole block their pools and lakes fill to.
                h = lerp(h, (1.3 * relief).round() + 1.05, q.swamp as f64);
            }
            let mut h = sea_f as f64 + h;
            if let Some(mask) = &sea_mask {
                if mask[k] {
                    let d = to_land[k] as f64;
                    h = sea_f as f64 - 1.5 - (d * 0.14).min(30.0)
                        + 1.5 * fbm(seed ^ 0xB7, x / 60.0, z / 60.0, 3, 0.5);
                } else {
                    let d = to_sea[k] as f64;
                    let t = smoothstep(0.0, q.coast as f64, d);
                    let low = sea_f as f64 + 0.6;
                    h = low + (h - low) * t;
                }
            }
            if stretch > 1.0 {
                h as f32
            } else {
                soft_cap(h, top as f64, stretch) as f32
            }
        });
        if stretch > 1.0 {
            // The broad shape (blurred over about 200 blocks) rises fully, what stands above
            // or below it less.
            let mut base = height.clone();
            base.blur(24, 2);
            for (v, b) in height.data.iter_mut().zip(&base.data) {
                let d = (*b - sea_f) as f64;
                let h =
                    sea_f as f64 + uplift(d, stretch) + steepening(d, stretch) * (*v - *b) as f64;
                *v = soft_cap(h, top as f64, stretch) as f32;
            }
        }
        // Hollows for lakes, more with the water slider.
        let dents = ((size as f64 / 1024.0).powi(2) * 16.0 * water * water).round() as usize;
        for d in 0..dents {
            let u = |salt: i64| unit2(seed ^ 0xDE47, d as i64, salt);
            let (x, z) = (u(0) * size as f64, u(1) * size as f64);
            let r = lerp(18.0, 64.0, u(2));
            let depth = lerp(3.0, 9.0, u(3));
            let (ci, cj) = ((x / CELL).round() as usize, (z / CELL).round() as usize);
            if sea_mask
                .as_ref()
                .is_some_and(|m| m[cj.min(n - 1) * n + ci.min(n - 1)])
            {
                continue;
            }
            let reach = (r / CELL).ceil() as i64 + 1;
            for j in (cj as i64 - reach).max(0)..=(cj as i64 + reach).min(n as i64 - 1) {
                for i in (ci as i64 - reach).max(0)..=(ci as i64 + reach).min(n as i64 - 1) {
                    let (px, pz) = (i as f64 * CELL, j as f64 * CELL);
                    let rr = r
                        * (1.0
                            + 0.25 * fbm(seed ^ 0xDE48 ^ d as u64, px / 30.0, pz / 30.0, 2, 0.5));
                    let t = (px - x).hypot(pz - z) / rr;
                    if t < 1.0 {
                        let k = j as usize * n + i as usize;
                        height.data[k] -= (depth * (1.0 - t * t).powf(1.5)) as f32;
                    }
                }
            }
        }
        let mut lift = Field::new(n, CELL, 0.0);
        edits::landforms(&mut height, &mut lift, &settings.edits, seed);

        // 3. Erosion.
        let rain: Vec<f32> = regions.w.data.iter().map(|w| 0.7 + 0.6 * w).collect();
        let seeds = |h: &[f32]| hydro::seeds(h, n, sea_mask.as_deref(), sea_f);
        let iterations = (4.0 + 22.0 * clamp(sl.erosion)).round() as usize;
        erosion::stream_power(
            &mut height.data,
            n,
            &pf[5].data,
            &rain,
            iterations,
            0.0035,
            seeds,
        );
        erosion::thermal(&mut height.data, n, CELL as f32, 1.3 * steeper as f32, 4);

        // 4. Water.
        edits::late(&mut height, &settings.edits, seed);
        let mut drawn = vec![NONE; n * n];
        let drawn_traces = edits::rivers(&mut height, &settings.edits, &mut drawn);
        let find_water = |h: &mut Field| {
            let fl = hydro::flood(&h.data, n, &seeds(&h.data));
            let w = hydro::water(
                &mut h.data,
                &fl.filled,
                n,
                sea_mask.is_some().then_some(sea_f),
                lerp(14.0, 3.0, water) as usize,
                lerp(2.5, 1.0, water) as f32,
            );
            (fl, w)
        };
        let (mut fl, mut w) = find_water(&mut height);
        // Very deep or very large lakes drain through a gorge until they are in proportion.
        let placed = edits::placed_lakes(&settings.edits, n, CELL);
        for _ in 0..4 {
            let max_depth = lerp(8.0, 30.0, water) as f32;
            let max_nodes = ((n * n) as f64 * lerp(0.008, 0.05, water)).max(150.0) as usize;
            let changed = hydro::breach(
                &mut height.data,
                &fl,
                &w,
                max_depth,
                max_nodes,
                CELL as f32,
                &placed,
            );
            if !changed {
                break;
            }
            (fl, w) = find_water(&mut height);
        }
        let fl = hydro::flood(&height.data, n, &seeds(&height.data));
        let area = hydro::drainage(&fl, &rain);

        // Derived fields.
        let total = (n * n) as f32;
        let drain = Field {
            n,
            cell: CELL,
            data: area.iter().map(|a| a.max(1.0).ln() / total.ln()).collect(),
        };
        let is_lake: Vec<bool> = w.lake.iter().map(|&l| l != NONE).collect();
        let field = |data: Vec<f32>| Field {
            n,
            cell: CELL,
            data,
        };
        let sea_dist = field(grid::distance(&w.sea, n, CELL));
        let lake_dist = field(grid::distance(&is_lake, n, CELL));
        // The level wetlands were flattened to; their pools fill to the block below it.
        let wet_table = sea_f as f64 + (1.3 * relief).round() + 1.05;
        let steep = Field::from_fn(n, CELL, |i, j| {
            let at = |a: usize, b: usize| height.at(a.min(n - 1), b.min(n - 1));
            let dx = at(i + 1, j) - at(i.saturating_sub(1), j);
            let dz = at(i, j + 1) - at(i, j.saturating_sub(1));
            (dx.hypot(dz) / (2.0 * CELL as f32)).min(4.0)
        });
        let slope_along = |dx: usize, dz: usize| {
            Field::from_fn(n, CELL, |i, j| {
                let at = |a: usize, b: usize| height.at(a.min(n - 1), b.min(n - 1));
                let (i0, j0) = (i.saturating_sub(dx), j.saturating_sub(dz));
                let (i1, j1) = (i + dx, j + dz);
                let span = ((i1.min(n - 1) - i0) + (j1.min(n - 1) - j0)).max(1) as f32;
                (at(i1, j1) - at(i0, j0)) / (span * CELL as f32)
            })
        };
        let (slope_x, slope_z) = (slope_along(1, 0), slope_along(0, 1));
        // Previews skip the finest gullies.
        let (gcell, octaves) = match quality {
            Quality::Full => (4.0, 3),
            Quality::Preview => (CELL, 2),
        };
        let gully_field = Field::from_fn((size as f64 / gcell) as usize + 2, gcell, |i, j| {
            let (x, z) = (i as f64 * gcell, j as f64 * gcell);
            let slope = (slope_x.bilinear(x, z), slope_z.bilinear(x, z));
            gullies(seed, x, z, slope, octaves) as f32
        });
        let mut table = height.clone();
        table.blur(4, 2);
        let mut lake_nodes = vec![0u32; w.surface.len()];
        for &l in &w.lake {
            if l != NONE {
                lake_nodes[l as usize] += 1;
            }
        }
        let mountain_heights: Vec<f32> = (0..n * n)
            .filter(|&k| mountain.data[k] > 0.6 && w.lake[k] == NONE && !w.sea[k])
            .map(|k| height.data[k])
            .collect();
        let mut bands = Bands::from_heights(&mix.family, mountain_heights, sea_f);
        bands.spread = steeper as f32;
        // Hand-made peaks use the mix's mountain bands, or rock and snow when it has none.
        let edit_bands = bands.clone();
        // Shore nodes where detail may take the ground under the water join the water's reach.
        let detail_amp = 0.3 + 1.4 * clamp(sl.roughness);
        let fine_margin = if quality == Quality::Full { 3.0 } else { 1.0 };
        let margin = |k: usize| (3.5 * pf[4].data[k] as f64 * detail_amp + fine_margin) as f32;
        let lake_reach = hydro::widen(
            &w.lake,
            |l| w.surface[l as usize] as f32,
            &height.data,
            &fl.filled,
            n,
            1,
            margin,
        );
        let sea_ids: Vec<u32> = w.sea.iter().map(|&s| if s { 0 } else { NONE }).collect();
        let sea_reach: Vec<bool> =
            hydro::widen(&sea_ids, |_| sea_f, &height.data, &fl.filled, n, 2, margin)
                .iter()
                .map(|&s| s != NONE)
                .collect();
        let mut pf = pf;
        let take =
            |pf: &mut Vec<Field>, p: usize| std::mem::replace(&mut pf[p], Field::new(1, CELL, 0.0));
        let mut terrain = Self {
            seed,
            size,
            top,
            stretch,
            sea_surface,
            sliders: sl,
            bands,
            edit_bands,
            n,
            height,
            rough: take(&mut pf, 4),
            terrace: take(&mut pf, 6),
            steep,
            gullies: gully_field,
            swamp: take(&mut pf, 7),
            pools: take(&mut pf, 8),
            table,
            wet_table,
            wet_lake_dist: Field::new(1, CELL, f32::MAX),
            wet: regions.w,
            drain,
            sea_dist,
            lake_dist,
            lift,
            top2,
            lake: lake_reach,
            lake_surface: w.surface.clone(),
            lake_nodes,
            sea: sea_reach,
            fine: None,
            rivers: Rivers::build(Vec::new(), 0, size, &|_, _| 0.0, &|_, _| 0.0),
            zones: edits::zones(&settings.edits, seed),
            brushes: brushes::Brushes::default(),
            detail_amp,
            blend_warp: 6.0 + 10.0 * clamp(sl.biome_blend),
            mix,
        };
        if quality == Quality::Full {
            terrain.fine = Some(terrain.droplet_erosion());
        }

        // 5. Water fitted to the ground at block scale: lakes come down to the lowest point
        // of their rim, and rivers stay below their banks.
        terrain.fit_lakes(&placed);
        w.surface.clone_from(&terrain.lake_surface);
        // Lakes at the level of wetland pools: the pools run down into their hollows.
        let pool_level = wet_table.floor() as i32;
        let wet_lake: Vec<bool> = w
            .lake
            .iter()
            .map(|&l| l != NONE && w.surface[l as usize] >= pool_level)
            .collect();
        terrain.wet_lake_dist = field(grid::distance(&wet_lake, n, CELL));
        let mut traces = drawn_traces;
        if sl.rivers > 0.02 {
            let a_min = 100.0 * 25f32.powf(1.0 - sl.rivers.clamp(0.0, 1.0));
            traces.extend(rivers::trace(&rivers::Network {
                n,
                cell: CELL,
                flood: &fl,
                area: &area,
                water: &w,
                sea_surface: sea_f,
                a_min,
                width: 0.4 + 0.6 * sl.rivers.clamp(0.0, 1.0),
                drawn: &drawn,
                drawn_count: traces.len(),
            }));
        }
        let ground = |x: f64, z: f64| terrain.natural(x, z, terrain.swamp.bilinear(x, z));
        // In wetlands rivers are slow channels at the level of the pools, not cut into the
        // ground; the lift fades in at the wetland's edge.
        let pools = wet_table.floor();
        let lift = |x: f64, z: f64| {
            let swampy = terrain.swamp.bilinear(x, z);
            match smoothstep(0.3, 0.6, swampy) {
                0.0 => f64::MIN,
                t => pools - 1.0 + t + 0.01,
            }
        };
        let rivers = Rivers::build(traces, seed ^ 0x7177, size, &ground, &lift);
        terrain.rivers = rivers;
        terrain.brushes = brushes::Brushes::build(&terrain, &settings.edits, seed);
        Ok(terrain)
    }

    /// Lowers lakes whose rim dips below their water between the macro nodes, where the
    /// lake would otherwise need a wall to hold it: on a slope, the ground past a lake's
    /// outlet falls away faster than the macro grid shows. A low lip of up to two blocks
    /// may stay. Wetlands are left to their pools, and lakes the user placed (with a node in
    /// `placed`) keep their level.
    fn fit_lakes(&mut self, placed: &[bool]) {
        let n = self.n;
        let corners = |i: usize, j: usize| {
            [
                j * n + i,
                j * n + i + 1,
                (j + 1) * n + i,
                (j + 1) * n + i + 1,
            ]
        };
        let cells: Vec<(usize, usize)> = (0..n - 1)
            .flat_map(|j| (0..n - 1).map(move |i| (i, j)))
            .filter(|&(i, j)| corners(i, j).iter().any(|&k| self.lake[k] != NONE))
            .collect();
        let this = &*self;
        let lows: Vec<Vec<(u32, f64)>> = grid::par_map(&cells, |&(i, j)| {
            let ks = corners(i, j);
            let mut out: Vec<(u32, f64)> = Vec::new();
            let c = CELL as usize;
            let mut seen: Vec<u32> = Vec::with_capacity(4);
            for dz in 0..c {
                for dx in 0..c {
                    let (tx, tz) = (dx as f32 / CELL as f32, dz as f32 / CELL as f32);
                    let ws = [
                        (1.0 - tx) * (1.0 - tz),
                        tx * (1.0 - tz),
                        (1.0 - tx) * tz,
                        tx * tz,
                    ];
                    let (x, z) = ((i * c + dx) as f64, (j * c + dz) as f64);
                    let mut g = None;
                    seen.clear();
                    for a in 0..4 {
                        let l = this.lake[ks[a]];
                        if l == NONE || seen.contains(&l) {
                            continue;
                        }
                        seen.push(l);
                        let lambda: f32 = (0..4)
                            .filter(|&b| this.lake[ks[b]] == l)
                            .map(|b| ws[b])
                            .sum();
                        if lambda <= 0.0 || lambda >= 0.3 {
                            continue;
                        }
                        let swampy = this.swamp.bilinear(x, z);
                        if swampy > 0.3 {
                            continue;
                        }
                        let g = *g.get_or_insert_with(|| this.natural(x, z, swampy));
                        let allow = g + 2.0 + (0.175 - lambda as f64).max(0.0) * 16.0;
                        match out.iter_mut().find(|o| o.0 == l) {
                            Some(o) => o.1 = o.1.min(allow),
                            None => out.push((l, allow)),
                        }
                    }
                }
            }
            out
        });
        let mut keep = vec![false; self.lake_surface.len()];
        for (k, &l) in self.lake.iter().enumerate() {
            if l != NONE && placed[k] {
                keep[l as usize] = true;
            }
        }
        for (l, allow) in lows.into_iter().flatten() {
            if !keep[l as usize] {
                let s = &mut self.lake_surface[l as usize];
                *s = (*s).min(allow.floor() as i32);
            }
        }
    }

    /// Side of the map in blocks.
    pub fn size(&self) -> i32 {
        self.size
    }

    /// Gentle knolls and dips a few dozen blocks across.
    fn detail(&self, x: f64, z: f64) -> f64 {
        3.2 * fbm(self.seed ^ 0xD1, x / 42.0, z / 42.0, 4, 0.5)
    }

    /// Unevenness of the ground at block scale, which shows as Conquest layers.
    fn micro(&self, x: f64, z: f64) -> f64 {
        0.32 * perlin(self.seed ^ 0xD2, x / 5.3, z / 5.3)
            + 0.14 * perlin(self.seed ^ 0xD3, x / 2.1, z / 2.1)
    }

    /// Ground before droplet erosion and block-scale detail.
    fn smooth_ground(&self, x: f64, z: f64) -> f64 {
        // Lake shores follow the basin rather than the detail, which could dip below the water.
        let shore = 0.35 + 0.65 * smoothstep(4.0, 28.0, self.lake_dist.bilinear(x, z));
        self.height.cubic(x, z)
            + self.rough.bilinear(x, z) * self.detail_amp * shore * self.detail(x, z)
            + shore * self.gullies.bilinear(x, z)
    }

    fn droplet_erosion(&self) -> Fine {
        let nf = (self.size as f64 / FINE) as usize + 1;
        let mut f = Field::from_fn(nf, FINE, |i, j| {
            self.smooth_ground(i as f64 * FINE, j as f64 * FINE) as f32
        });
        let before = f.data.clone();
        let p = erosion::Droplets {
            density: 0.15 + 0.9 * self.sliders.erosion.clamp(0.0, 1.0) as f64,
            ..erosion::Droplets::default()
        };
        let visits = erosion::droplets(&mut f, &p, self.seed ^ 0xD80);
        // Lakes and the sea keep their basins: sediment would fill small lakes, and a
        // lowered shore would no longer hold the water.
        let delta = Field::from_fn(nf, FINE, |i, j| {
            let k = j * nf + i;
            let (x, z) = (i as f64 * FINE, j as f64 * FINE);
            let shore = self
                .lake_dist
                .bilinear(x, z)
                .min(self.sea_dist.bilinear(x, z));
            ((f.data[k] - before[k]).clamp(-7.0, 3.0)) * smoothstep(6.0, 24.0, shore) as f32
        });
        // Typical visits per node are about density × a few dozen; gullies get far more.
        let typical = (p.density * 12.0).max(1.0) as f32;
        let (lo, hi) = ((typical * 2.0).ln(), (typical * 30.0).ln());
        let flow = Field {
            n: nf,
            cell: FINE,
            data: visits
                .iter()
                .map(|&v| (((v as f32 + 1.0).ln() - lo) / (hi - lo)).clamp(0.0, 1.0))
                .collect(),
        };
        Fine { delta, flow }
    }

    /// The lake whose water is nearest (id, weight 0..1 over the surrounding nodes), and
    /// the weight of sea nodes around the column.
    fn standing_water(&self, x: f64, z: f64) -> (Option<(u32, f32)>, f32) {
        let max = (self.n - 1) as f64;
        let (gx, gz) = ((x / CELL).clamp(0.0, max), (z / CELL).clamp(0.0, max));
        let (i, j) = (gx.floor() as usize, gz.floor() as usize);
        let (tx, tz) = ((gx - i as f64) as f32, (gz - j as f64) as f32);
        let (i1, j1) = ((i + 1).min(self.n - 1), (j + 1).min(self.n - 1));
        let corners = [
            (j * self.n + i, (1.0 - tx) * (1.0 - tz)),
            (j * self.n + i1, tx * (1.0 - tz)),
            (j1 * self.n + i, (1.0 - tx) * tz),
            (j1 * self.n + i1, tx * tz),
        ];
        let mut sea = 0.0;
        let mut lakes: [(u32, f32); 4] = [(NONE, 0.0); 4];
        for (k, w) in corners {
            if self.sea[k] {
                sea += w;
            }
            let l = self.lake[k];
            if l != NONE {
                for slot in lakes.iter_mut() {
                    if slot.0 == l || slot.0 == NONE {
                        slot.0 = l;
                        slot.1 += w;
                        break;
                    }
                }
            }
        }
        let best =
            lakes
                .iter()
                .filter(|l| l.0 != NONE && l.1 > 0.0)
                .fold(None::<(u32, f32)>, |b, &l| match b {
                    Some(b) if b.1 >= l.1 => Some(b),
                    _ => Some(l),
                });
        (best, sea)
    }

    /// The highest surface among the lakes that reach this column, and how high the ground
    /// must be to hold them all in. Next to a lake's water (where its nodes weigh at least
    /// 0.175) that is the surface itself; farther out the floor slopes away, so shores blend
    /// into the land instead of ending in a wall.
    fn lake_rim(&self, x: f64, z: f64) -> Option<(i32, f32)> {
        let max = (self.n - 1) as f64;
        let (gx, gz) = ((x / CELL).clamp(0.0, max), (z / CELL).clamp(0.0, max));
        let (i, j) = (gx.floor() as usize, gz.floor() as usize);
        let (tx, tz) = (gx - i as f64, gz - j as f64);
        let (i1, j1) = ((i + 1).min(self.n - 1), (j + 1).min(self.n - 1));
        let mut levels: [(i32, f32); 4] = [(i32::MIN, 0.0); 4];
        for (k, w) in [
            (j * self.n + i, (1.0 - tx) * (1.0 - tz)),
            (j * self.n + i1, tx * (1.0 - tz)),
            (j1 * self.n + i, (1.0 - tx) * tz),
            (j1 * self.n + i1, tx * tz),
        ] {
            if w <= 0.0 || self.lake[k] == NONE {
                continue;
            }
            let s = self.lake_surface[self.lake[k] as usize];
            for slot in levels.iter_mut() {
                if slot.0 == s || slot.0 == i32::MIN {
                    slot.0 = s;
                    slot.1 += w as f32;
                    break;
                }
            }
        }
        let top = levels
            .iter()
            .map(|l| l.0)
            .max()
            .filter(|&s| s != i32::MIN)?;
        let floor = levels
            .iter()
            .filter(|l| l.0 != i32::MIN)
            .map(|&(s, w)| shore_floor(s, w))
            .fold(f32::MIN, f32::max);
        Some((top, floor))
    }

    /// Wetland ground relative to the water surface: hummocks, pools and winding channels.
    fn wetland(&self, x: f64, z: f64, pools: f64) -> f64 {
        let s = self.seed;
        let (wx, wz) = warp(s ^ 0x5A1, x, z, 50.0, 18.0);
        let channel = smoothstep(
            0.86,
            0.97,
            1.0 - perlin(s ^ 0x5A2, wx / 70.0, wz / 70.0).abs(),
        );
        let water = self.sliders.water.clamp(0.0, 1.0) as f64;
        fbm(s ^ 0x5A3, x / 38.0, z / 38.0, 3, 0.5)
            + 0.2 * perlin(s ^ 0x5A4, x / 9.0, z / 9.0)
            + 0.45
            - 0.9 * pools
            - 0.3 * (water - 0.5)
            - 1.1 * channel
    }

    /// How many times steeper than in a standard world the ground at height `h` is (1 in a
    /// standard world).
    fn steeper(&self, h: f64) -> f64 {
        steepening(h - self.sea_surface as f64, self.stretch)
    }

    /// Ground before any water shapes it: the smooth ground with droplet erosion, rock
    /// ledges on steep slopes and wetlands flattened to their water table. It has no
    /// block-scale unevenness, so water edges that follow it are clean lines. Lake and river
    /// levels are fitted to it.
    fn natural(&self, fx: f64, fz: f64, swampy: f64) -> f64 {
        let mut hs = self.smooth_ground(fx, fz);
        if let Some(fine) = &self.fine {
            hs += fine.delta.bilinear(fx, fz);
        }

        // Rock ledges on steep, rocky slopes.
        let patches = smoothstep(-0.1, 0.4, perlin(self.seed ^ 0x7E1, fx / 70.0, fz / 70.0));
        let k = self.steeper(hs);
        let ledges = self.terrace.bilinear(fx, fz)
            * smoothstep(0.6, 1.2, self.steep.bilinear(fx, fz) / k)
            * patches;
        if ledges > 0.01 {
            let step = (7.0 + 3.0 * perlin(self.seed ^ 0x7E2, fx / 90.0, fz / 90.0)) * k.sqrt();
            let offset = 2.0 * k.sqrt() * perlin(self.seed ^ 0x7E3, fx / 40.0, fz / 40.0);
            let q = (hs + offset) / step;
            let stepped = (q.floor() + smoothstep(0.6, 1.0, q - q.floor())) * step - offset;
            hs = lerp(hs, stepped, ledges * 0.6);
        }

        // Wetland cores sit at one water table for the whole map; deeper hollows (lake
        // beds, river valleys) keep their shape.
        if swampy > 0.05 {
            let table = self.table.bilinear(fx, fz);
            let off = (table - self.wet_table).abs();
            let p = self.wet_table.floor();
            let core = smoothstep(0.45, 0.25, off);
            let floor = lerp(table - 0.5, p - 1.0, core);
            let target = floor + 1.0 + self.wetland(fx, fz, self.pools.bilinear(fx, fz));
            let keep = smoothstep(1.5, 3.0, table - hs);
            hs = lerp(hs, target, smoothstep(0.15, 0.7, swampy) * (1.0 - keep));
        }
        hs
    }

    fn sample(&self, x: i32, z: i32) -> Sample {
        let mut s = self.base_sample(x, z);
        self.brushes.apply(self, x, z, &mut s);
        s
    }

    /// A column before roads and waterfalls.
    fn base_sample(&self, x: i32, z: i32) -> Sample {
        let (fx, fz) = (x as f64, z as f64);
        let swampy = self.swamp.bilinear(fx, fz);
        let mut hs = self.natural(fx, fz, swampy);

        // Wetlands: pools where the flattened ground dips below the water table.
        let (lake, sea) = self.standing_water(fx, fz);
        // Where the reaches of two lakes meet, dry ground holds back the higher one.
        let rim_water = self.lake_rim(fx, fz);
        let rim = rim_water.map(|r| r.0);
        let lake_floor = rim_water.map(|r| r.1);
        let sea_floor = (sea > 0.0).then(|| shore_floor(self.sea_surface, sea));
        let mut pool = None;
        let mut pool_rim = None;
        if swampy > 0.05 {
            let table = self.table.bilinear(fx, fz);
            // Pools fill to `p`, the block below the water table.
            let p = self.wet_table.floor();
            // Pools reach down into hollows that hold a lake at the same level; elsewhere
            // they stop a little below the table, where a low rim holds them in.
            let rel = table - self.wet_table;
            let near = smoothstep(48.0, 12.0, self.wet_lake_dist.bilinear(fx, fz));
            let lowest = -1.0 - 4.0 * near + 0.3 * perlin(self.seed ^ 0x5A6, fx / 23.0, fz / 23.0);
            // A lake lower than the pools keeps a rim of its own between them.
            let lower_lake = rim.is_some_and(|r| r < p as i32);
            let in_lower_lake = lower_lake && lake.is_some_and(|l| l.1 >= 0.3);
            if lower_lake {
                if swampy > 0.45 && !in_lower_lake {
                    hs = hs.max(p);
                    pool_rim = Some(p as f32);
                }
            } else if swampy > 0.6 && rel < 0.35 && (rel > lowest || rim.is_some()) {
                pool = Some(p as i32);
            } else if swampy > 0.45 && rel < 1.2 && rel > lowest - 0.6 {
                // The rim of a wetland core holds its pools in.
                hs = hs.max(p);
                pool_rim = Some(p as f32);
            }
        }

        // Lakes and sea: inside their reach the water fills whatever lies below its
        // surface; on the rim around that, the ground is at least as high as the water.
        let lake_surface = lake.map(|(l, _)| self.lake_surface[l as usize]);
        // A lake fills only where no higher lake reaches, so two lakes never touch at
        // different levels.
        let lake_core = lake.is_some_and(|l| l.1 >= 0.3) && lake_surface >= rim;
        let sea_core = sea >= 0.3;
        if let (Some(r), Some(f)) = (rim, lake_floor) {
            // Wetland pools at the same level hold the water in on their own.
            if !lake_core && pool.is_none_or(|p| p < r) {
                hs = hs.max(f as f64);
            }
        }
        if let (Some(f), false) = (sea_floor, sea_core) {
            hs = hs.max(f as f64);
        }

        let mut hs = hs as f32;
        let mut h =
            hs + (self.rough.bilinear(fx, fz) * self.detail_amp * self.micro(fx, fz)) as f32;
        let cut = self.rivers.at(fx, fz);
        if let Some(c) = &cut {
            let jitter = (perlin(self.seed ^ 0x51F, fx / 13.0, fz / 13.0)
                + 0.5 * perlin(self.seed ^ 0x520, fx / 5.0, fz / 5.0))
                as f32;
            hs = c.apply_rough(hs, jitter);
            h = c.apply_rough(h, jitter);
        }
        let channel = cut.and_then(|c| c.channel);
        let wet_below = |surface: i32| ground_y(hs) < surface;
        let (water, surface) = match () {
            // A lake inside a wetland is open water where deep and swamp where shallow.
            _ if lake_core
                && pool == lake_surface
                && hs > pool.unwrap() as f32 - 1.5
                && wet_below(pool.unwrap()) =>
            {
                (WaterKind::Swamp, pool)
            }
            _ if lake_core && wet_below(lake_surface.unwrap()) => (WaterKind::Lake, lake_surface),
            _ if sea_core && wet_below(self.sea_surface) => {
                (WaterKind::Sea, Some(self.sea_surface))
            }
            _ if channel.is_some_and(|c| ground_y(h) < c) => (WaterKind::River, channel),
            _ if pool.is_some_and(wet_below) => (WaterKind::Swamp, pool),
            _ => (WaterKind::None, None),
        };
        match surface {
            // Under water: the bed stays below the surface.
            Some(s) => h = h.min(s as f32 - 0.07),
            None => {
                // Dry ground next to water is at least as high as that water.
                let mut floor = f32::MIN;
                if let Some(f) = lake_floor {
                    floor = floor.max(f);
                }
                if let Some(f) = sea_floor {
                    floor = floor.max(f);
                }
                if let Some(p) = pool {
                    floor = floor.max(p as f32);
                }
                if let Some(p) = pool_rim {
                    floor = floor.max(p);
                }
                if let Some(b) = cut.and_then(|c| c.bank) {
                    floor = floor.max(b as f32);
                }
                h = h.max(floor);
            }
        }
        let h = h.clamp(MIN_Y as f32 + 4.0, self.top);

        // Beaches: low, gentle ground beside the sea or a large lake.
        let mut beach = 0.0;
        if water == WaterKind::None {
            let n01 = 0.5 + 0.5 * perlin(self.seed ^ 0xBEA, fx / 45.0, fz / 45.0) as f32;
            let above = h - self.sea_surface as f32;
            let reach = smoothstep(
                14.0 + 14.0 * n01 as f64,
                6.0,
                self.sea_dist.bilinear(fx, fz),
            ) as f32;
            if reach > 0.0 && above < 1.2 + 1.6 * n01 {
                beach = (1.0 - (above / (1.2 + 1.6 * n01)).max(0.0)) * reach;
            } else if let (Some((l, _)), Some(ls)) = (lake, lake_surface) {
                if n01 > 0.55
                    && swampy < 0.3
                    && self.lake_nodes[l as usize] >= 24
                    && h < ls as f32 + 1.0
                {
                    beach = 0.7;
                }
            }
        }

        Sample {
            h,
            water,
            level: surface.map(|s| s - 1),
            river_edge: cut.map_or(1e9, |c| c.edge),
            beach,
            swampy: swampy as f32,
            fall: match (water, cut.and_then(|c| c.fall), surface) {
                (WaterKind::River, Some(top), Some(s)) if top > s => Some(top - 1),
                _ => None,
            },
            road: None,
        }
    }

    /// The two strongest region types at a column and the second one's blend weight
    /// (0..0.5), with borders made ragged by noise.
    fn regions_at(&self, x: f64, z: f64) -> (u8, u8, f32) {
        let s = self.seed;
        let a = self.blend_warp;
        let wx = x + a
            * (perlin(s ^ 0xA1, x / 37.0, z / 37.0) + 0.45 * perlin(s ^ 0xA2, x / 11.0, z / 11.0));
        let wz = z + a
            * (perlin(s ^ 0xA3, x / 37.0, z / 37.0) + 0.45 * perlin(s ^ 0xA4, x / 11.0, z / 11.0));
        let max = (self.n - 1) as f64;
        let (gx, gz) = ((wx / CELL).clamp(0.0, max), (wz / CELL).clamp(0.0, max));
        let (i, j) = (gx.floor() as usize, gz.floor() as usize);
        let (tx, tz) = ((gx - i as f64) as f32, (gz - j as f64) as f32);
        let (i1, j1) = ((i + 1).min(self.n - 1), (j + 1).min(self.n - 1));
        let corners = [
            (j * self.n + i, (1.0 - tx) * (1.0 - tz)),
            (j * self.n + i1, tx * (1.0 - tz)),
            (j1 * self.n + i, (1.0 - tx) * tz),
            (j1 * self.n + i1, tx * tz),
        ];
        let mut acc: [(u8, f32); 8] = [(0, 0.0); 8];
        let mut used = 0;
        for (k, w) in corners {
            let t = &self.top2[k];
            for slot in 0..2 {
                let (c, v) = (t.cat[slot], t.weight[slot] * w);
                match acc[..used].iter_mut().find(|a| a.0 == c) {
                    Some(a) => a.1 += v,
                    None => {
                        acc[used] = (c, v);
                        used += 1;
                    }
                }
            }
        }
        let mut first = (acc[0].0, -1.0f32);
        let mut second = (acc[0].0, -1.0f32);
        for &(c, v) in &acc[..used] {
            if v > first.1 {
                second = first;
                first = (c, v);
            } else if v > second.1 {
                second = (c, v);
            }
        }
        if second.1 <= 0.0 {
            return (first.0, first.0, 0.0);
        }
        (first.0, second.0, second.1 / (first.1 + second.1))
    }

    fn finish(&self, x: i32, z: i32, s: &Sample, slope: f32) -> ColumnInfo {
        let (fx, fz) = (x as f64, z as f64);
        let h = s.h;
        // Biomes and rock go by how steep the ground is for its kind of world, so the tall
        // world's steeper mountains are dressed like a standard world's.
        let feel = slope / self.steeper(h as f64) as f32;
        let jitter = perlin(self.seed ^ 0x717, fx / 31.0, fz / 31.0) as f32;
        let bank_noise = if s.river_edge < 7.0 {
            0.5 + 0.5 * perlin(self.seed ^ 0xBA4, fx / 9.0, fz / 9.0) as f32
        } else {
            0.0
        };
        let (c1, c2, t) = self.regions_at(fx, fz);
        let resolve = |c: u8| match self.mix.categories[c as usize].biome {
            Some(b) => (b, b, 0.0),
            None => self.bands.resolve(h, feel, jitter),
        };
        let (b1, o1, t1) = resolve(c1);
        let (mut biome, mut biome2, mut blend) = if c2 != c1 && t >= t1 {
            (b1, resolve(c2).0, t)
        } else {
            (b1, o1, t1)
        };

        // Hand-made mountains: rock on steep or high ground, snow above the snow line.
        let lift = self.lift.bilinear(fx, fz) as f32;
        if lift > 20.0 && !is_mountain(biome) {
            let (band, _, _) = self.edit_bands.resolve(h, feel, jitter);
            let snowy = band == Biome::SnowyPeaks;
            if snowy || feel > 0.75 || lift > 70.0 {
                biome2 = biome;
                biome = if snowy || !self.edit_bands.members.contains(&Biome::Mountains) {
                    band
                } else {
                    Biome::Mountains
                };
                blend = 0.5 * (1.0 - smoothstep(20.0, 45.0, lift as f64) as f32);
            }
        }

        // Biome zones painted by hand.
        for zone in &self.zones {
            let inside = zone.inside(fx, fz) as f32;
            if inside > 0.5 {
                if zone.biome != biome {
                    biome2 = biome;
                    biome = zone.biome;
                }
                blend = (1.0 - inside).min(0.5);
            } else if inside > 0.0 && inside > blend && zone.biome != biome {
                biome2 = zone.biome;
                blend = inside;
            }
        }
        let land = biome;

        match s.water {
            WaterKind::River => {
                biome = Biome::River;
                biome2 = land;
                blend = 0.0;
            }
            WaterKind::Lake | WaterKind::Sea => {
                biome = Biome::Lake;
                biome2 = land;
                blend = 0.0;
            }
            WaterKind::Swamp => {}
            WaterKind::None => {
                if s.beach > 0.0 && slope < 0.6 && !is_mountain(land) {
                    biome = Biome::Beach;
                    biome2 = land;
                    blend = 0.5 * (1.0 - s.beach.min(1.0));
                } else if s.river_edge < 5.0 {
                    // The bank band narrows and widens along the river.
                    let band = 1.0 + 3.5 * bank_noise;
                    let b = 0.5 * (1.0 - s.river_edge.max(0.0) / band).max(0.0).powi(2);
                    if b > blend {
                        biome2 = Biome::River;
                        blend = b;
                    }
                }
            }
        }
        if biome2 == biome {
            blend = 0.0;
        }

        let wet = s.water != WaterKind::None;
        let drain = self.drain.bilinear(fx, fz) as f32;
        let near_water = s
            .river_edge
            .max(0.0)
            .min(self.sea_dist.bilinear(fx, fz) as f32)
            .min(self.lake_dist.bilinear(fx, fz) as f32);
        let bias = match land {
            Biome::Swamp | Biome::Marsh => 0.25,
            Biome::Taiga | Biome::Forest => 0.05,
            Biome::PineForest => -0.12,
            Biome::Mountains => -0.1,
            Biome::Beach => -0.1,
            _ => 0.0,
        };
        let moisture = if wet {
            1.0
        } else {
            0.22 + 0.36 * self.wet.bilinear(fx, fz) as f32
                + 0.42 * (-near_water / 14.0).exp()
                + 0.2 * smoothstep(0.3, 0.8, drain as f64) as f32
                + 0.45 * s.swampy
                - 0.25 * smoothstep(0.45, 1.3, slope as f64) as f32
                + bias
        }
        .clamp(0.0, 1.0);

        let steep = smoothstep(0.7, 1.45, feel as f64) as f32;
        let outcrop = (perlin(self.seed ^ 0x0C, fx / 17.0, fz / 17.0) as f32 - 0.42).max(0.0) * 1.3;
        let mut rock = 0.9 * steep + 0.08 * jitter;
        rock += match land {
            Biome::Mountains => 0.12 + 0.35 * smoothstep(0.25, 0.7, feel as f64) as f32 + outcrop,
            Biome::SnowyPeaks => 0.1,
            Biome::AlpineMeadow | Biome::Highlands => outcrop,
            Biome::PineForest | Biome::Taiga => 0.4 * outcrop,
            _ => 0.0,
        };
        let rock = rock.clamp(0.0, 1.0);

        let mut flow = 0.55 * smoothstep(0.32, 0.75, drain as f64) as f32;
        if let Some(f) = &self.fine {
            flow = flow.max(f.flow.bilinear(fx, fz) as f32);
        }
        if s.river_edge < 7.0 {
            let reach = 2.0 + 5.0 * bank_noise;
            flow = flow.max(0.85 * (1.0 - s.river_edge.max(0.0) / reach).max(0.0).powf(1.5));
        }
        if s.water == WaterKind::River {
            flow = 1.0;
        }

        ColumnInfo {
            height: h,
            water_level: s.level,
            water: s.water,
            biome,
            biome2,
            blend: blend.clamp(0.0, 0.5),
            slope,
            moisture,
            rock,
            flow: flow.clamp(0.0, 1.0),
            fall: s.fall,
            road: s.road,
        }
    }

    pub fn column(&self, x: i32, z: i32) -> ColumnInfo {
        let s = self.sample(x, z);
        let slope = slope(
            self.sample(x - 1, z).h,
            self.sample(x + 1, z).h,
            self.sample(x, z - 1).h,
            self.sample(x, z + 1).h,
        );
        self.finish(x, z, &s, slope)
    }

    pub fn grid(&self, rect: Rect) -> ColumnGrid {
        let outer = rect.expand(1);
        let d = outer.depth();
        let samples: Vec<Sample> = outer.columns().map(|(x, z)| self.sample(x, z)).collect();
        let at = |x: i32, z: i32| &samples[((x - outer.x0) * d + (z - outer.z0)) as usize];
        ColumnGrid::build(rect, |x, z| {
            let slope = slope(
                at(x - 1, z).h,
                at(x + 1, z).h,
                at(x, z - 1).h,
                at(x, z + 1).h,
            );
            self.finish(x, z, at(x, z), slope)
        })
    }
}

fn slope(west: f32, east: f32, north: f32, south: f32) -> f32 {
    ((east - west) * 0.5).hypot((south - north) * 0.5)
}

#[cfg(test)]
mod tests;
