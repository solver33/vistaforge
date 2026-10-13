//! Writes a [`World`] as Minecraft 1.20.1 Anvil region files (`region/r.X.Z.mca`).
//!
//! Chunks are saved with status `minecraft:full` and `isLightOn = 0`, so the game computes
//! lighting itself on first load. Heightmaps are left out on purpose: the game recomputes
//! missing heightmaps when it reads a chunk.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::catalog::BlockState;
use crate::nbt::{self, Compound, Tag};
use crate::world::{Chunk, Palette, World, MIN_SECTION, SECTIONS};

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

fn section_tag(y: i32, blocks: Option<&[u32; 4096]>, palette: &Palette, biome: &str) -> Compound {
    let mut local: Vec<u32> = Vec::new();
    let mut indices = vec![0u32; 4096];
    match blocks {
        None => local.push(0),
        Some(blocks) => {
            let mut lookup: BTreeMap<u32, u32> = BTreeMap::new();
            for (i, &id) in blocks.iter().enumerate() {
                let next = local.len() as u32;
                let idx = *lookup.entry(id).or_insert_with(|| {
                    local.push(id);
                    next
                });
                indices[i] = idx;
            }
        }
    }

    let mut block_states = Compound::new();
    block_states.put(
        "palette",
        Tag::List(
            local
                .iter()
                .map(|id| state_tag(palette.state(*id)))
                .collect(),
        ),
    );
    if local.len() > 1 {
        let bits = ceil_log2(local.len()).max(4);
        block_states.put("data", Tag::LongArray(pack(&indices, bits)));
    }

    Compound::new()
        .with("Y", y as i8)
        .with("block_states", block_states)
        .with(
            "biomes",
            Compound::new().with("palette", Tag::strings(&[biome])),
        )
}

pub fn chunk_tag(cx: i32, cz: i32, chunk: &Chunk, palette: &Palette, biome: &str) -> Compound {
    let sections: Vec<Tag> = (0..SECTIONS)
        .map(|i| {
            section_tag(
                MIN_SECTION + i as i32,
                chunk.sections[i].as_deref(),
                palette,
                biome,
            )
            .into()
        })
        .collect();
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

/// Writes every chunk of `world` into `<world_dir>/region`. Returns the number of region files.
pub fn write_regions(world: &World, world_dir: &Path) -> Result<usize> {
    let region_dir = world_dir.join("region");
    fs::create_dir_all(&region_dir)?;
    let mut regions: BTreeMap<(i32, i32), Vec<(i32, i32)>> = BTreeMap::new();
    for &(cx, cz) in world.chunks.keys() {
        regions
            .entry((cx >> 5, cz >> 5))
            .or_default()
            .push((cx, cz));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    for (&(rx, rz), chunks) in &regions {
        let mut header = vec![0u8; 8192];
        let mut body: Vec<u8> = Vec::new();
        for &(cx, cz) in chunks {
            let tag = chunk_tag(
                cx,
                cz,
                &world.chunks[&(cx, cz)],
                &world.palette,
                &world.biome,
            );
            let raw = nbt::to_bytes("", &tag);
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&raw)?;
            let compressed = enc.finish()?;

            let length = compressed.len() + 1; // + compression type byte
            let sectors = (4 + length).div_ceil(4096);
            if sectors > 255 {
                bail!("chunk {cx},{cz} is too large for a region file ({length} bytes)");
            }
            let offset = 2 + body.len() / 4096;
            body.extend_from_slice(&(length as u32).to_be_bytes());
            body.push(2); // zlib
            body.extend_from_slice(&compressed);
            body.resize(body.len().div_ceil(4096) * 4096, 0);

            let slot = ((cx & 31) + (cz & 31) * 32) as usize;
            let location = ((offset as u32) << 8) | sectors as u32;
            header[slot * 4..slot * 4 + 4].copy_from_slice(&location.to_be_bytes());
            header[4096 + slot * 4..4096 + slot * 4 + 4].copy_from_slice(&now.to_be_bytes());
        }
        let mut file = header;
        file.extend_from_slice(&body);
        fs::write(region_dir.join(format!("r.{rx}.{rz}.mca")), file)?;
    }
    Ok(regions.len())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn section_palette() {
        let mut palette = Palette::default();
        let stone = BlockState {
            name: "minecraft:stone".into(),
            props: vec![],
        };
        let stone_id = palette.id(&stone);
        let mut blocks = Box::new([0u32; 4096]);
        blocks[4095] = stone_id;
        let tag = section_tag(3, Some(&blocks), &palette, "minecraft:plains");
        let Some(Tag::Compound(bs)) = tag.get("block_states") else {
            panic!()
        };
        let Some(Tag::List(p)) = bs.get("palette") else {
            panic!()
        };
        assert_eq!(p.len(), 2);
        let Some(Tag::LongArray(data)) = bs.get("data") else {
            panic!()
        };
        assert_eq!(data.len(), 256); // 4 bits minimum, 16 per long
        assert_eq!(unpack(data, 4, 4096)[4095], 1);

        let air = section_tag(-4, None, &palette, "minecraft:plains");
        let Some(Tag::Compound(bs)) = air.get("block_states") else {
            panic!()
        };
        assert!(bs.get("data").is_none());
    }
}
