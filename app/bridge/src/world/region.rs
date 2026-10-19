//! Region files (`region/r.X.Z.mca`): where each chunk's bytes are, and reading them back.
//!
//! A region holds 32×32 chunks. Its first 4 KiB say, per chunk, at which 4 KiB sector the
//! chunk starts and how many sectors it takes (0 when the chunk was never saved). A chunk is a
//! 4-byte length, a compression byte and the compressed NBT; chunks too big for the region
//! live beside it in `c.X.Z.mcc` with the high bit of the compression byte set.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use flate2::read::{GzDecoder, ZlibDecoder};

pub struct Region {
    pub rx: i32,
    pub rz: i32,
    dir: PathBuf,
    /// Per chunk slot: sector offset << 8 | sector count, 0 where there is no chunk.
    locations: Box<[u32; 1024]>,
    file: Mutex<File>,
}

impl Region {
    pub fn open(path: &Path, rx: i32, rz: i32) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut header = vec![0u8; 4096];
        file.read_exact(&mut header)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut locations = Box::new([0u32; 1024]);
        for (i, l) in locations.iter_mut().enumerate() {
            *l = u32::from_be_bytes(header[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Ok(Self {
            rx,
            rz,
            dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            locations,
            file: Mutex::new(file),
        })
    }

    fn slot(cx: i32, cz: i32) -> usize {
        ((cx & 31) + (cz & 31) * 32) as usize
    }

    /// Whether chunk (cx, cz), in world chunk coordinates, was saved in this region.
    pub fn has(&self, cx: i32, cz: i32) -> bool {
        self.locations[Self::slot(cx, cz)] != 0
    }

    /// Chunks saved here, in world chunk coordinates.
    pub fn chunks(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (0..1024).filter(|&i| self.locations[i] != 0).map(|i| {
            (
                self.rx * 32 + (i % 32) as i32,
                self.rz * 32 + (i / 32) as i32,
            )
        })
    }

    /// The chunk's uncompressed NBT, or None when it was never saved.
    pub fn read(&self, cx: i32, cz: i32) -> Result<Option<Vec<u8>>> {
        let location = self.locations[Self::slot(cx, cz)];
        if location == 0 {
            return Ok(None);
        }
        let (offset, sectors) = ((location >> 8) as u64, (location & 0xFF) as usize);
        if offset < 2 || sectors == 0 {
            return Ok(None);
        }
        let mut raw = vec![0u8; sectors * 4096];
        {
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(offset * 4096))?;
            // The last chunk may end before its last sector does.
            let mut got = 0;
            while got < raw.len() {
                match file.read(&mut raw[got..])? {
                    0 => break,
                    n => got += n,
                }
            }
            raw.truncate(got);
        }
        if raw.len() < 5 {
            return Ok(None);
        }
        let length = u32::from_be_bytes(raw[0..4].try_into().unwrap()) as usize;
        let kind = raw[4];
        if kind & 0x80 != 0 {
            let path = self.dir.join(format!("c.{cx}.{cz}.mcc"));
            let data =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            return decompress(kind & 0x7F, &data).map(Some);
        }
        if length < 1 || 4 + length > raw.len() {
            bail!("chunk {cx},{cz} is cut short");
        }
        decompress(kind, &raw[5..4 + length]).map(Some)
    }
}

fn decompress(kind: u8, data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 4);
    match kind {
        1 => {
            GzDecoder::new(data).read_to_end(&mut out)?;
        }
        2 => {
            ZlibDecoder::new(data).read_to_end(&mut out)?;
        }
        3 => out.extend_from_slice(data),
        // LZ4 and custom compressors came with Minecraft 1.20.5, after the version we read.
        other => bail!("chunk compression {other} is not supported"),
    }
    Ok(out)
}

/// Every region file of one dimension.
pub struct Regions {
    pub dir: PathBuf,
    pub regions: HashMap<(i32, i32), Region>,
}

impl Regions {
    /// Lists `dir/r.X.Z.mca`. Empty files (regions the game created but never filled) are
    /// left out.
    pub fn open(dir: &Path) -> Result<Self> {
        let mut regions = HashMap::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    dir: dir.to_path_buf(),
                    regions,
                })
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some((rx, rz)) = name.to_str().and_then(parse_name) else {
                continue;
            };
            if entry.metadata().map(|m| m.len() < 8192).unwrap_or(true) {
                continue;
            }
            match Region::open(&entry.path(), rx, rz) {
                Ok(r) => {
                    regions.insert((rx, rz), r);
                }
                Err(e) => eprintln!("skipping region {}: {e:#}", entry.path().display()),
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            regions,
        })
    }

    pub fn region_of(&self, cx: i32, cz: i32) -> Option<&Region> {
        self.regions.get(&(cx >> 5, cz >> 5))
    }

    pub fn has(&self, cx: i32, cz: i32) -> bool {
        self.region_of(cx, cz).is_some_and(|r| r.has(cx, cz))
    }

    pub fn read(&self, cx: i32, cz: i32) -> Result<Option<Vec<u8>>> {
        match self.region_of(cx, cz) {
            Some(r) => r.read(cx, cz),
            None => Ok(None),
        }
    }

    pub fn chunk_count(&self) -> usize {
        self.regions.values().map(|r| r.chunks().count()).sum()
    }
}

/// `r.-2.5.mca` → (-2, 5).
pub fn parse_name(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?.strip_suffix(".mca")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_names() {
        assert_eq!(parse_name("r.0.0.mca"), Some((0, 0)));
        assert_eq!(parse_name("r.-2.15.mca"), Some((-2, 15)));
        assert_eq!(parse_name("r.0.0.mcr"), None);
        assert_eq!(parse_name("r.a.0.mca"), None);
    }
}
