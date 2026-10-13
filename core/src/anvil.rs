//! Writes Minecraft 1.20.1 Anvil chunks and region files (`region/r.X.Z.mca`).
//!
//! Chunks are saved with status `minecraft:full` and `isLightOn = 0`, so the game computes
//! lighting itself on first load. Heightmaps are left out on purpose: the game recomputes
//! missing heightmaps when it reads a chunk.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::catalog::BlockState;
use crate::nbt::{self, Compound, Tag};
use crate::sink::{BlockId, Palette, AIR};
use crate::world::{Area, MIN_SECTION, MIN_Y};

/// Minecraft 1.20.1.
pub const DATA_VERSION: i32 = 3465;

pub fn ceil_log2(n: usize) -> u32 {
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
    }
}

/// Packs indices into longs the way 1.16+ does: entries never straddle two longs.
pub fn pack(indices: &[u32], bits: u32) -> Vec<i64> {
    let per_long = (64 / bits) as usize;
    let mut out = vec![0u64; indices.len().div_ceil(per_long)];
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    for (i, &v) in indices.iter().enumerate() {
        debug_assert!((v as u64) <= mask);
        out[i / per_long] |= (v as u64 & mask) << ((i % per_long) as u32 * bits);
    }
    out.into_iter().map(|x| x as i64).collect()
}

pub fn unpack(data: &[i64], bits: u32, count: usize) -> Vec<u32> {
    let per_long = (64 / bits) as usize;
    let mask = (1u64 << bits) - 1;
    (0..count)
        .map(|i| ((data[i / per_long] as u64 >> ((i % per_long) as u32 * bits)) & mask) as u32)
        .collect()
}

fn state_tag(state: &BlockState) -> Tag {
    let mut c = Compound::new().with("Name", state.name.as_str());
    if !state.props.is_empty() {
        let mut props = Compound::new();
        for (k, v) in &state.props {
            props.put(k, v.as_str());
        }
        c.put("Properties", props);
    }
    c.into()
}

/// Block palette and packed data for one section, from its 4096 ids in Minecraft order
/// (index = (y * 16 + z) * 16 + x). `scratch` maps global ids to local ones; it must hold
/// `u32::MAX` for every id and is restored before returning.
fn block_states(blocks: &[BlockId; 4096], palette: &Palette, scratch: &mut Vec<u32>) -> Compound {
    let mut local: Vec<BlockId> = Vec::new();
    let mut indices = [0u32; 4096];
    if blocks.iter().all(|&b| b == blocks[0]) {
        local.push(blocks[0]);
    } else {
        for (i, &id) in blocks.iter().enumerate() {
            let id_us = id as usize;
            if id_us >= scratch.len() {
                scratch.resize(id_us + 1, u32::MAX);
            }
            if scratch[id_us] == u32::MAX {
                scratch[id_us] = local.len() as u32;
                local.push(id);
            }
            indices[i] = scratch[id_us];
        }
        for &id in &local {
            scratch[id as usize] = u32::MAX;
        }
    }
    let mut c = Compound::new();
    c.put(
        "palette",
        Tag::List(
            local
                .iter()
                .map(|&id| palette.with(id, state_tag))
                .collect(),
        ),
    );
    if local.len() > 1 {
        let bits = ceil_log2(local.len()).max(4);
        c.put("data", Tag::LongArray(pack(&indices, bits)));
    }
    c
}

/// Biome palette and data for one section from its 64 biome cells, indexed (y * 4 + z) * 4 + x.
/// Linear biome palettes hold at most 8 entries (3 bits), so rarer extra biomes in one section
/// fall back to the first one.
fn biomes_tag(cells: &[&'static str; 64]) -> Compound {
    let mut local: Vec<&str> = Vec::new();
    let mut idx = [0u32; 64];
    for (i, b) in cells.iter().enumerate() {
        idx[i] = match local.iter().position(|l| l == b) {
            Some(p) => p as u32,
            None if local.len() < 8 => {
                local.push(b);
                (local.len() - 1) as u32
            }
            None => 0,
        };
    }
    let mut c = Compound::new().with("palette", Tag::strings(&local));
    if local.len() > 1 {
        c.put("data", Tag::LongArray(pack(&idx, ceil_log2(local.len()))));
    }
    c
}

/// The NBT of chunk (cx, cz), read from an area that contains the whole chunk.
/// `biome(x, y, z)` names the Minecraft biome of the 4×4×4 biome cell whose lowest block is at
/// `y`, sampled at the centre column of the cell.
pub fn chunk_tag(
    cx: i32,
    cz: i32,
    area: &Area,
    palette: &Palette,
    biome: &dyn Fn(i32, i32, i32) -> &'static str,
    scratch: &mut Vec<u32>,
) -> Compound {
    let (x0, z0) = (cx * 16, cz * 16);
    let mut last: Option<([&'static str; 64], Compound)> = None;
    let mut section_biomes = |s: usize| {
        let mut cells = [""; 64];
        for (i, cell) in cells.iter_mut().enumerate() {
            let (bx, by, bz) = (
                (i % 4) as i32 * 4,
                (i / 16) as i32 * 4,
                (i / 4 % 4) as i32 * 4,
            );
            *cell = biome(x0 + bx + 2, MIN_Y + s as i32 * 16 + by, z0 + bz + 2);
        }
        match &last {
            Some((prev, tag)) if *prev == cells => tag.clone(),
            _ => {
                let tag = biomes_tag(&cells);
                last = Some((cells, tag.clone()));
                tag
            }
        }
    };

    let columns: Vec<&[BlockId]> = (0..256)
        .map(|i: i32| area.column(x0 + i % 16, z0 + i / 16))
        .collect();
    // Every column of an area stores the same number of cells; above them is only air.
    let stored = columns[0].len();
    let count = ((area.max_y() - MIN_Y + 1) / 16) as usize;
    let mut sections = Vec::with_capacity(count);
    let mut blocks = [0 as BlockId; 4096];
    let mut air: Option<Compound> = None;
    for s in 0..count {
        let states = if s * 16 >= stored {
            air.get_or_insert_with(|| block_states(&[AIR; 4096], palette, scratch))
                .clone()
        } else {
            for y in 0..16 {
                let level = s * 16 + y;
                for (i, col) in columns.iter().enumerate() {
                    blocks[y * 256 + i] = col[level];
                }
            }
            block_states(&blocks, palette, scratch)
        };
        sections.push(
            Compound::new()
                .with("Y", (MIN_SECTION + s as i32) as i8)
                .with("block_states", states)
                .with("biomes", section_biomes(s))
                .into(),
        );
    }
    debug_assert_eq!(MIN_SECTION * 16, MIN_Y);

    Compound::new()
        .with("DataVersion", DATA_VERSION)
        .with("xPos", cx)
        .with("yPos", MIN_SECTION)
        .with("zPos", cz)
        .with("Status", "minecraft:full")
        .with("LastUpdate", 0i64)
        .with("InhabitedTime", 0i64)
        .with("isLightOn", 0i8)
        .with("sections", Tag::List(sections))
        .with("block_entities", Tag::List(vec![]))
        .with("block_ticks", Tag::List(vec![]))
        .with("fluid_ticks", Tag::List(vec![]))
        .with(
            "structures",
            Compound::new()
                .with("References", Compound::new())
                .with("starts", Compound::new()),
        )
}

/// A chunk's NBT, zlib-compressed as region files store it.
pub fn compress_chunk(tag: &Compound) -> Result<Vec<u8>> {
    let raw = nbt::to_bytes("", tag);
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(&raw)?;
    Ok(enc.finish()?)
}

/// Collects compressed chunks of one region and writes `r.X.Z.mca`.
pub struct RegionWriter {
    header: Vec<u8>,
    body: Vec<u8>,
    timestamp: u32,
}

impl Default for RegionWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl RegionWriter {
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        Self {
            header: vec![0u8; 8192],
            body: Vec::new(),
            timestamp,
        }
    }

    pub fn add(&mut self, cx: i32, cz: i32, compressed: &[u8]) -> Result<()> {
        let length = compressed.len() + 1; // + compression type byte
        let sectors = (4 + length).div_ceil(4096);
        if sectors > 255 {
            bail!("chunk {cx},{cz} is too large for a region file ({length} bytes)");
        }
        let offset = 2 + self.body.len() / 4096;
        self.body.extend_from_slice(&(length as u32).to_be_bytes());
        self.body.push(2); // zlib
        self.body.extend_from_slice(compressed);
        self.body.resize(self.body.len().div_ceil(4096) * 4096, 0);

        let slot = ((cx & 31) + (cz & 31) * 32) as usize;
        let location = ((offset as u32) << 8) | sectors as u32;
        self.header[slot * 4..slot * 4 + 4].copy_from_slice(&location.to_be_bytes());
        self.header[4096 + slot * 4..4096 + slot * 4 + 4]
            .copy_from_slice(&self.timestamp.to_be_bytes());
        Ok(())
    }

    pub fn write(self, region_dir: &Path, rx: i32, rz: i32) -> Result<()> {
        fs::create_dir_all(region_dir)?;
        let mut file = self.header;
        file.extend_from_slice(&self.body);
        fs::write(region_dir.join(format!("r.{rx}.{rz}.mca")), file)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::Rect;
    use crate::sink::BlockSink;

    #[test]
    fn log2() {
        assert_eq!(ceil_log2(1), 0);
        assert_eq!(ceil_log2(2), 1);
        assert_eq!(ceil_log2(3), 2);
        assert_eq!(ceil_log2(4), 2);
        assert_eq!(ceil_log2(5), 3);
        assert_eq!(ceil_log2(17), 5);
    }

    #[test]
    fn pack_layout() {
        // 5 bits: 12 entries per long, the top 4 bits of each long stay unused.
        let indices: Vec<u32> = (0..4096).map(|i| (i % 31) as u32).collect();
        let packed = pack(&indices, 5);
        assert_eq!(packed.len(), 4096usize.div_ceil(12));
        assert_eq!(unpack(&packed, 5, 4096), indices);
        assert_eq!(packed[0] as u64 & 0x1F, 0);
        assert_eq!((packed[0] as u64 >> 5) & 0x1F, 1);
        assert_eq!(packed[0] as u64 >> 60, 0);
    }

    fn sections(tag: &Compound) -> &Vec<Tag> {
        let Some(Tag::List(s)) = tag.get("sections") else {
            panic!()
        };
        s
    }

    #[test]
    fn chunk_sections_and_biomes() {
        let palette = Palette::new();
        let stone = palette.intern(&BlockState {
            name: "minecraft:stone".into(),
            props: vec![],
        });
        let mut area = Area::new(Rect::new(16, 0, 32, 16), &palette);
        // One block at local (15, y=-64+3, 15): last index of the bottom section's 4th layer.
        area.set(31, -61, 15, stone);
        // River in the two lowest biome cells of the east half, plains everywhere else.
        let biome = |x: i32, y: i32, _z: i32| {
            if x >= 24 && y <= -60 {
                "minecraft:river"
            } else {
                "minecraft:plains"
            }
        };
        let mut scratch = Vec::new();
        let tag = chunk_tag(1, 0, &area, &palette, &biome, &mut scratch);
        assert!(scratch.iter().all(|&s| s == u32::MAX));
        let secs = sections(&tag);
        assert_eq!(secs.len(), crate::world::SECTIONS);

        let Tag::Compound(bottom) = &secs[0] else {
            panic!()
        };
        let Some(Tag::Compound(bs)) = bottom.get("block_states") else {
            panic!()
        };
        let Some(Tag::List(p)) = bs.get("palette") else {
            panic!()
        };
        assert_eq!(p.len(), 2);
        let Some(Tag::LongArray(data)) = bs.get("data") else {
            panic!()
        };
        let idx = unpack(data, 4, 4096);
        assert_eq!(idx[(3 * 16 + 15) * 16 + 15], 1);
        assert_eq!(idx.iter().filter(|&&i| i == 1).count(), 1);

        let Some(Tag::Compound(biomes)) = bottom.get("biomes") else {
            panic!()
        };
        let Some(Tag::LongArray(bdata)) = biomes.get("data") else {
            panic!()
        };
        let b = unpack(bdata, 1, 64);
        // Cells are indexed (y * 4 + z) * 4 + x: river in x cells 2-3 of the two lowest layers.
        assert_eq!(&b[0..4], &[0, 0, 1, 1]);
        assert_eq!(&b[28..32], &[0, 0, 1, 1]);
        assert_eq!(&b[32..36], &[0, 0, 0, 0]);
        assert_eq!(&b[60..64], &[0, 0, 0, 0]);
        let Tag::Compound(second) = &secs[1] else {
            panic!()
        };
        let Some(Tag::Compound(biomes)) = second.get("biomes") else {
            panic!()
        };
        assert!(biomes.get("data").is_none());

        let Tag::Compound(top) = &secs[crate::world::SECTIONS - 1] else {
            panic!()
        };
        let Some(Tag::Compound(bs)) = top.get("block_states") else {
            panic!()
        };
        assert!(bs.get("data").is_none());
    }

    #[test]
    fn tall_chunks_hold_every_section_up_to_the_top() {
        let palette = Palette::new();
        let stone = palette.intern(&BlockState {
            name: "minecraft:stone".into(),
            props: vec![],
        });
        let mut area = Area::for_world(Rect::new(0, 0, 16, 16), &palette, 2031, 80);
        area.set(3, 1000, 4, stone);
        let mut scratch = Vec::new();
        let tag = chunk_tag(
            0,
            0,
            &area,
            &palette,
            &|_, _, _| "minecraft:plains",
            &mut scratch,
        );
        let secs = sections(&tag);
        assert_eq!(secs.len(), 131);
        let y_of = |s: &Tag| match s {
            Tag::Compound(c) => match c.get("Y") {
                Some(Tag::Byte(y)) => *y as i32,
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert_eq!(y_of(&secs[0]), -4);
        assert_eq!(y_of(&secs[130]), 126);
        // The stone at Y 1000 is in section 62 (Y 992..1007), local y 8.
        let Tag::Compound(s62) = &secs[(1000 + 64) / 16] else {
            panic!()
        };
        assert_eq!(y_of(&secs[(1000 + 64) / 16]), 62);
        let Some(Tag::Compound(bs)) = s62.get("block_states") else {
            panic!()
        };
        let Some(Tag::LongArray(data)) = bs.get("data") else {
            panic!()
        };
        let idx = unpack(data, 4, 4096);
        assert_eq!(idx[(8 * 16 + 4) * 16 + 3], 1);
        // Sections above everything stored are plain air.
        let Tag::Compound(last) = &secs[130] else {
            panic!()
        };
        let Some(Tag::Compound(bs)) = last.get("block_states") else {
            panic!()
        };
        assert!(bs.get("data").is_none());
    }
}
