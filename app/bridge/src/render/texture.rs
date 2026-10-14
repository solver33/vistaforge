//! Block textures: decoded from the game's PNGs, or made from the catalog's average colours
//! when the game's files can't be found.

use super::assets::Assets;

/// An RGBA image, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn decode_png(bytes: &[u8]) -> Option<Texture> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == 4 * n).then_some(Texture {
        width: w,
        height: h,
        rgba,
    })
}

impl Texture {
    /// A texture by id (`minecraft:block/stone`), the first frame of an animated one.
    pub fn load(assets: &Assets, id: &str) -> Option<Texture> {
        let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
        let mut t = decode_png(&assets.read(&format!("assets/{ns}/textures/{path}.png"))?)?;
        // Animations are frames stacked downwards, each as wide as the strip.
        if t.height > t.width && t.height % t.width == 0 {
            t.height = t.width;
            t.rgba.truncate((t.width * t.height * 4) as usize);
        }
        Some(t)
    }

    pub fn solid(rgb: u32) -> Texture {
        Texture {
            width: 1,
            height: 1,
            rgba: vec![(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255],
        }
    }

    /// A 16×16 tile of `rgb` with holes, for leaves and plants without their textures:
    /// `coverage` of the pixels are kept, in small clumps, and `blades` keeps them in upright
    /// strokes like grass rather than blobs like leaves.
    pub fn speckled(rgb: u32, coverage: f32, blades: bool, seed: u32) -> Texture {
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for y in 0..16u32 {
            for x in 0..16u32 {
                let cell = if blades {
                    hash(x, seed) as f32 / u32::MAX as f32 * 0.6 + (y as f32 / 16.0) * 0.8
                } else {
                    hash(x / 2 + 31 * (y / 2), seed) as f32 / u32::MAX as f32
                };
                let keep = if blades { cell > 1.0 - coverage } else { cell < coverage };
                let shade = 0.8 + 0.4 * (hash(x + 16 * y, seed ^ 0x9e37) as f32 / u32::MAX as f32);
                let ch = |c: u32| ((c & 0xFF) as f32 * shade).min(255.0) as u8;
                rgba.extend_from_slice(&[ch(rgb >> 16), ch(rgb >> 8), ch(rgb), if keep { 255 } else { 0 }]);
            }
        }
        Texture {
            width: 16,
            height: 16,
            rgba,
        }
    }

    /// True when no pixel is see-through.
    pub fn opaque(&self) -> bool {
        self.rgba.chunks_exact(4).all(|p| p[3] == 255)
    }

    /// The image scaled to `size`×`size`: nearest pixel when growing, box average when
    /// shrinking (alpha decided by the majority, as the game's cutout does).
    pub fn resampled(&self, size: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let (x0, x1) = (x * self.width / size, ((x + 1) * self.width).div_ceil(size));
                let (y0, y1) = (y * self.height / size, ((y + 1) * self.height).div_ceil(size));
                let (x1, y1) = (x1.max(x0 + 1).min(self.width), y1.max(y0 + 1).min(self.height));
                let mut sum = [0u32; 4];
                let mut solid = 0;
                let mut count = 0;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let p = &self.rgba[((sy * self.width + sx) * 4) as usize..][..4];
                        if p[3] >= 128 {
                            for c in 0..3 {
                                sum[c] += p[c] as u32;
                            }
                            solid += 1;
                        }
                        sum[3] += p[3] as u32;
                        count += 1;
                    }
                }
                if solid == 0 {
                    let p = &self.rgba[((y0 * self.width + x0) * 4) as usize..][..4];
                    out.extend_from_slice(&[p[0], p[1], p[2], 0]);
                } else {
                    let a = if solid * 2 >= count { 255 } else { 0 };
                    out.extend_from_slice(&[
                        (sum[0] / solid) as u8,
                        (sum[1] / solid) as u8,
                        (sum[2] / solid) as u8,
                        if self.opaque() { (sum[3] / count) as u8 } else { a },
                    ]);
                }
            }
        }
        out
    }

    /// Average colour of the visible pixels, 0xRRGGBB.
    pub fn average(&self) -> u32 {
        let mut sum = [0u64; 3];
        let mut n = 0u64;
        for p in self.rgba.chunks_exact(4).filter(|p| p[3] >= 128) {
            for c in 0..3 {
                sum[c] += p[c] as u64;
            }
            n += 1;
        }
        if n == 0 {
            return 0x808080;
        }
        ((sum[0] / n) << 16 | (sum[1] / n) << 8 | (sum[2] / n)) as u32
    }
}

fn hash(a: u32, seed: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG of the given RGBA pixels.
    pub fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, width, height);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(rgba).unwrap();
        out
    }

    #[test]
    fn decodes_and_resamples() {
        let px: Vec<u8> = (0..4).flat_map(|i| [i * 60, 0, 0, if i == 3 { 0 } else { 255 }]).collect();
        let t = decode_png(&png(2, 2, &px)).unwrap();
        assert_eq!((t.width, t.height), (2, 2));
        assert_eq!(t.rgba, px);
        assert!(!t.opaque());
        let big = t.resampled(4);
        assert_eq!(big.len(), 64);
        // Nearest pixel when growing: the top-left 2×2 block is pixel 0.
        assert_eq!(&big[0..4], &[0, 0, 0, 255]);
        assert_eq!(&big[(4 + 1) * 4..][..4], &[0, 0, 0, 255]);
        assert_eq!(big[(3 * 4 + 3) * 4 + 3], 0);
        let small = t.resampled(1);
        assert_eq!(small, vec![60, 0, 0, 255]);
        assert_eq!(Texture::solid(0x123456).average(), 0x123456);
    }

    #[test]
    fn speckles_keep_about_the_coverage() {
        let t = Texture::speckled(0x40a040, 0.4, false, 7);
        let kept = t.rgba.chunks_exact(4).filter(|p| p[3] == 255).count() as f32 / 256.0;
        assert!((0.2..0.6).contains(&kept), "{kept}");
    }
}
