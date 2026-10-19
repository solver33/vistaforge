//! Opening Minecraft worlds the player already has, to look at and render them.
//!
//! A world folder (the one with `level.dat`) is only ever read. Minecraft worlds have no edge,
//! so nothing reads the whole world at once: [`map`] draws a coarse picture of every saved
//! chunk to choose an area on, and only the chosen square is read in full, for the 3D
//! preview ([`map::area_preview`]) and for render mode ([`scene`]).

pub mod blocks;
pub mod chunk;
pub mod map;
pub mod region;
pub mod scene;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use serde::Serialize;
use vistaforge_core::catalog::Catalog;
use vistaforge_core::nbt::{self, Compound, Tag};

use crate::render::library::CatalogBlock;
use blocks::{BlockTable, Names};
use chunk::{Chunk, Skipped};
use region::Regions;

/// Face colours from the catalog file, by block name.
pub type Faces = Arc<HashMap<String, CatalogBlock>>;

/// Dimensions as the game names them, and where each keeps its regions.
pub const DIMENSIONS: [(&str, &str); 3] = [
    ("overworld", "region"),
    ("the_nether", "DIM-1/region"),
    ("the_end", "DIM1/region"),
];

/// The Nether's bedrock roof: the map and renders look under it.
pub const NETHER_ROOF: i32 = 128;

/// What the UI shows about an opened world.
#[derive(Clone, Debug, Serialize)]
pub struct WorldInfo {
    pub path: PathBuf,
    pub name: String,
    /// The Minecraft version that last saved it, e.g. `1.20.1`.
    pub version: Option<String>,
    pub data_version: Option<i32>,
    pub dimension: String,
    /// Dimensions with saved chunks.
    pub dimensions: Vec<String>,
    pub spawn: [i32; 3],
    /// Where the player was when the world was last saved, when in this dimension.
    pub player: Option<[i32; 3]>,
    pub regions: usize,
    pub chunks: usize,
    /// Blocks covered by saved chunks: x0, z0, x1, z1 (the far edges exclusive).
    pub bounds: Option<[i32; 4]>,
    pub seconds: f64,
}

pub struct OpenWorld {
    pub info: WorldInfo,
    pub regions: Regions,
    pub blocks: BlockTable,
    pub biomes: Names,
    /// Blocks at and above this Y are left out (the Nether's roof).
    pub roof: Option<i32>,
}

/// How reading one chunk went.
pub enum ChunkRead {
    Missing,
    Skipped(Skipped),
    Failed,
    Chunk(Chunk),
}

impl OpenWorld {
    /// Opens `path` (a world folder, or its `level.dat`) for `dimension`, the overworld when
    /// None or when the world lacks it.
    pub fn open(
        path: &Path,
        dimension: Option<&str>,
        catalog: Arc<Catalog>,
        faces: Faces,
    ) -> Result<Self> {
        let started = Instant::now();
        let dir = if path.is_file() {
            path.parent().unwrap_or(path).to_path_buf()
        } else {
            path.to_path_buf()
        };
        if !dir.is_dir() {
            bail!("{} is not a folder", dir.display());
        }
        let level = read_level(&dir.join("level.dat")).ok();
        let dimensions: Vec<String> = DIMENSIONS
            .iter()
            .filter(|(_, sub)| has_regions(&dir.join(sub)))
            .map(|(id, _)| id.to_string())
            .collect();
        if dimensions.is_empty() {
            bail!(
                "no saved chunks in {}. Choose a world's own folder: the one inside the saves \
                 folder that holds level.dat and a region folder.",
                dir.display()
            );
        }
        let wanted = dimension.unwrap_or("overworld");
        let dimension = if dimensions.iter().any(|d| d == wanted) {
            wanted.to_string()
        } else {
            dimensions[0].clone()
        };
        let sub = DIMENSIONS
            .iter()
            .find(|(id, _)| *id == dimension)
            .map(|(_, sub)| *sub)
            .unwrap();
        let regions = Regions::open(&dir.join(sub))?;

        let mut bounds: Option<[i32; 4]> = None;
        let mut chunks = 0;
        for r in regions.regions.values() {
            for (cx, cz) in r.chunks() {
                chunks += 1;
                let b = bounds.get_or_insert([cx * 16, cz * 16, cx * 16 + 16, cz * 16 + 16]);
                b[0] = b[0].min(cx * 16);
                b[1] = b[1].min(cz * 16);
                b[2] = b[2].max(cx * 16 + 16);
                b[3] = b[3].max(cz * 16 + 16);
            }
        }
        let folder = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let level = level.unwrap_or_default();
        let player = level
            .player
            .filter(|(_, d)| {
                d.as_deref().unwrap_or("minecraft:overworld") == format!("minecraft:{dimension}")
            })
            .map(|(p, _)| p);
        let info = WorldInfo {
            path: dir.clone(),
            name: level
                .name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(folder),
            version: level.version,
            data_version: level.data_version,
            dimension: dimension.clone(),
            dimensions,
            spawn: level.spawn.unwrap_or([0, 64, 0]),
            player,
            regions: regions.regions.len(),
            chunks,
            bounds,
            seconds: started.elapsed().as_secs_f64(),
        };
        Ok(Self {
            info,
            regions,
            blocks: BlockTable::new(catalog, faces),
            biomes: Names::default(),
            roof: (dimension == "the_nether").then_some(NETHER_ROOF),
        })
    }

    /// Reads and decodes chunk (cx, cz).
    pub fn chunk(&self, cx: i32, cz: i32) -> ChunkRead {
        let bytes = match self.regions.read(cx, cz) {
            Ok(Some(b)) => b,
            Ok(None) => return ChunkRead::Missing,
            Err(e) => {
                eprintln!("chunk {cx},{cz}: {e:#}");
                return ChunkRead::Failed;
            }
        };
        match chunk::decode(&bytes, &self.blocks, &self.biomes) {
            Ok(Ok(c)) => ChunkRead::Chunk(c),
            Ok(Err(s)) => ChunkRead::Skipped(s),
            Err(e) => {
                eprintln!("chunk {cx},{cz}: {e:#}");
                ChunkRead::Failed
            }
        }
    }
}

fn has_regions(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                e.file_name()
                    .to_str()
                    .and_then(region::parse_name)
                    .is_some()
                    && e.metadata().map(|m| m.len() >= 8192).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[derive(Default)]
struct Level {
    name: Option<String>,
    version: Option<String>,
    data_version: Option<i32>,
    spawn: Option<[i32; 3]>,
    player: Option<([i32; 3], Option<String>)>,
}

fn read_level(path: &Path) -> Result<Level> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut raw = Vec::new();
    GzDecoder::new(file)
        .read_to_end(&mut raw)
        .with_context(|| format!("reading {}", path.display()))?;
    let (_, root) = nbt::read_root(&mut &raw[..])?;
    let Some(Tag::Compound(data)) = root.get("Data") else {
        bail!("{} has no Data", path.display());
    };
    let int = |c: &Compound, k: &str| match c.get(k) {
        Some(Tag::Int(v)) => Some(*v),
        _ => None,
    };
    let string = |c: &Compound, k: &str| match c.get(k) {
        Some(Tag::String(s)) => Some(s.clone()),
        _ => None,
    };
    let version = match data.get("Version") {
        Some(Tag::Compound(v)) => string(v, "Name"),
        _ => None,
    };
    let spawn = match (
        int(data, "SpawnX"),
        int(data, "SpawnY"),
        int(data, "SpawnZ"),
    ) {
        (Some(x), Some(y), Some(z)) => Some([x, y, z]),
        _ => None,
    };
    let player = match data.get("Player") {
        Some(Tag::Compound(p)) => match p.get("Pos") {
            Some(Tag::List(pos)) if pos.len() == 3 => {
                let v: Vec<i32> = pos
                    .iter()
                    .map(|t| match t {
                        Tag::Double(d) => d.floor() as i32,
                        _ => 0,
                    })
                    .collect();
                Some(([v[0], v[1], v[2]], string(p, "Dimension")))
            }
            _ => None,
        },
        _ => None,
    };
    Ok(Level {
        name: string(data, "LevelName"),
        version,
        data_version: int(data, "DataVersion"),
        spawn,
        player,
    })
}

#[cfg(test)]
pub(crate) mod tests;
