//! Render mode for an opened world: the chosen square read block by block, then packed by
//! the same code as a generated map's scene ([`crate::render::assemble`]).
//!
//! A generated map is solid from its surface down. A real world has caves, so "everything
//! under the first gap is hidden" would keep whole cave systems. Instead the open space that
//! can be reached from the sky is flooded (down through water, under crowns and overhangs,
//! into cave mouths), and each column is kept from the lowest block that touches that space.
//! What lies below can't be seen and is left solid, as the scene format expects.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Instant;

use anyhow::{bail, Result};
use vistaforge_core::catalog::Catalog;
use vistaforge_core::column::Rect;
use vistaforge_core::world::MIN_Y;

use super::blocks::WATER_PLANTS;
use super::chunk::Chunk;
use super::map::column_start;
use super::{ChunkRead, OpenWorld};
use crate::render::library::{Kind, Library, RenderBlock};
use crate::render::{assemble, SceneRequest, Unit, TOP_BIT};

/// Side of the squares the area is read in, in blocks.
const UNIT: i32 = 128;
/// Where the scene header puts the sea when the world doesn't say.
const SEA_LEVEL: i32 = 63;

const SOLID: u8 = 1;
const WET: u8 = 2;
const AIR: u8 = 4;

/// How each block id renders, worked out through the library as ids turn up. Workers share
/// it; the library itself is only touched under the lock.
struct Kinds<'a> {
    world: &'a OpenWorld,
    catalog: &'a Catalog,
    library: Mutex<&'a mut Library>,
    resolved: RwLock<(Vec<Arc<RenderBlock>>, Vec<u8>)>,
}

impl<'a> Kinds<'a> {
    fn new(world: &'a OpenWorld, catalog: &'a Catalog, library: &'a mut Library) -> Self {
        Self {
            world,
            catalog,
            library: Mutex::new(library),
            resolved: RwLock::new((Vec::new(), Vec::new())),
        }
    }

    /// Flags for every id the world has met so far.
    fn flags(&self) -> Vec<u8> {
        let known = self.world.blocks.len();
        if self.resolved.read().unwrap().1.len() < known {
            let mut library = self.library.lock().unwrap();
            let mut resolved = self.resolved.write().unwrap();
            while resolved.1.len() < self.world.blocks.len() {
                let id = resolved.1.len() as u32;
                let state = self.world.blocks.palette.state(id);
                let mut block = library.block(&state, self.catalog);
                // Kelp and seagrass stand in water without saying so.
                if WATER_PLANTS.contains(&state.name.as_str()) && !block.waterlogged {
                    let mut b = (*block).clone();
                    b.waterlogged = true;
                    b.full_cube = false;
                    block = Arc::new(b);
                }
                let mut f = 0;
                if block.full_cube && block.kind == Kind::Solid {
                    f |= SOLID;
                }
                if block.kind == Kind::Water || block.waterlogged {
                    f |= WET;
                }
                if block.kind == Kind::Air {
                    f |= AIR;
                }
                resolved.0.push(block);
                resolved.1.push(f);
            }
        }
        self.resolved.read().unwrap().1.clone()
    }
}

/// Chunks shared by the units that need them, read once and dropped when the last of those
/// units is done.
struct ChunkCache<'w> {
    world: &'w OpenWorld,
    #[allow(clippy::type_complexity)]
    slots: Mutex<HashMap<(i32, i32), (Arc<OnceLock<Option<Arc<Chunk>>>>, usize)>>,
}

fn chunks_of(buffer: &Rect) -> impl Iterator<Item = (i32, i32)> {
    let (cx0, cz0) = (buffer.x0 >> 4, buffer.z0 >> 4);
    let (cx1, cz1) = ((buffer.x1 - 1) >> 4, (buffer.z1 - 1) >> 4);
    (cz0..=cz1).flat_map(move |cz| (cx0..=cx1).map(move |cx| (cx, cz)))
}

impl<'w> ChunkCache<'w> {
    fn new(world: &'w OpenWorld, buffers: &[Rect]) -> Self {
        let mut slots = HashMap::new();
        for b in buffers {
            for key in chunks_of(b) {
                if world.regions.has(key.0, key.1) {
                    slots
                        .entry(key)
                        .or_insert_with(|| (Arc::new(OnceLock::new()), 0))
                        .1 += 1;
                }
            }
        }
        Self {
            world,
            slots: Mutex::new(slots),
        }
    }

    fn get(&self, cx: i32, cz: i32) -> Option<Arc<Chunk>> {
        let cell = self.slots.lock().unwrap().get(&(cx, cz))?.0.clone();
        cell.get_or_init(|| match self.world.chunk(cx, cz) {
            ChunkRead::Chunk(c) => Some(Arc::new(c)),
            _ => None,
        })
        .clone()
    }

    fn release(&self, buffer: &Rect) {
        let mut slots = self.slots.lock().unwrap();
        for key in chunks_of(buffer) {
            if let Some(slot) = slots.get_mut(&key) {
                slot.1 -= 1;
                if slot.1 == 0 {
                    slots.remove(&key);
                }
            }
        }
    }
}

/// A unit's cells by block id, with the biome of each column for its colours.
struct WorldUnit {
    rect: Rect,
    columns: Vec<(i32, u32)>,
    cells: Vec<u32>,
    biomes: Vec<u32>,
}

/// One column of a unit's buffer: its chunk, where it is in it, and where looking stops.
#[derive(Clone, Copy)]
struct Col<'c> {
    chunk: Option<&'c Chunk>,
    x: usize,
    z: usize,
    /// Blocks at and above this Y are left out (the Nether's roof).
    cut: i32,
    top: Option<i32>,
}

impl Col<'_> {
    #[inline]
    fn block(&self, y: i32) -> u32 {
        match self.chunk {
            Some(c) if y < self.cut => c.block(self.x, y, self.z),
            _ => 0,
        }
    }
}

fn read_unit(world: &OpenWorld, cache: &ChunkCache, kinds: &Kinds, rect: Rect) -> WorldUnit {
    let buffer = rect.expand(1);
    let (cx0, cz0) = (buffer.x0 >> 4, buffer.z0 >> 4);
    let cw = (((buffer.x1 - 1) >> 4) - cx0 + 1) as usize;
    let chunks: Vec<Option<Arc<Chunk>>> = chunks_of(&buffer)
        .map(|(cx, cz)| cache.get(cx, cz))
        .collect();
    let flags = kinds.flags();
    let air: Vec<bool> = flags.iter().map(|f| f & AIR != 0).collect();
    let flag = |id: u32| flags.get(id as usize).copied().unwrap_or(SOLID);

    let (w, d) = (buffer.width() as usize, buffer.depth() as usize);
    let mut cols: Vec<Col> = Vec::with_capacity(w * d);
    for z in buffer.z0..buffer.z1 {
        for x in buffer.x0..buffer.x1 {
            let k = (((z >> 4) - cz0) as usize) * cw + ((x >> 4) - cx0) as usize;
            let chunk = chunks[k].as_deref();
            let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
            let (cut, top) = match chunk {
                Some(c) => {
                    let cut = column_start(c, lx, lz, world.roof, &air);
                    (cut, c.top(lx, lz, cut, &air))
                }
                None => (i32::MIN, None),
            };
            cols.push(Col {
                chunk,
                x: lx,
                z: lz,
                cut,
                top,
            });
        }
    }
    let lo = chunks
        .iter()
        .flatten()
        .map(|c| c.y_range().0)
        .min()
        .unwrap_or(MIN_Y)
        .max(MIN_Y);
    let hi = cols.iter().filter_map(|c| c.top).max().unwrap_or(lo) + 2;
    let h = (hi - lo) as usize;
    // Everything at and above a column's sky level is open air under the sky.
    let sky: Vec<i32> = cols
        .iter()
        .map(|c| c.top.map(|t| t + 1).unwrap_or(lo).max(lo))
        .collect();

    // Open space below the sky levels that the sky reaches.
    let mut flooded = vec![0u64; (w * d * h).div_ceil(64)];
    let mut low = sky.clone();
    let mut queue: Vec<(u32, i32)> = Vec::new();
    let open = |c: usize, y: i32| flag(cols[c].block(y)) & SOLID == 0;
    let bit = |c: usize, y: i32| c * h + (y - lo) as usize;
    let visit = |c: usize, y: i32, flooded: &mut Vec<u64>, queue: &mut Vec<(u32, i32)>| {
        if y < lo || y >= sky[c] {
            return;
        }
        let b = bit(c, y);
        if flooded[b / 64] & (1 << (b % 64)) != 0 || !open(c, y) {
            return;
        }
        flooded[b / 64] |= 1 << (b % 64);
        queue.push((c as u32, y));
    };
    let neighbours = |c: usize| {
        let (x, z) = (c % w, c / w);
        [
            (x > 0).then(|| c - 1),
            (x + 1 < w).then(|| c + 1),
            (z > 0).then(|| c - w),
            (z + 1 < d).then(|| c + w),
        ]
    };
    for c in 0..w * d {
        visit(c, sky[c] - 1, &mut flooded, &mut queue);
        for n in neighbours(c).into_iter().flatten() {
            for y in sky[c]..sky[n] {
                visit(n, y, &mut flooded, &mut queue);
            }
        }
    }
    while let Some((c, y)) = queue.pop() {
        let c = c as usize;
        low[c] = low[c].min(y);
        visit(c, y - 1, &mut flooded, &mut queue);
        visit(c, y + 1, &mut flooded, &mut queue);
        for n in neighbours(c).into_iter().flatten() {
            visit(n, y, &mut flooded, &mut queue);
        }
    }

    let mut out = WorldUnit {
        rect,
        columns: Vec::with_capacity(rect.area()),
        cells: Vec::new(),
        biomes: Vec::with_capacity(rect.area()),
    };
    let wet = |id: u32| flag(id) & WET != 0;
    for (x, z) in rect.columns() {
        let c = (z - buffer.z0) as usize * w + (x - buffer.x0) as usize;
        let col = &cols[c];
        let Some(top) = col.top else {
            out.columns.push((MIN_Y, 0));
            out.biomes.push(
                col.chunk
                    .map(|ch| ch.biome(col.x, SEA_LEVEL, col.z))
                    .unwrap_or(0),
            );
            continue;
        };
        // From the lowest block next to open space: under this column's own, or beside a
        // neighbour's.
        let mut bottom = low[c] - 1;
        for n in neighbours(c).into_iter().flatten() {
            bottom = bottom.min(low[n]);
        }
        let bottom = bottom.max(lo).min(top);
        let mut below = col.block(bottom);
        for y in bottom..=top {
            let id = below;
            below = col.block(y + 1);
            let flag = if wet(id) && !wet(below) { TOP_BIT } else { 0 };
            out.cells.push(id | flag);
        }
        out.columns.push((bottom, (top - bottom + 1) as u32));
        out.biomes
            .push(col.chunk.map(|ch| ch.biome(col.x, top, col.z)).unwrap_or(0));
    }
    out
}

/// Builds the scene of the square in `req` (world coordinates). `progress` gets (parts done,
/// parts in all).
pub fn build(
    world: &OpenWorld,
    catalog: &Catalog,
    library: &mut Library,
    req: &SceneRequest,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Vec<u8>> {
    let started = Instant::now();
    let side = req.size.clamp(16, 2048).next_power_of_two() as i32;
    let area = Rect::new(req.x0, req.z0, req.x0 + side, req.z0 + side);
    let mut rects = Vec::new();
    for z in (area.z0..area.z1).step_by(UNIT as usize) {
        for x in (area.x0..area.x1).step_by(UNIT as usize) {
            rects.push(Rect::new(
                x,
                z,
                (x + UNIT).min(area.x1),
                (z + UNIT).min(area.z1),
            ));
        }
    }
    if !rects
        .iter()
        .flat_map(chunks_of)
        .any(|(cx, cz)| world.regions.has(cx, cz))
    {
        bail!("nothing was saved in this part of the world; choose an area on the map");
    }
    let buffers: Vec<Rect> = rects.iter().map(|r| r.expand(1)).collect();
    let cache = ChunkCache::new(world, &buffers);
    let kinds = Kinds::new(world, catalog, library);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<WorldUnit>>> =
        Mutex::new((0..rects.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(&rect) = rects.get(i) else { return };
                let unit = read_unit(world, &cache, &kinds, rect);
                cache.release(&buffers[i]);
                results.lock().unwrap()[i] = Some(unit);
                progress(done.fetch_add(1, Ordering::Relaxed) + 1, rects.len());
            });
        }
    });
    kinds.flags();
    let Kinds {
        library, resolved, ..
    } = kinds;
    let library = library.into_inner().unwrap();
    let blocks = resolved.into_inner().unwrap().0;

    let colors: Vec<[u32; 3]> = world
        .biomes
        .all()
        .iter()
        .map(|b| library.colormaps.colors(b))
        .collect();
    let color = |b: u32| {
        colors
            .get(b as usize)
            .copied()
            .unwrap_or([0x91BD59, 0x77AB2F, 0x3F76E4])
    };
    let units: Vec<Unit> = results
        .into_inner()
        .unwrap()
        .into_iter()
        .flatten()
        .map(|u| Unit {
            rect: u.rect,
            columns: u.columns,
            cells: u.cells,
            grass: u.biomes.iter().map(|&b| color(b)[0]).collect(),
            foliage: u.biomes.iter().map(|&b| color(b)[1]).collect(),
            water: u.biomes.iter().map(|&b| color(b)[2]).collect(),
        })
        .collect();
    assemble(&units, &blocks, library, area, side, SEA_LEVEL, started)
}
