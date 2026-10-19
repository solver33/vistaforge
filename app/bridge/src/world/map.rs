//! Top-down pictures of an opened world: a coarse map of every saved chunk to choose an area
//! on, and the chosen area in the 3D preview's own format.
//!
//! The map is packed for the UI as:
//!
//! | bytes          | content                                                      |
//! |----------------|--------------------------------------------------------------|
//! | 4              | magic `VFM1`                                                 |
//! | 4              | `u32` length of the JSON header ([`MapHeader`]), multiple of 4 |
//! | header         | padded with spaces                                           |
//! | 4·width·height | RGBA, row-major (z, then x); alpha 0 where nothing was saved |

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{bail, Result};
use serde::Serialize;

use super::blocks::{MapBlock, AIR, SEE_THROUGH, TREE, WATER};
use super::chunk::{Chunk, Skipped};
use super::{ChunkRead, OpenWorld};
use crate::render::tint::{ColorMaps, Tint};

pub const MAGIC: &[u8; 4] = b"VFM1";
/// Longest side of the world map in pixels; bigger worlds get several blocks per pixel.
pub const MAX_MAP: i32 = 2048;

/// What a map lookup needs to know of every block and biome seen so far.
pub struct Look {
    pub info: Vec<MapBlock>,
    pub air: Vec<bool>,
    /// Grass, foliage and water colour per biome id.
    pub colors: Vec<[u32; 3]>,
}

impl Look {
    pub fn new(world: &OpenWorld, maps: &ColorMaps) -> Self {
        let info = world.blocks.snapshot();
        let air = info.iter().map(|b| b.is(AIR)).collect();
        let colors = world.biomes.all().iter().map(|b| maps.colors(b)).collect();
        Self { info, air, colors }
    }

    /// True when `chunk` holds blocks or biomes this look doesn't know yet.
    pub fn stale_for(&self, chunk: &Chunk) -> bool {
        chunk.sections.iter().any(|s| {
            s.blocks
                .palette
                .iter()
                .any(|&id| id as usize >= self.info.len())
                || s.biomes
                    .palette
                    .iter()
                    .any(|&id| id as usize >= self.colors.len())
        })
    }

    fn block(&self, id: u32) -> MapBlock {
        self.info.get(id as usize).copied().unwrap_or_default()
    }

    fn biome(&self, id: u32) -> [u32; 3] {
        self.colors
            .get(id as usize)
            .copied()
            .unwrap_or([0x91BD59, 0x77AB2F, 0x3F76E4])
    }
}

/// Where looking down a column starts, exclusive: the top of the world, or under the
/// Nether's roof (down through the roof to the first air).
pub fn column_start(chunk: &Chunk, x: usize, z: usize, roof: Option<i32>, air: &[bool]) -> i32 {
    let Some(roof) = roof else {
        return i32::MAX;
    };
    let (lo, hi) = chunk.y_range();
    let mut y = roof.min(hi) - 1;
    while y >= lo
        && !air
            .get(chunk.block(x, y, z) as usize)
            .copied()
            .unwrap_or(false)
    {
        y -= 1;
    }
    y + 1
}

/// One column seen from above.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub rgb: [u8; 3],
    /// Y of what is seen: a crown, the water or the ground.
    pub shown: i32,
    /// Y of the top ground block (not leaves, logs, plants or water).
    pub ground: i32,
    /// Y of the top water block, when the column is wet.
    pub water: Option<i32>,
    pub biome: u32,
    pub wet_on_top: bool,
}

fn tinted(b: &MapBlock, biome: [u32; 3]) -> [f32; 3] {
    let rgb = [(b.rgb >> 16) & 0xFF, (b.rgb >> 8) & 0xFF, b.rgb & 0xFF].map(|c| c as f32);
    let t = match b.tint {
        Tint::None => return rgb,
        Tint::Grass => biome[0],
        Tint::Foliage => biome[1],
        Tint::Water => biome[2],
        Tint::Fixed(c) => c,
    };
    let t = [(t >> 16) & 0xFF, (t >> 8) & 0xFF, t & 0xFF].map(|c| c as f32);
    // Averages of tinted textures come out darker than the game looks.
    [0, 1, 2].map(|i| rgb[i] * t[i] / 255.0 * 1.25)
}

/// Column (x, z) of `chunk`, looking down from below `start`.
pub fn sample(chunk: &Chunk, x: usize, z: usize, start: i32, look: &Look) -> Option<Sample> {
    let top = chunk.top(x, z, start, &look.air)?;
    let (lo, _) = chunk.y_range();
    let mut crown: Option<([f32; 3], i32)> = None;
    let mut water: Option<i32> = None;
    let mut ground: Option<i32> = None;
    let mut skips = 0;
    let mut y = top;
    while y >= lo {
        let b = look.block(chunk.block(x, y, z));
        if b.is(AIR) {
            y -= 1;
            continue;
        }
        if b.wet() && water.is_none() {
            water = Some(y);
        }
        if b.is(WATER) || (b.wet() && b.is(SEE_THROUGH)) {
            y -= 1;
            continue;
        }
        if b.is(SEE_THROUGH) && skips < 8 {
            skips += 1;
            y -= 1;
            continue;
        }
        if b.is(TREE) {
            if crown.is_none() {
                crown = Some((tinted(&b, look.biome(chunk.biome(x, y, z))), y));
            }
            y -= 1;
            continue;
        }
        ground = Some(y);
        break;
    }
    let ground_y = ground.unwrap_or(lo);
    let ground_rgb = match ground {
        Some(g) => tinted(
            &look.block(chunk.block(x, g, z)),
            look.biome(chunk.biome(x, g, z)),
        ),
        None => [40.0, 40.0, 40.0],
    };
    let biome = chunk.biome(x, top, z);
    let crown_on_top = crown.filter(|&(_, cy)| water.is_none_or(|w| cy > w));
    let (rgb, shown) = if let Some((c, cy)) = crown_on_top {
        (c, cy)
    } else if let Some(w) = water {
        let depth = (w - ground_y) as f32;
        let tint = look.biome(chunk.biome(x, w, z))[2];
        let tex = [110.0, 120.0, 125.0];
        let t = (0.5 + depth * 0.06).min(0.85);
        let dark = (1.0 - 0.04 * depth).max(0.6);
        let c = [0, 1, 2].map(|i| {
            let tint = ((tint >> (16 - 8 * i)) & 0xFF) as f32;
            let water = tex[i] * tint / 255.0 * 1.6;
            ground_rgb[i] * (1.0 - t) * dark + water * t
        });
        (c, w)
    } else {
        (ground_rgb, ground_y)
    };
    Some(Sample {
        rgb: rgb.map(|v| v.clamp(0.0, 255.0) as u8),
        shown,
        ground: ground_y,
        water,
        biome,
        wet_on_top: crown_on_top.is_none() && water.is_some(),
    })
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MapHeader {
    /// Block of the map's north-west corner.
    pub x0: i32,
    pub z0: i32,
    /// Blocks per pixel.
    pub scale: i32,
    pub width: i32,
    pub height: i32,
    /// Chunks drawn.
    pub chunks: usize,
    /// Chunks saved by Minecraft before 1.18, which the game upgrades only when visited.
    pub old_chunks: usize,
    /// Chunks at the edge of the explored world that the game had not finished.
    pub unfinished: usize,
    pub failed: usize,
    pub seconds: f64,
}

/// `a / b` rounded down, for any sign.
fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

fn ceil_div(a: i32, b: i32) -> i32 {
    -(-a).div_euclid(b)
}

/// A map pixel's index, colour, the Y seen there, and whether it is open water.
type Pixel = (u32, [u8; 3], i32, bool);
/// Sample index and the column in its chunk, by chunk (z, x).
type ByChunk = BTreeMap<(i32, i32), Vec<(usize, usize, usize)>>;

#[derive(Default)]
struct Counts {
    chunks: AtomicUsize,
    old: AtomicUsize,
    unfinished: AtomicUsize,
    failed: AtomicUsize,
}

impl Counts {
    fn note(&self, read: &ChunkRead) {
        let c = match read {
            ChunkRead::Chunk(_) => &self.chunks,
            ChunkRead::Skipped(Skipped::OldFormat) => &self.old,
            ChunkRead::Skipped(Skipped::Unfinished) => &self.unfinished,
            ChunkRead::Failed => &self.failed,
            ChunkRead::Missing => return,
        };
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// A worker's view of the blocks and biomes, refreshed when a chunk brings new ones.
struct Looker<'w> {
    world: &'w OpenWorld,
    maps: &'w ColorMaps,
    look: Look,
}

impl<'w> Looker<'w> {
    fn new(world: &'w OpenWorld, maps: &'w ColorMaps) -> Self {
        Self {
            world,
            maps,
            look: Look::new(world, maps),
        }
    }

    fn for_chunk(&mut self, chunk: &Chunk) -> &Look {
        if self.look.stale_for(chunk) {
            self.look = Look::new(self.world, self.maps);
        }
        &self.look
    }
}

fn workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// A picture of every saved chunk of the world, at most [`MAX_MAP`] pixels a side.
/// `progress` gets (regions done, regions in all); setting `cancel` stops it early.
pub fn overview(
    world: &OpenWorld,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    let Some([bx0, bz0, bx1, bz1]) = world.info.bounds else {
        return Ok(pack_map(&MapHeader::default(), &[]));
    };
    let extent = (bx1 - bx0).max(bz1 - bz0);
    let mut scale = 1;
    while extent / scale > MAX_MAP {
        scale *= 2;
    }
    let x0 = floor_div(bx0, scale) * scale;
    let z0 = floor_div(bz0, scale) * scale;
    let width = ceil_div(bx1 - x0, scale);
    let height = ceil_div(bz1 - z0, scale);
    let half = scale / 2;

    let mut regions: Vec<_> = world.regions.regions.values().collect();
    regions.sort_by_key(|r| (r.rz, r.rx));
    let maps = ColorMaps::load(None);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let counts = Counts::default();
    // Per pixel: colour, the Y seen, and whether it is open water (left unshaded).
    let pixels: Mutex<Vec<Pixel>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers() {
            scope.spawn(|| {
                let mut looker = Looker::new(world, &maps);
                let mut out = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(region) = regions.get(i) else { break };
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    for (cx, cz) in region.chunks() {
                        let (i0, i1) = (
                            ceil_div(cx * 16 - x0 - half, scale),
                            floor_div(cx * 16 + 15 - x0 - half, scale),
                        );
                        let (j0, j1) = (
                            ceil_div(cz * 16 - z0 - half, scale),
                            floor_div(cz * 16 + 15 - z0 - half, scale),
                        );
                        if i0 > i1 || j0 > j1 {
                            continue;
                        }
                        let read = world.chunk(cx, cz);
                        counts.note(&read);
                        let ChunkRead::Chunk(chunk) = read else {
                            continue;
                        };
                        let look = looker.for_chunk(&chunk);
                        for j in j0..=j1 {
                            for i in i0..=i1 {
                                let lx = (x0 + i * scale + half - cx * 16) as usize;
                                let lz = (z0 + j * scale + half - cz * 16) as usize;
                                let start = column_start(&chunk, lx, lz, world.roof, &look.air);
                                if let Some(s) = sample(&chunk, lx, lz, start, look) {
                                    out.push((
                                        (j * width + i) as u32,
                                        s.rgb,
                                        s.shown,
                                        s.wet_on_top,
                                    ));
                                }
                            }
                        }
                    }
                    progress(done.fetch_add(1, Ordering::Relaxed) + 1, regions.len());
                }
                pixels.lock().unwrap().extend(out);
            });
        }
    });
    if cancel.load(Ordering::Relaxed) {
        bail!("cancelled");
    }

    let n = (width * height) as usize;
    let mut rgba = vec![0u8; n * 4];
    let mut heights = vec![i32::MIN; n];
    let mut wet = vec![false; n];
    for (i, rgb, y, w) in pixels.into_inner().unwrap() {
        let i = i as usize;
        rgba[4 * i..4 * i + 3].copy_from_slice(&rgb);
        rgba[4 * i + 3] = 255;
        heights[i] = y;
        wet[i] = w;
    }
    shade(
        &mut rgba,
        &heights,
        &wet,
        width as usize,
        height as usize,
        scale,
    );
    let header = MapHeader {
        x0,
        z0,
        scale,
        width,
        height,
        chunks: counts.chunks.into_inner(),
        old_chunks: counts.old.into_inner(),
        unfinished: counts.unfinished.into_inner(),
        failed: counts.failed.into_inner(),
        seconds: started.elapsed().as_secs_f64(),
    };
    Ok(pack_map(&header, &rgba))
}

/// Light from the north-west: slopes facing it brighter, the others darker, as on the game's
/// own maps but softer. Open water stays flat.
fn shade(rgba: &mut [u8], heights: &[i32], wet: &[bool], w: usize, h: usize, scale: i32) {
    let k = 0.18 / scale as f32;
    let factors: Vec<f32> = (0..w * h)
        .map(|i| {
            let y = heights[i];
            if y == i32::MIN || wet[i] {
                return 1.0;
            }
            let (x, z) = (i % w, i / w);
            let mut d = 0.0;
            let mut n = 0.0;
            if z > 0 && heights[i - w] != i32::MIN {
                d += (y - heights[i - w]) as f32;
                n += 1.0;
            }
            if x > 0 && heights[i - 1] != i32::MIN {
                d += (y - heights[i - 1]) as f32;
                n += 1.0;
            }
            if n == 0.0 {
                return 1.0;
            }
            1.0 + (d / n * k * scale.min(4) as f32).clamp(-0.32, 0.28)
        })
        .collect();
    for (i, f) in factors.into_iter().enumerate() {
        for c in &mut rgba[4 * i..4 * i + 3] {
            *c = (*c as f32 * f).clamp(0.0, 255.0) as u8;
        }
    }
}

fn pad4(out: &mut Vec<u8>, fill: u8) {
    while !out.len().is_multiple_of(4) {
        out.push(fill);
    }
}

fn pack_map(header: &MapHeader, rgba: &[u8]) -> Vec<u8> {
    let mut head = serde_json::to_vec(header).expect("header serialises");
    pad4(&mut head, b' ');
    let mut out = Vec::with_capacity(8 + head.len() + rgba.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(head.len() as u32).to_le_bytes());
    out.extend_from_slice(&head);
    out.extend_from_slice(rgba);
    out
}

/// The square `x0, z0, size` read as the 3D preview reads a generated map: `res` samples a
/// side, packed by [`crate::encode::world_preview`].
pub fn area_preview(world: &OpenWorld, x0: i32, z0: i32, size: u32, res: u32) -> Result<Vec<u8>> {
    let size = size.clamp(16, 4096);
    let res = res.clamp(16, size);
    let step = size as f32 / res as f32;
    let at = |i: u32| (i as f32 + 0.5) * step;
    // Samples by chunk, so each chunk is read once.
    let mut by_chunk = ByChunk::new();
    for j in 0..res {
        for i in 0..res {
            let x = x0 + at(i) as i32;
            let z = z0 + at(j) as i32;
            by_chunk.entry((z >> 4, x >> 4)).or_default().push((
                (j * res + i) as usize,
                (x & 15) as usize,
                (z & 15) as usize,
            ));
        }
    }
    let jobs: Vec<_> = by_chunk.into_iter().collect();
    let maps = ColorMaps::load(None);
    let next = AtomicUsize::new(0);
    let samples: Mutex<Vec<(usize, Sample)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers() {
            scope.spawn(|| {
                let mut looker = Looker::new(world, &maps);
                let mut out = Vec::new();
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(((cz, cx), list)) = jobs.get(k) else {
                        break;
                    };
                    let ChunkRead::Chunk(chunk) = world.chunk(*cx, *cz) else {
                        continue;
                    };
                    let look = looker.for_chunk(&chunk);
                    for &(idx, lx, lz) in list {
                        let start = column_start(&chunk, lx, lz, world.roof, &look.air);
                        if let Some(s) = sample(&chunk, lx, lz, start, look) {
                            out.push((idx, s));
                        }
                    }
                }
                samples.lock().unwrap().extend(out);
            });
        }
    });
    let samples = samples.into_inner().unwrap();
    let n = (res * res) as usize;
    let floor = samples.iter().map(|(_, s)| s.ground).min().unwrap_or(0) as f32;
    let mut heights = vec![floor; n];
    let mut water = vec![None; n];
    let mut colors = vec![[34u8, 39, 44]; n];
    let mut biome = vec![0u8; n];
    let names = world.biomes.all();
    let mut table: Vec<String> = vec!["unexplored".into()];
    let mut index: Vec<Option<u8>> = vec![None; names.len()];
    for (i, s) in samples {
        heights[i] = s.ground as f32 + 1.0;
        water[i] = s.water.map(|w| w as f32 + 1.0);
        colors[i] = s.rgb;
        let b = s.biome as usize;
        biome[i] = match index.get(b).copied().flatten() {
            Some(k) => k,
            None => {
                let name = names.get(b).map(String::as_str).unwrap_or("plains");
                table.push(name.strip_prefix("minecraft:").unwrap_or(name).to_string());
                let k = (table.len() - 1).min(255) as u8;
                if let Some(slot) = index.get_mut(b) {
                    *slot = Some(k);
                }
                k
            }
        };
    }
    Ok(crate::encode::world_preview(
        size, res, step, &heights, &water, &colors, &table, &biome,
    ))
}
