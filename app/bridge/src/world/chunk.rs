//! Chunks as Minecraft 1.18 and later save them: a list of 16-block-high sections, each with
//! a palette of block states and indices packed into longs, and the same for biomes per
//! 4×4×4 cell.

use anyhow::{Context, Result};
use vistaforge_core::catalog::BlockState;
use vistaforge_core::nbt::{self, Compound, Tag};

use super::blocks::{BlockTable, Names};

/// Values from a palette, packed the way 1.16+ does: entries never straddle two longs.
#[derive(Clone, Debug)]
pub struct Paletted {
    pub palette: Vec<u32>,
    bits: u32,
    per_long: u32,
    data: Box<[u64]>,
}

impl Paletted {
    pub fn single(value: u32) -> Self {
        Self {
            palette: vec![value],
            bits: 0,
            per_long: 1,
            data: Box::new([]),
        }
    }

    /// `min_bits` is 4 for blocks and 1 for biomes, as the game writes them.
    fn new(palette: Vec<u32>, data: Option<&[i64]>, entries: usize, min_bits: u32) -> Self {
        let Some(data) = data.filter(|d| !d.is_empty() && palette.len() > 1) else {
            return Self::single(palette.first().copied().unwrap_or(0));
        };
        let needed = ceil_log2(palette.len()).max(min_bits);
        // A writer may have used wider entries than needed; the array's length tells.
        let bits = (needed..=32)
            .find(|&b| entries.div_ceil((64 / b) as usize) == data.len())
            .unwrap_or(needed);
        let per_long = 64 / bits;
        Self {
            palette,
            bits,
            per_long,
            data: data.iter().map(|&v| v as u64).collect(),
        }
    }

    #[inline]
    pub fn get(&self, i: usize) -> u32 {
        if self.bits == 0 {
            return self.palette[0];
        }
        let per = self.per_long as usize;
        let Some(&word) = self.data.get(i / per) else {
            return self.palette[0];
        };
        let index = (word >> ((i % per) as u32 * self.bits)) & ((1u64 << self.bits) - 1);
        self.palette
            .get(index as usize)
            .copied()
            .unwrap_or(self.palette[0])
    }

    /// One value everywhere.
    pub fn uniform(&self) -> Option<u32> {
        (self.bits == 0).then(|| self.palette[0])
    }
}

fn ceil_log2(n: usize) -> u32 {
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
    }
}

#[derive(Clone, Debug)]
pub struct Section {
    /// Block ids, index `y·256 + z·16 + x`.
    pub blocks: Paletted,
    /// Biome ids, index `(y/4)·16 + (z/4)·4 + x/4`.
    pub biomes: Paletted,
}

/// One chunk's blocks and biomes, bottom section first.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub cx: i32,
    pub cz: i32,
    /// Section Y (block Y / 16) of `sections[0]`.
    pub min_section: i32,
    pub sections: Vec<Section>,
}

/// How a chunk read went when it gave no blocks.
#[derive(Debug, PartialEq, Eq)]
pub enum Skipped {
    /// Saved by Minecraft older than 1.18; the game upgrades it when the area is visited.
    OldFormat,
    /// Generation stopped part-way (the edge of the explored world).
    Unfinished,
}

impl Chunk {
    /// Lowest and one-above-highest block Y the sections cover.
    pub fn y_range(&self) -> (i32, i32) {
        (
            self.min_section * 16,
            (self.min_section + self.sections.len() as i32) * 16,
        )
    }

    pub fn section(&self, y: i32) -> Option<&Section> {
        let s = (y >> 4) - self.min_section;
        if s < 0 {
            return None;
        }
        self.sections.get(s as usize)
    }

    /// Block id at chunk-local x, z (0..16) and world y; air outside the sections.
    #[inline]
    pub fn block(&self, x: usize, y: i32, z: usize) -> u32 {
        match self.section(y) {
            Some(s) => s.blocks.get(((y & 15) as usize) << 8 | z << 4 | x),
            None => 0,
        }
    }

    /// Biome id of the 4×4×4 cell holding chunk-local x, z and world y.
    pub fn biome(&self, x: usize, y: i32, z: usize) -> u32 {
        let (lo, hi) = self.y_range();
        let y = y.clamp(lo, hi - 1);
        match self.section(y) {
            Some(s) => s
                .biomes
                .get((((y & 15) >> 2) as usize) << 4 | (z >> 2) << 2 | x >> 2),
            None => 0,
        }
    }

    /// The highest Y in column (x, z) holding anything but air, below `below`.
    pub fn top(&self, x: usize, z: usize, below: i32, air: &[bool]) -> Option<i32> {
        let is_air = |id: u32| air.get(id as usize).copied().unwrap_or(false);
        let (lo, hi) = self.y_range();
        let start = hi.min(below) - 1;
        let mut y = start;
        while y >= lo {
            let s = &self.sections[((y >> 4) - self.min_section) as usize];
            if let Some(id) = s.blocks.uniform() {
                if is_air(id) {
                    y = (y & !15) - 1;
                    continue;
                }
                return Some(y);
            }
            let base = z << 4 | x;
            for yy in (0..=(y & 15)).rev() {
                if !is_air(s.blocks.get((yy as usize) << 8 | base)) {
                    return Some((y & !15) + yy);
                }
            }
            y = (y & !15) - 1;
        }
        None
    }
}

fn get<'a>(c: &'a Compound, key: &str) -> Option<&'a Tag> {
    c.get(key)
}

fn int(c: &Compound, key: &str) -> Option<i32> {
    match get(c, key)? {
        Tag::Byte(v) => Some(*v as i32),
        Tag::Short(v) => Some(*v as i32),
        Tag::Int(v) => Some(*v),
        Tag::Long(v) => Some(*v as i32),
        _ => None,
    }
}

fn string<'a>(c: &'a Compound, key: &str) -> Option<&'a str> {
    match get(c, key)? {
        Tag::String(s) => Some(s),
        _ => None,
    }
}

fn compound<'a>(c: &'a Compound, key: &str) -> Option<&'a Compound> {
    match get(c, key)? {
        Tag::Compound(c) => Some(c),
        _ => None,
    }
}

fn list<'a>(c: &'a Compound, key: &str) -> Option<&'a [Tag]> {
    match get(c, key)? {
        Tag::List(l) => Some(l),
        _ => None,
    }
}

fn longs<'a>(c: &'a Compound, key: &str) -> Option<&'a [i64]> {
    match get(c, key)? {
        Tag::LongArray(l) => Some(l),
        _ => None,
    }
}

/// A palette entry `{Name, Properties}` as a block state with its properties sorted.
pub fn state_of(entry: &Compound) -> BlockState {
    let name = string(entry, "Name").unwrap_or("minecraft:air").to_string();
    let mut props: Vec<(String, String)> = compound(entry, "Properties")
        .map(|p| {
            p.0.iter()
                .filter_map(|(k, v)| match v {
                    Tag::String(s) => Some((k.clone(), s.clone())),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    props.sort();
    BlockState { name, props }
}

/// Reads a chunk's NBT. Blocks and biomes are numbered in `blocks` and `biomes`.
pub fn decode(
    bytes: &[u8],
    blocks: &BlockTable,
    biomes: &Names,
) -> Result<std::result::Result<Chunk, Skipped>> {
    let (_, root) = nbt::read_root(&mut &bytes[..]).context("reading chunk NBT")?;
    if compound(&root, "Level").is_some() && list(&root, "sections").is_none() {
        return Ok(Err(Skipped::OldFormat));
    }
    if let Some(status) = string(&root, "Status") {
        let status = status.strip_prefix("minecraft:").unwrap_or(status);
        if status != "full" {
            return Ok(Err(Skipped::Unfinished));
        }
    }
    let cx = int(&root, "xPos").unwrap_or(0);
    let cz = int(&root, "zPos").unwrap_or(0);
    let mut found: Vec<(i32, Section)> = Vec::new();
    for tag in list(&root, "sections").unwrap_or(&[]) {
        let Tag::Compound(s) = tag else { continue };
        let Some(y) = int(s, "Y") else { continue };
        let block_section = compound(s, "block_states").map(|bs| {
            let palette: Vec<u32> = list(bs, "palette")
                .unwrap_or(&[])
                .iter()
                .map(|e| match e {
                    Tag::Compound(e) => blocks.intern(&state_of(e)),
                    _ => 0,
                })
                .collect();
            Paletted::new(palette, longs(bs, "data"), 4096, 4)
        });
        let biome_section = compound(s, "biomes").map(|bs| {
            let palette: Vec<u32> = list(bs, "palette")
                .unwrap_or(&[])
                .iter()
                .map(|e| match e {
                    Tag::String(name) => biomes.intern(name),
                    _ => biomes.intern("minecraft:plains"),
                })
                .collect();
            Paletted::new(palette, longs(bs, "data"), 64, 1)
        });
        found.push((
            y,
            Section {
                blocks: block_section.unwrap_or_else(|| Paletted::single(0)),
                biomes: biome_section
                    .unwrap_or_else(|| Paletted::single(biomes.intern("minecraft:plains"))),
            },
        ));
    }
    found.sort_by_key(|(y, _)| *y);
    let min_section = int(&root, "yPos")
        .or(found.first().map(|(y, _)| *y))
        .unwrap_or(-4);
    let max_section = found.last().map(|(y, _)| *y).unwrap_or(min_section - 1);
    let fill_biome = found
        .first()
        .map(|(_, s)| s.biomes.palette[0])
        .unwrap_or_else(|| biomes.intern("minecraft:plains"));
    let mut sections: Vec<Section> = (min_section..=max_section)
        .map(|_| Section {
            blocks: Paletted::single(0),
            biomes: Paletted::single(fill_biome),
        })
        .collect();
    for (y, s) in found {
        if y >= min_section {
            sections[(y - min_section) as usize] = s;
        }
    }
    Ok(Ok(Chunk {
        cx,
        cz,
        min_section,
        sections,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_like_the_game() {
        // 5 bits: 12 entries per long.
        let values: Vec<u32> = (0..4096).map(|i| (i * 7 % 17) as u32).collect();
        let packed = vistaforge_core::anvil::pack(&values, 5);
        let p = Paletted::new((0..17).collect(), Some(&packed), 4096, 4);
        for (i, &v) in values.iter().enumerate() {
            assert_eq!(p.get(i), v);
        }
        let single = Paletted::new(vec![9], None, 4096, 4);
        assert_eq!(single.get(100), 9);
        assert_eq!(single.uniform(), Some(9));
        // A palette of 3 uses at least 4 bits for blocks, but 2 for biomes.
        let biomes: Vec<u32> = (0..64).map(|i| i % 3).collect();
        let packed = vistaforge_core::anvil::pack(&biomes, 2);
        let p = Paletted::new(vec![5, 6, 7], Some(&packed), 64, 1);
        assert_eq!(p.get(4), 6);
        assert_eq!(p.get(5), 7);
    }
}
