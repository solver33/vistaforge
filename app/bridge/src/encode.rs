//! Packs an engine [`Preview`] into one binary buffer for the UI.
//!
//! A 512×512 preview is about 9 MB as JSON but 3 MB packed, and the UI reads it straight into
//! typed arrays. `app/src/lib/decode.ts` reads this layout; both sides test the same bytes.
//!
//! Layout, little-endian, every section starting on a 4-byte boundary:
//!
//! | bytes            | content                                                           |
//! |------------------|-------------------------------------------------------------------|
//! | 4                | magic `VFP1`                                                      |
//! | 4                | `u32` length of the header, a multiple of 4                       |
//! | header           | JSON `{size, res, step, trees, biomes: [id…], species: [id…]}`,   |
//! |                  | padded with spaces                                                |
//! | 4·n              | `f32` ground heights, row-major (z, then x); n = res²             |
//! | 4·n              | `f32` water surface Y, NaN where dry                              |
//! | 4·n              | RGBA colours, alpha 255                                           |
//! | n, padded to 4   | `u8` biome, an index into `header.biomes`                         |
//! | 24·trees         | `f32` x, y, z, height, crown, species index into `header.species` |

use serde::Serialize;
use vistaforge_core::engine::Preview;

pub const MAGIC: &[u8; 4] = b"VFP1";

#[derive(Serialize)]
struct Header<'a> {
    size: u32,
    res: u32,
    step: f32,
    trees: usize,
    biomes: Vec<&'a str>,
    species: Vec<String>,
}

/// Index of `id` in `table`, adding it if new.
fn intern<T: PartialEq>(table: &mut Vec<T>, id: T) -> u8 {
    let i = match table.iter().position(|t| *t == id) {
        Some(i) => i,
        None => {
            table.push(id);
            table.len() - 1
        }
    };
    u8::try_from(i).expect("more than 256 distinct ids in a preview")
}

fn pad4(out: &mut Vec<u8>, fill: u8) {
    while !out.len().is_multiple_of(4) {
        out.push(fill);
    }
}

pub fn preview(p: &Preview) -> Vec<u8> {
    let mut biomes = Vec::new();
    let biome_index: Vec<u8> = p
        .biomes
        .iter()
        .map(|b| intern(&mut biomes, b.id()))
        .collect();
    let mut species = Vec::new();
    let tree_species: Vec<u8> = p
        .trees
        .iter()
        .map(|t| {
            let id = serde_json::to_value(t.species)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            intern(&mut species, id)
        })
        .collect();

    let mut header = serde_json::to_vec(&Header {
        size: p.size,
        res: p.res,
        step: p.step,
        trees: p.trees.len(),
        biomes,
        species,
    })
    .expect("header serialises");
    pad4(&mut header, b' ');

    let n = p.heights.len();
    let mut out = Vec::with_capacity(8 + header.len() + n * 13 + p.trees.len() * 24 + 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    for h in &p.heights {
        out.extend_from_slice(&h.to_le_bytes());
    }
    for w in &p.water {
        out.extend_from_slice(&w.unwrap_or(f32::NAN).to_le_bytes());
    }
    for c in &p.colors {
        out.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    out.extend_from_slice(&biome_index);
    pad4(&mut out, 0);
    for (t, &s) in p.trees.iter().zip(&tree_species) {
        for v in [t.x, t.y, t.z, t.height, t.crown, s as f32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use vistaforge_core::biome::Biome;
    use vistaforge_core::engine::PreviewTree;
    use vistaforge_core::trees::Species;

    /// What `decode.ts` reads back; used to check the layout from the Rust side.
    pub struct Decoded {
        pub res: u32,
        pub heights: Vec<f32>,
        pub water: Vec<f32>,
        pub biomes: Vec<String>,
        pub trees: Vec<[f32; 6]>,
        pub header: serde_json::Value,
    }

    fn f32s(bytes: &[u8]) -> Vec<f32> {
        let (words, _) = bytes.as_chunks::<4>();
        words.iter().map(|w| f32::from_le_bytes(*w)).collect()
    }

    pub fn decode(bytes: &[u8]) -> Decoded {
        assert_eq!(&bytes[..4], MAGIC);
        let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let header: serde_json::Value = serde_json::from_slice(&bytes[8..8 + len]).unwrap();
        let res = header["res"].as_u64().unwrap() as u32;
        let n = (res * res) as usize;
        let trees = header["trees"].as_u64().unwrap() as usize;
        let mut at = 8 + len;
        let heights = f32s(&bytes[at..at + 4 * n]);
        at += 4 * n;
        let water = f32s(&bytes[at..at + 4 * n]);
        at += 8 * n; // and the colours
        let names = header["biomes"].as_array().unwrap();
        let biomes = bytes[at..at + n]
            .iter()
            .map(|&i| names[i as usize].as_str().unwrap().to_string())
            .collect();
        at += n.div_ceil(4) * 4;
        let trees = f32s(&bytes[at..at + 24 * trees])
            .as_chunks::<6>()
            .0
            .to_vec();
        assert_eq!(
            bytes.len(),
            at + 24 * header["trees"].as_u64().unwrap() as usize
        );
        Decoded {
            res,
            heights,
            water,
            biomes,
            trees,
            header,
        }
    }

    /// The 2×2 preview that `decode.test.ts` also checks, byte for byte.
    pub fn sample() -> Preview {
        Preview {
            size: 256,
            res: 2,
            step: 128.0,
            heights: vec![64.0, 65.5, 70.25, 80.0],
            water: vec![Some(63.0), None, None, Some(81.0)],
            colors: vec![[10, 20, 30], [40, 50, 60], [70, 80, 90], [100, 110, 120]],
            biomes: vec![Biome::Lake, Biome::Plains, Biome::Plains, Biome::Forest],
            trees: vec![PreviewTree {
                x: 100.5,
                y: 66.0,
                z: 30.5,
                species: Species::Birch,
                height: 14.0,
                crown: 4.5,
            }],
        }
    }

    #[test]
    fn layout_round_trips() {
        let bytes = preview(&sample());
        assert_eq!(bytes.len() % 4, 0);
        let d = decode(&bytes);
        assert_eq!(d.res, 2);
        assert_eq!(d.heights, vec![64.0, 65.5, 70.25, 80.0]);
        assert_eq!(d.water[0], 63.0);
        assert!(d.water[1].is_nan());
        assert_eq!(d.biomes, vec!["lake", "plains", "plains", "forest"]);
        assert_eq!(d.trees, vec![[100.5, 66.0, 30.5, 14.0, 4.5, 0.0]]);
        assert_eq!(d.header["species"][0], "birch");
        assert_eq!(d.header["size"], 256);
    }

    /// `decode.test.ts` decodes the same file, so both sides agree on the layout.
    #[test]
    fn sample_bytes_match_the_ui_test() {
        let hex: String = preview(&sample())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let expected = include_str!("../../src/lib/preview-sample.hex").trim();
        assert_eq!(hex, expected);
    }
}
