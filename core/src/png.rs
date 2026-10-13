//! A minimal PNG writer (8-bit RGB), for preview images from the command line and tests.

use std::io::Write;

use anyhow::Result;
use flate2::write::ZlibEncoder;
use flate2::Compression;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes `pixels` (row-major RGB, `width * height` entries) as a PNG file.
pub fn encode_rgb(width: u32, height: u32, pixels: &[[u8; 3]]) -> Result<Vec<u8>> {
    assert_eq!(pixels.len(), (width * height) as usize);
    let mut raw = Vec::with_capacity(pixels.len() * 3 + height as usize);
    for row in pixels.chunks(width as usize) {
        raw.push(0); // filter: none
        for p in row {
            raw.extend_from_slice(p);
        }
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&raw)?;
    let idat = z.finish()?;

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB, deflate, no filter, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_reference() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        let png = encode_rgb(2, 1, &[[255, 0, 0], [0, 0, 255]]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }
}
