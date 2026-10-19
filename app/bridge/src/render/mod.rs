//! Render mode: a piece of the map with every block, for the app's path tracer.
//!
//! The engine generates the chosen area exactly as an export would. Each block state gets its
//! look from the game's own models and textures, read from the player's install
//! ([`assets`]), or a shape and colour guessed from the catalog when those can't be found.
//! Everything is packed into one buffer the UI uploads to the GPU as it is
//! (`app/src/lib/render/scene.ts` reads it).
//!
//! Layout, little-endian, every section starting on a 4-byte boundary:
//!
//! | bytes            | content                                                            |
//! |------------------|--------------------------------------------------------------------|
//! | 4                | magic `VFR1`                                                       |
//! | 4                | `u32` length of the JSON header, a multiple of 4                   |
//! | header           | [`Header`], padded with spaces                                     |
//! | 16·side²         | columns, row-major (z, then x): `u32` voxel offset, `u32` bottom   |
//! |                  | (Y + 64) \| count << 16, `u32` grass and `u32` foliage 0xRRGGBB    |
//! | 4·side²          | water colour per column: R, G, B, 0                                |
//! | 2·voxels, pad 4  | `u16` render state per cell, bottom up; 0 is air                   |
//! | 48·states        | 3 × `u32`×4 per state: [flags, slots, colour, water tile + 1],     |
//! |                  | then four (quad offset, quad count) pairs                          |
//! | 80·quads         | 5 × `f32`×4 per quad: [o, tile], [u, tint kind], [v, tint colour], |
//! |                  | [uv at o, uv along u], [uv along v, flags, 0]                      |
//! | 4·w·h            | texture atlas, RGBA, `tile`-pixel tiles left to right, top down    |
//!
//! Below a column's first stored cell everything is solid: rays can't reach there except by
//! coming through the bottom cell, which is a full block.

pub mod assets;
pub mod library;
pub mod model;
pub mod texture;
pub mod tint;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use vistaforge_core::catalog::Catalog;
use vistaforge_core::column::Rect;
use vistaforge_core::engine::Generator;
use vistaforge_core::world::MIN_Y;

use library::{Kind, Library, RenderBlock, Stats};
use model::MAX_SLOTS;

pub const MAGIC: &[u8; 4] = b"VFR1";
/// Area sides the UI offers.
pub const SIDES: [u32; 4] = [256, 512, 1024, 2048];
/// More cells than this would not fit the GPU comfortably.
const MAX_VOXELS: usize = 150_000_000;

/// State flags, as the shader reads them.
pub const FULL: u32 = 1;
pub const WATER: u32 = 2;
pub const WATERLOGGED: u32 = 4;
pub const WATER_TOP: u32 = 8;
pub const CUTOUT: u32 = 16;

/// Which part of the map to render, and where the game's files are.
#[derive(Clone, Debug, Deserialize)]
pub struct SceneRequest {
    /// North-west corner of the area.
    pub x0: i32,
    pub z0: i32,
    /// Side of the area in blocks; rounded up to a power of two.
    pub size: u32,
    /// The game folder to read models and textures from; found from the catalog when absent.
    #[serde(default)]
    pub game_dir: Option<PathBuf>,
    /// The catalog file, `<game folder>/vistaforge/catalog.json` when the exporter wrote it.
    #[serde(default)]
    pub catalog_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub game_dir: Option<PathBuf>,
    pub minecraft_jar: bool,
    pub mods: usize,
    pub packs: usize,
    #[serde(flatten)]
    pub stats: Stats,
}

#[derive(Clone, Debug, Serialize)]
pub struct Header {
    pub x0: i32,
    pub z0: i32,
    /// Columns per side of the column table (a power of two).
    pub side: u32,
    /// The part of it on the map.
    pub width: u32,
    pub depth: u32,
    pub min_y: i32,
    /// One above the highest block.
    pub max_y: i32,
    pub sea_level: i32,
    pub voxels: usize,
    pub states: usize,
    pub quads: usize,
    pub tile: u32,
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub tiles: usize,
    pub source: Source,
    pub seconds: f64,
}

/// What render mode keeps between scenes: the game's files, read once.
pub struct Cache {
    key: (Option<PathBuf>, PathBuf),
    library: Library,
}

/// The game folder to read from: the one asked for, the one the catalog was exported in, or
/// a Conquest instance found on this computer.
pub fn game_dir(req: &SceneRequest) -> Option<PathBuf> {
    if let Some(dir) = req.game_dir.as_ref().filter(|d| d.is_dir()) {
        return Some(dir.clone());
    }
    let from_catalog = req
        .catalog_path
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|g| g.join("mods").is_dir());
    if let Some(dir) = from_catalog {
        return Some(dir.to_path_buf());
    }
    crate::paths::detect()
        .into_iter()
        .map(|i| i.game_dir)
        .find(|g| g.join("mods").is_dir())
}

/// The library for this game folder and catalog, reusing the cached one when they match.
pub fn library<'c>(
    cache: &'c mut Option<Cache>,
    req: &SceneRequest,
    catalog: &Catalog,
) -> Result<&'c mut Library> {
    let game = game_dir(req);
    let catalog_path = req.catalog_path.clone().unwrap_or_default();
    let key = (game.clone(), catalog_path.clone());
    if cache.as_ref().is_none_or(|c| c.key != key) {
        let faces = if catalog_path.is_file() {
            library::catalog_faces(&catalog_path)?
        } else {
            HashMap::new()
        };
        let version = catalog
            .mods
            .get("minecraft")
            .cloned()
            .flatten()
            .unwrap_or_else(|| "1.20.1".into());
        let assets = game
            .as_deref()
            .and_then(|g| assets::Assets::open(g, &version).ok())
            .filter(|a| !a.is_empty());
        *cache = Some(Cache {
            key,
            library: Library::new(assets, faces),
        });
    }
    Ok(&mut cache.as_mut().unwrap().library)
}

/// One generated unit of the area: its columns' cells as palette ids (the high bit marks the
/// top cell of water), and their colours.
pub(crate) struct Unit {
    pub rect: Rect,
    /// Per column, x-major like `Rect::columns`: bottom Y and cell count.
    pub columns: Vec<(i32, u32)>,
    pub cells: Vec<u32>,
    pub grass: Vec<u32>,
    pub foliage: Vec<u32>,
    pub water: Vec<u32>,
}

pub(crate) const TOP_BIT: u32 = 1 << 31;

/// Builds and packs the scene for `req`. `progress` gets (units done, units in all).
pub fn build(
    generator: &Generator,
    catalog: &Catalog,
    library: &mut Library,
    req: &SceneRequest,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Vec<u8>> {
    let started = Instant::now();
    let size = generator.size();
    let side = req.size.clamp(16, 2048).next_power_of_two() as i32;
    let area =
        Rect::new(req.x0, req.z0, req.x0 + side, req.z0 + side).intersect(&Rect::square(size));
    if area.is_empty() {
        bail!("the render area is outside the map");
    }

    // What each palette entry is, for working out where the hidden ground starts.
    let resolve = |library: &mut Library, from: usize| -> Vec<Arc<RenderBlock>> {
        generator
            .palette
            .states()
            .iter()
            .skip(from)
            .map(|s| library.block(s, catalog))
            .collect()
    };
    let mut blocks = resolve(library, 0);
    let solid: Vec<bool> = blocks
        .iter()
        .map(|b| b.full_cube && b.kind == Kind::Solid)
        .collect();
    let wet: Vec<bool> = blocks
        .iter()
        .map(|b| b.kind == Kind::Water || b.waterlogged)
        .collect();

    let mut units = Vec::new();
    for x in (area.x0..area.x1).step_by(64) {
        for z in (area.z0..area.z1).step_by(64) {
            units.push(Rect::new(
                x,
                z,
                (x + 64).min(area.x1),
                (z + 64).min(area.z1),
            ));
        }
    }
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Unit>>> = Mutex::new((0..units.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let colors = &library.colormaps;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(&rect) = units.get(i) else { return };
                let unit = generate_unit(generator, colors, rect, &solid, &wet);
                results.lock().unwrap()[i] = Some(unit);
                progress(done.fetch_add(1, Ordering::Relaxed) + 1, units.len());
            });
        }
    });
    let units: Vec<Unit> = results
        .into_inner()
        .unwrap()
        .into_iter()
        .flatten()
        .collect();

    // States a module added while generating, if any.
    if generator.palette.len() > blocks.len() {
        let more = resolve(library, blocks.len());
        blocks.extend(more);
    }
    assemble(
        &units,
        &blocks,
        library,
        area,
        side,
        generator.settings.sea_level,
        started,
    )
}

/// Packs the units of `area` into a scene. `blocks` says how each id in the units' cells
/// renders. Generated maps and opened worlds both end here.
pub(crate) fn assemble(
    units: &[Unit],
    blocks: &[Arc<RenderBlock>],
    library: &mut Library,
    area: Rect,
    side: i32,
    sea_level: i32,
    started: Instant,
) -> Result<Vec<u8>> {
    // Render states in palette order: the plain look of each id, and its water-top look.
    let mut used: BTreeMap<u32, u16> = BTreeMap::new();
    for u in units {
        for &c in &u.cells {
            used.entry(c).or_insert(0);
        }
    }
    used.remove(&0);
    used.retain(|&c, _| blocks[(c & !TOP_BIT) as usize].kind != Kind::Air);
    if used.len() >= u16::MAX as usize {
        bail!("too many different blocks in this area");
    }
    for (i, v) in used.values_mut().enumerate() {
        *v = i as u16 + 1;
    }

    // Columns and cells.
    let s = side as usize;
    let mut columns = vec![0u32; s * s * 4];
    let mut water = vec![0u8; s * s * 4];
    let mut voxels: Vec<u16> = Vec::new();
    let mut grass = vec![0u32; s * s];
    let mut foliage = vec![0u32; s * s];
    let mut water_rgb = vec![0u32; s * s];
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for u in units {
        let mut at = 0usize;
        for (k, (x, z)) in u.rect.columns().enumerate() {
            let (bottom, count) = u.columns[k];
            let i = (z - area.z0) as usize * s + (x - area.x0) as usize;
            if voxels.len() + count as usize > MAX_VOXELS {
                bail!("this area holds too many blocks to render; choose a smaller one");
            }
            columns[4 * i] = voxels.len() as u32;
            columns[4 * i + 1] = ((bottom - MIN_Y) as u32) | count << 16;
            voxels.extend(
                u.cells[at..at + count as usize]
                    .iter()
                    .map(|c| used.get(c).copied().unwrap_or(0)),
            );
            at += count as usize;
            if count > 0 {
                min_y = min_y.min(bottom);
                max_y = max_y.max(bottom + count as i32);
            }
            grass[i] = u.grass[k];
            foliage[i] = u.foliage[k];
            water_rgb[i] = u.water[k];
        }
    }
    let (w, d) = (area.width() as usize, area.depth() as usize);
    let blend = |c: &[u32]| {
        // Blend inside the map only: copy the on-map part out and back.
        let mut part = Vec::with_capacity(w * d);
        for z in 0..d {
            part.extend_from_slice(&c[z * s..z * s + w]);
        }
        let side_max = w.max(d);
        let mut sq = vec![0u32; side_max * side_max];
        for z in 0..d {
            sq[z * side_max..z * side_max + w].copy_from_slice(&part[z * w..(z + 1) * w]);
        }
        let b = tint::blend(&sq, side_max);
        let mut out = c.to_vec();
        for z in 0..d {
            out[z * s..z * s + w].copy_from_slice(&b[z * side_max..z * side_max + w]);
        }
        out
    };
    let (grass, foliage, water_rgb) = (blend(&grass), blend(&foliage), blend(&water_rgb));
    for i in 0..s * s {
        columns[4 * i + 2] = grass[i];
        columns[4 * i + 3] = foliage[i];
        water[4 * i..4 * i + 3].copy_from_slice(&[
            (water_rgb[i] >> 16) as u8,
            (water_rgb[i] >> 8) as u8,
            water_rgb[i] as u8,
        ]);
    }

    // States, quads and the textures they use.
    let water_texture = library_water_texture(library);
    let mut tiles: BTreeMap<String, u32> = BTreeMap::new();
    let entries: Vec<(&u32, &u16)> = used.iter().collect();
    for (&c, _) in &entries {
        let b = &blocks[(c & !TOP_BIT) as usize];
        for f in b.slots.iter().flatten() {
            tiles.entry(f.quad.texture.clone()).or_insert(0);
        }
        if b.kind == Kind::Water || b.waterlogged {
            tiles.entry(water_texture.clone()).or_insert(0);
        }
    }
    let textures: Vec<(String, Arc<texture::Texture>)> = tiles
        .keys()
        .map(|k| {
            let t = library
                .texture(k)
                .unwrap_or_else(|| Arc::new(texture::Texture::solid(0xFF00FF)));
            (k.clone(), t)
        })
        .collect();
    for (i, v) in tiles.values_mut().enumerate() {
        *v = i as u32;
    }
    let opaque: HashMap<&str, bool> = textures
        .iter()
        .map(|(k, t)| (k.as_str(), t.opaque()))
        .collect();
    let tile = textures
        .iter()
        .map(|(_, t)| t.width.max(t.height))
        .max()
        .unwrap_or(16)
        .clamp(16, 64)
        .next_power_of_two();

    let mut states = vec![0u32; 12]; // state 0: air
    let mut quads: Vec<f32> = Vec::new();
    for (&c, _) in &entries {
        let b = &blocks[(c & !TOP_BIT) as usize];
        let mut flags = 0;
        if b.full_cube {
            flags |= FULL;
        }
        if b.kind == Kind::Water {
            flags |= WATER;
        }
        if b.waterlogged {
            flags |= WATERLOGGED;
        }
        if c & TOP_BIT != 0 {
            flags |= WATER_TOP;
        }
        let mut pairs = [0u32; 8];
        let slots = b.slots.len().clamp(1, MAX_SLOTS);
        for (k, slot) in b.slots.iter().take(MAX_SLOTS).enumerate() {
            pairs[2 * k] = (quads.len() / 20) as u32;
            pairs[2 * k + 1] = slot.len() as u32;
            for f in slot {
                let q = &f.quad;
                let cutout = !opaque.get(q.texture.as_str()).copied().unwrap_or(true);
                if cutout {
                    flags |= CUTOUT;
                }
                let (kind, rgb) = f.tint.code();
                quads.extend_from_slice(&[
                    q.o[0],
                    q.o[1],
                    q.o[2],
                    tiles[&q.texture] as f32,
                    q.u[0],
                    q.u[1],
                    q.u[2],
                    kind,
                    q.v[0],
                    q.v[1],
                    q.v[2],
                    rgb as f32,
                    q.uv[0][0],
                    q.uv[0][1],
                    q.uv[1][0],
                    q.uv[1][1],
                    q.uv[2][0],
                    q.uv[2][1],
                    if cutout { 1.0 } else { 0.0 },
                    0.0,
                ]);
            }
        }
        let water_tile = if b.kind == Kind::Water || b.waterlogged {
            tiles[&water_texture] + 1
        } else {
            0
        };
        states.extend_from_slice(&[flags, slots as u32, b.color, water_tile]);
        states.extend_from_slice(&pairs);
    }

    let per_row = 2048 / tile;
    let rows = (textures.len() as u32).div_ceil(per_row).max(1);
    let atlas_width = tile * per_row.min(textures.len().max(1) as u32);
    let atlas_height = rows * tile;
    let mut atlas = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    for (i, (_, t)) in textures.iter().enumerate() {
        let px = t.resampled(tile);
        let (tx, ty) = ((i as u32 % per_row) * tile, (i as u32 / per_row) * tile);
        for row in 0..tile {
            let dst = (((ty + row) * atlas_width + tx) * 4) as usize;
            let src = (row * tile * 4) as usize;
            atlas[dst..dst + (tile * 4) as usize]
                .copy_from_slice(&px[src..src + (tile * 4) as usize]);
        }
    }

    // How the blocks in this area are drawn: from the game's models, or as stand-ins.
    let shown: BTreeSet<u32> = used
        .keys()
        .map(|c| c & !TOP_BIT)
        .filter(|&c| blocks[c as usize].kind == Kind::Solid)
        .collect();
    let mut stats = library.stats.clone();
    stats.models = shown
        .iter()
        .filter(|&&c| blocks[c as usize].from_model)
        .count();
    stats.fallback = shown.len() - stats.models;

    let found = library
        .assets
        .as_ref()
        .map(|a| a.found.clone())
        .unwrap_or_default();
    let header = Header {
        x0: area.x0,
        z0: area.z0,
        side: side as u32,
        width: w as u32,
        depth: d as u32,
        min_y: if min_y == i32::MAX { MIN_Y } else { min_y },
        max_y: if max_y == i32::MIN { MIN_Y + 1 } else { max_y },
        sea_level,
        voxels: voxels.len(),
        states: states.len() / 12,
        quads: quads.len() / 20,
        tile,
        atlas_width,
        atlas_height,
        tiles: textures.len(),
        source: Source {
            game_dir: found.game_dir,
            minecraft_jar: found.minecraft_jar.is_some(),
            mods: found.mods,
            packs: found.packs,
            stats,
        },
        seconds: started.elapsed().as_secs_f64(),
    };
    Ok(pack(
        &header, &columns, &water, &voxels, &states, &quads, &atlas,
    ))
}

fn library_water_texture(library: &mut Library) -> String {
    let id = "minecraft:block/water_still";
    if library.texture(id).is_some() {
        id.to_string()
    } else {
        "#solid:6e787d".to_string()
    }
}

fn generate_unit(
    generator: &Generator,
    colors: &tint::ColorMaps,
    rect: Rect,
    solid: &[bool],
    wet: &[bool],
) -> Unit {
    let piece = generator.generate(rect);
    let is = |table: &[bool], id: u32| table.get(id as usize).copied().unwrap_or(false);
    // The highest Y up to which a column is full, opaque blocks from the bottom.
    let solid_top = |x: i32, z: i32| -> i32 {
        let col = piece.area.column(x, z);
        let n = col
            .iter()
            .position(|&id| !is(solid, id))
            .unwrap_or(col.len());
        MIN_Y + n as i32 - 1
    };
    let buffer = piece.area.rect;
    let mut out = Unit {
        rect,
        columns: Vec::with_capacity(rect.area()),
        cells: Vec::new(),
        grass: Vec::with_capacity(rect.area()),
        foliage: Vec::with_capacity(rect.area()),
        water: Vec::with_capacity(rect.area()),
    };
    let mut tops = HashMap::new();
    let mut top_of = |x: i32, z: i32| *tops.entry((x, z)).or_insert_with(|| solid_top(x, z));
    for (x, z) in rect.columns() {
        let col = piece.area.column(x, z);
        let Some(top) = col.iter().rposition(|&id| id != 0) else {
            out.columns.push((MIN_Y, 0));
            push_colors(&mut out, generator, colors, &piece.grid, x, z);
            continue;
        };
        // Cells hidden behind full blocks on every side are left out.
        let mut bottom = top_of(x, z);
        for (nx, nz) in [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)] {
            if buffer.contains(nx, nz) {
                bottom = bottom.min(top_of(nx, nz));
            }
        }
        let first = (bottom.max(MIN_Y) - MIN_Y) as usize;
        let first = first.min(top);
        for i in first..=top {
            let id = col[i];
            let above = col.get(i + 1).copied().unwrap_or(0);
            let flag = if is(wet, id) && !is(wet, above) {
                TOP_BIT
            } else {
                0
            };
            out.cells.push(id | flag);
        }
        out.columns
            .push((MIN_Y + first as i32, (top - first + 1) as u32));
        push_colors(&mut out, generator, colors, &piece.grid, x, z);
    }
    out
}

fn push_colors(
    out: &mut Unit,
    generator: &Generator,
    colors: &tint::ColorMaps,
    grid: &vistaforge_core::column::ColumnGrid,
    x: i32,
    z: i32,
) {
    let col = grid
        .try_get(x, z)
        .copied()
        .unwrap_or_else(|| generator.terrain.column(x, z));
    let land = colors.colors(generator.surface.minecraft_biome_at(&col, i32::MAX));
    let water = colors.colors(generator.surface.minecraft_biome_at(&col, MIN_Y));
    out.grass.push(land[0]);
    out.foliage.push(land[1]);
    out.water.push(water[2]);
}

fn pad4(out: &mut Vec<u8>, fill: u8) {
    while !out.len().is_multiple_of(4) {
        out.push(fill);
    }
}

/// A file path sent as the hex of its UTF-8 bytes.
pub fn hex_path(hex: &str) -> Option<PathBuf> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok().map(PathBuf::from)
}

fn pack(
    header: &Header,
    columns: &[u32],
    water: &[u8],
    voxels: &[u16],
    states: &[u32],
    quads: &[f32],
    atlas: &[u8],
) -> Vec<u8> {
    let mut head = serde_json::to_vec(header).expect("header serialises");
    pad4(&mut head, b' ');
    let mut out = Vec::with_capacity(
        8 + head.len()
            + columns.len() * 4
            + water.len()
            + voxels.len() * 2
            + 4
            + states.len() * 4
            + quads.len() * 4
            + atlas.len(),
    );
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(head.len() as u32).to_le_bytes());
    out.extend_from_slice(&head);
    for c in columns {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out.extend_from_slice(water);
    for v in voxels {
        out.extend_from_slice(&v.to_le_bytes());
    }
    pad4(&mut out, 0);
    for s in states {
        out.extend_from_slice(&s.to_le_bytes());
    }
    for q in quads {
        out.extend_from_slice(&q.to_le_bytes());
    }
    out.extend_from_slice(atlas);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use vistaforge_core::settings::WorldSettings;
    use vistaforge_core::terrain::Quality;

    pub struct Decoded {
        pub header: serde_json::Value,
        pub columns: Vec<u32>,
        pub voxels: Vec<u16>,
        pub states: Vec<u32>,
        pub quads: Vec<f32>,
        pub atlas_len: usize,
    }

    pub fn decode(bytes: &[u8]) -> Decoded {
        assert_eq!(&bytes[..4], MAGIC);
        let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let header: serde_json::Value = serde_json::from_slice(&bytes[8..8 + len]).unwrap();
        let n = |k: &str| header[k].as_u64().unwrap() as usize;
        let side = n("side");
        let mut at = 8 + len;
        let u32s = |b: &[u8]| {
            b.as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w))
                .collect::<Vec<_>>()
        };
        let columns = u32s(&bytes[at..at + 16 * side * side]);
        at += 20 * side * side;
        let voxels = bytes[at..at + 2 * n("voxels")]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u16::from_le_bytes(*w))
            .collect();
        at += (2 * n("voxels")).div_ceil(4) * 4;
        let states = u32s(&bytes[at..at + 48 * n("states")]);
        at += 48 * n("states");
        let quads = bytes[at..at + 80 * n("quads")]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| f32::from_le_bytes(*w))
            .collect();
        at += 80 * n("quads");
        let atlas_len = bytes.len() - at;
        assert_eq!(atlas_len, 4 * n("atlas_width") * n("atlas_height"));
        Decoded {
            header,
            columns,
            voxels,
            states,
            quads,
            atlas_len,
        }
    }

    fn catalog() -> Option<(Catalog, PathBuf)> {
        let path = PathBuf::from(
            std::env::var("VISTAFORGE_CATALOG")
                .unwrap_or_else(|_| "/mnt/project-files/vistaforge/catalog-cr-1.7.0.json".into()),
        );
        Some((Catalog::load(&path).ok()?, path))
    }

    /// Without the game's files, a scene is still built from catalog colours, and every
    /// column's cells start at a full block and end at its top.
    #[test]
    fn scene_from_the_catalog_alone() {
        let Some((catalog, path)) = catalog() else {
            eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
            return;
        };
        let settings = WorldSettings {
            size: 256,
            ..WorldSettings::default()
        };
        let generator = Generator::new(&settings, &catalog, Quality::Full).unwrap();
        let faces = library::catalog_faces(&path).unwrap();
        let mut library = Library::new(None, faces);
        let req = SceneRequest {
            x0: 64,
            z0: 64,
            size: 100,
            game_dir: None,
            catalog_path: Some(path),
        };
        let calls = AtomicUsize::new(0);
        let bytes = build(&generator, &catalog, &mut library, &req, &|_, _| {
            calls.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
        let d = decode(&bytes);
        assert_eq!(d.header["side"], 128);
        assert_eq!(d.header["width"], 128);
        assert_eq!(calls.into_inner(), 4);
        let states = d.header["states"].as_u64().unwrap() as usize;
        assert!(states > 10, "{states} states");
        assert!(d.atlas_len > 0);
        // Every column has cells, bottom first; the bottom cell is a full block or water.
        let mut water_tops = 0;
        for i in 0..128 * 128 {
            let (offset, packed) = (d.columns[4 * i] as usize, d.columns[4 * i + 1]);
            let count = (packed >> 16) as usize;
            assert!(count > 0);
            let first = d.voxels[offset] as usize;
            assert!(first > 0 && first < states);
            let top = d.voxels[offset + count - 1] as usize;
            assert_ne!(top, 0, "a column ends at its highest block");
            if d.states[12 * top] & WATER_TOP != 0 {
                water_tops += 1;
            }
            assert!(d.columns[4 * i + 2] > 0, "grass colour");
        }
        // Quads index the atlas and stay near their cell.
        let tiles = d.header["tiles"].as_u64().unwrap() as f32;
        for q in d.quads.as_chunks::<20>().0 {
            assert!(q[3] < tiles);
            assert!(q[0..3].iter().all(|c| (-0.5..=1.5).contains(c)));
        }
        eprintln!(
            "{} voxels, {states} states, {water_tops} water tops",
            d.voxels.len()
        );
    }

    #[test]
    fn paths_come_back_from_hex() {
        let path = "C:\\Users\\Фёдор\\карта 1.png";
        let hex: String = path.bytes().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex_path(&hex), Some(PathBuf::from(path)));
        assert_eq!(hex_path("4"), None);
        assert_eq!(hex_path("zz"), None);
        assert_eq!(hex_path(""), None);
    }
}
