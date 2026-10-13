//! The engine: runs terrain → surface → trees → decor for pieces of the map and streams them
//! to Anvil region files on all CPU cores, so a 4096×4096 map never has to fit in memory.
//!
//! The map is cut into 64×64-block units. Each unit is generated with a [`MARGIN`] of extra
//! columns on every side, so trees and rocks that stand next to the unit but reach into it come
//! out whole; only the unit's own chunks are saved.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::anvil::{chunk_tag, compress_chunk, RegionWriter};
use crate::biome::Biome;
use crate::catalog::Catalog;
use crate::column::{ColumnGrid, Rect};
use crate::decor::{self, Decor};
use crate::level::{write_level_dat, LevelSettings};
use crate::settings::WorldSettings;
use crate::sink::{Blocks, Palette};
use crate::surface::Surface;
use crate::terrain::{Quality, Terrain};
use crate::trees::{self, Species, Trees};
use crate::world::Area;

/// Side of a generation unit in blocks (4×4 chunks).
pub const UNIT: i32 = 64;
/// Extra columns generated around a unit; covers the reach of trees and decor.
pub const MARGIN: i32 = 16;
const _: () = assert!(MARGIN >= trees::MAX_REACH && MARGIN >= decor::MAX_REACH);

pub struct Generator<'c> {
    pub settings: WorldSettings,
    pub catalog: &'c Catalog,
    pub palette: Palette,
    pub terrain: Terrain,
    pub surface: Surface,
    pub decor: Decor,
    pub trees: Trees,
}

/// A generated piece: the blocks of `rect` plus its margin, and the column facts behind them.
pub struct Piece<'p> {
    pub rect: Rect,
    pub area: Area<'p>,
    pub grid: ColumnGrid,
    pub trees: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Progress {
    pub chunks_done: usize,
    pub chunks_total: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExportReport {
    pub world_dir: PathBuf,
    pub chunks: usize,
    pub regions: usize,
    pub block_states: usize,
    pub trees: usize,
    pub seconds: f64,
    pub spawn: (i32, i32, i32),
}

/// A low-resolution picture of the map for the app's 2D and 3D views.
#[derive(Clone, Debug, Serialize)]
pub struct Preview {
    /// Map side in blocks.
    pub size: u32,
    /// Samples per side; sample (i, j) is the column at ((i + 0.5) · step, (j + 0.5) · step).
    pub res: u32,
    pub step: f32,
    /// Row-major (z, then x) ground heights in blocks.
    pub heights: Vec<f32>,
    /// Water surface Y per sample (the top of the water block), or `None` when dry.
    pub water: Vec<Option<f32>>,
    pub colors: Vec<[u8; 3]>,
    pub biomes: Vec<Biome>,
    pub trees: Vec<PreviewTree>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PreviewTree {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub species: Species,
    pub height: f32,
    pub crown: f32,
}

impl<'c> Generator<'c> {
    pub fn new(settings: &WorldSettings, catalog: &'c Catalog, quality: Quality) -> Result<Self> {
        settings.validate().map_err(anyhow::Error::msg)?;
        let palette = Palette::new();
        let blocks = Blocks::new(catalog, &palette);
        let surface = Surface::new(&blocks, settings).context("surface blocks")?;
        let decor = Decor::new(&blocks, settings).context("decor blocks")?;
        let trees = Trees::new(&blocks).context("tree blocks")?;
        let terrain = Terrain::new(settings, quality)?;
        Ok(Self {
            settings: settings.clone(),
            catalog,
            palette,
            terrain,
            surface,
            decor,
            trees,
        })
    }

    pub fn size(&self) -> i32 {
        self.settings.size as i32
    }

    /// Generates `rect` (with its margin) into memory.
    pub fn generate(&self, rect: Rect) -> Piece<'_> {
        let buffer = rect.expand(MARGIN);
        let grid = self.terrain.grid(buffer);
        let mut area = Area::new(buffer, &self.palette);
        for (x, z) in buffer.columns() {
            self.surface.fill(x, z, &grid, &mut area);
        }
        let column = |x: i32, z: i32| match grid.try_get(x, z) {
            Some(c) => *c,
            None => self.terrain.column(x, z),
        };
        // Only trees standing on the map: one just outside would show as half a crown.
        let map = Rect::square(self.size());
        let planned: Vec<_> = self
            .decor
            .plan_trees(buffer.intersect(&map), &column);
        for tree in &planned {
            self.trees.build(tree, &mut area);
        }
        self.decor.decorate(buffer, &grid, &mut area);
        let trees = planned.iter().filter(|t| rect.contains(t.x, t.z)).count();
        Piece {
            rect,
            area,
            grid,
            trees,
        }
    }

    /// A safe place to stand near the middle of the map: on the highest block of the column.
    pub fn spawn(&self) -> (i32, i32, i32) {
        let c = self.size() / 2;
        let piece = self.generate(Rect::new(c, c, c + 1, c + 1));
        let top = piece.area.top(c, c).unwrap_or(self.settings.sea_level);
        (c, top + 1, c)
    }

    pub fn preview(&self, res: u32) -> Preview {
        let size = self.settings.size;
        let res = res.clamp(16, size);
        let step = size as f32 / res as f32;
        let n = (res * res) as usize;
        let (mut heights, mut water, mut colors, mut biomes) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        for j in 0..res {
            for i in 0..res {
                let x = ((i as f32 + 0.5) * step) as i32;
                let z = ((j as f32 + 0.5) * step) as i32;
                let col = self.terrain.column(x, z);
                heights.push(col.height);
                water.push(col.water_level.map(|w| w as f32 + 1.0));
                colors.push(self.surface.preview_color(x, z, &col));
                biomes.push(col.biome);
            }
        }
        let column = |x: i32, z: i32| self.terrain.column(x, z);
        let trees = self
            .decor
            .plan_trees(Rect::square(size as i32), &column)
            .iter()
            .map(|t| {
                let (height, crown) = self.trees.shape(t);
                PreviewTree {
                    x: t.x as f32 + 0.5,
                    y: t.y as f32,
                    z: t.z as f32 + 0.5,
                    species: t.species,
                    height,
                    crown,
                }
            })
            .collect();
        Preview {
            size,
            res,
            step,
            heights,
            water,
            colors,
            biomes,
            trees,
        }
    }

    /// Writes the whole map as a Minecraft world folder `<saves_dir>/<settings.name>`.
    pub fn export(
        &self,
        saves_dir: &Path,
        overwrite: bool,
        progress: &(dyn Fn(Progress) + Sync),
        cancel: &AtomicBool,
    ) -> Result<ExportReport> {
        let started = Instant::now();
        let world_dir = saves_dir.join(&self.settings.name);
        if world_dir.exists() {
            if !overwrite {
                bail!("{} already exists", world_dir.display());
            }
            std::fs::remove_dir_all(&world_dir)
                .with_context(|| format!("removing {}", world_dir.display()))?;
        }
        let region_dir = world_dir.join("region");
        std::fs::create_dir_all(&region_dir)?;

        let size = self.size();
        let units_per_side = (size + UNIT - 1) / UNIT;
        let regions_per_side = (size + 511) / 512;
        let chunks_total = ((size / 16) * (size / 16)) as usize;

        // Units in region order, so regions finish (and leave memory) one after another.
        let mut units = Vec::new();
        let mut pending = Vec::new();
        for rz in 0..regions_per_side {
            for rx in 0..regions_per_side {
                let mut count = 0;
                for uz in rz * 8..((rz + 1) * 8).min(units_per_side) {
                    for ux in rx * 8..((rx + 1) * 8).min(units_per_side) {
                        units.push((rx, rz, ux, uz));
                        count += 1;
                    }
                }
                pending.push(Mutex::new((Some(RegionWriter::new()), count)));
            }
        }

        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let trees = AtomicUsize::new(0);
        let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    let mut scratch = Vec::new();
                    loop {
                        if cancel.load(Ordering::Relaxed) || failure.lock().unwrap().is_some() {
                            return;
                        }
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&(rx, rz, ux, uz)) = units.get(i) else {
                            return;
                        };
                        let rect = Rect::new(
                            ux * UNIT,
                            uz * UNIT,
                            ((ux + 1) * UNIT).min(size),
                            ((uz + 1) * UNIT).min(size),
                        );
                        let result =
                            self.save_unit(rect, rx, rz, &pending, &region_dir, &mut scratch);
                        match result {
                            Ok((chunks, unit_trees)) => {
                                trees.fetch_add(unit_trees, Ordering::Relaxed);
                                let chunks_done =
                                    done.fetch_add(chunks, Ordering::Relaxed) + chunks;
                                progress(Progress {
                                    chunks_done,
                                    chunks_total,
                                });
                            }
                            Err(e) => {
                                failure.lock().unwrap().get_or_insert(e);
                                return;
                            }
                        }
                    }
                });
            }
        });
        if let Some(e) = failure.into_inner().unwrap() {
            return Err(e);
        }
        if cancel.load(Ordering::Relaxed) {
            bail!("export cancelled");
        }

        validate_palette(&self.palette, self.catalog)?;
        let spawn = self.spawn();
        let half = size as f64 / 2.0;
        write_level_dat(
            &LevelSettings {
                name: self.settings.name.clone(),
                seed: self.settings.seed as i64,
                spawn,
                border_center: (half, half),
                border_size: size as f64,
                creative: true,
            },
            &world_dir,
        )?;
        Ok(ExportReport {
            world_dir,
            chunks: chunks_total,
            regions: (regions_per_side * regions_per_side) as usize,
            block_states: self.palette.len(),
            trees: trees.into_inner(),
            seconds: started.elapsed().as_secs_f64(),
            spawn,
        })
    }

    /// Generates one unit, compresses its chunks into its region, and writes the region file
    /// when this was the region's last unit. Returns the chunk and tree counts.
    fn save_unit(
        &self,
        rect: Rect,
        rx: i32,
        rz: i32,
        pending: &[Mutex<(Option<RegionWriter>, usize)>],
        region_dir: &Path,
        scratch: &mut Vec<u32>,
    ) -> Result<(usize, usize)> {
        let piece = self.generate(rect);
        let biome = |x: i32, z: i32| {
            let col = piece
                .grid
                .try_get(x, z)
                .copied()
                .unwrap_or_else(|| self.terrain.column(x, z));
            self.surface.minecraft_biome(&col)
        };
        let mut chunks = Vec::new();
        for cz in rect.z0 / 16..rect.z1 / 16 {
            for cx in rect.x0 / 16..rect.x1 / 16 {
                let tag = chunk_tag(cx, cz, &piece.area, &self.palette, &biome, scratch);
                chunks.push((cx, cz, compress_chunk(&tag)?));
            }
        }
        let regions_per_side = (self.size() + 511) / 512;
        let slot = &pending[(rz * regions_per_side + rx) as usize];
        let finished = {
            let mut guard = slot.lock().unwrap();
            let writer = guard.0.as_mut().expect("region already written");
            for (cx, cz, data) in &chunks {
                writer.add(*cx, *cz, data)?;
            }
            guard.1 -= 1;
            if guard.1 == 0 {
                guard.0.take()
            } else {
                None
            }
        };
        if let Some(writer) = finished {
            writer.write(region_dir, rx, rz)?;
        }
        Ok((chunks.len(), piece.trees))
    }
}

/// Checks that every block state the run used is complete and valid for the catalog.
pub fn validate_palette(palette: &Palette, catalog: &Catalog) -> Result<()> {
    for state in palette.states() {
        let props: Vec<(&str, &str)> = state
            .props
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let complete = catalog.state(&state.name, &props)?;
        if complete != state {
            bail!("incomplete block state {state}, expected {complete}");
        }
    }
    Ok(())
}

/// True when two areas hold the same blocks in every column of `rect`.
#[cfg(test)]
pub(crate) fn same_blocks(a: &Area, b: &Area, rect: Rect) -> bool {
    rect.columns()
        .all(|(x, z)| a.column(x, z) == b.column(x, z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::{BlockSink, AIR};

    fn catalog() -> Option<Catalog> {
        let path = std::env::var("VISTAFORGE_CATALOG")
            .unwrap_or_else(|_| "/mnt/project-files/vistaforge/catalog-cr-1.7.0.json".into());
        Catalog::load(Path::new(&path)).ok()
    }

    /// Two overlapping pieces must agree on every block they share, trees included.
    #[test]
    fn pieces_agree_across_borders() {
        let Some(catalog) = catalog() else {
            eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
            return;
        };
        let settings = WorldSettings {
            size: 256,
            ..WorldSettings::default()
        };
        let g = Generator::new(&settings, &catalog, Quality::Full).unwrap();
        // A piece is complete for its own rect; two pieces must agree where those overlap.
        let a = g.generate(Rect::new(0, 0, 64, 64));
        let b = g.generate(Rect::new(32, 16, 96, 80));
        let shared = Rect::new(32, 16, 64, 64);
        assert!(same_blocks(&a.area, &b.area, shared));
        assert!(shared.columns().any(|(x, z)| a.area.top(x, z).is_some()));
        assert_ne!(a.area.get(10, 40, 10), AIR);
    }
}
