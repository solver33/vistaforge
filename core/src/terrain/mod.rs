//! Terrain: the shape of the land, water, and where each biome is.
//!
//! [`Terrain::new`] does any whole-map work up front (erosion, river networks, biome layout);
//! [`Terrain::column`] must then be cheap, thread-safe and deterministic, because the engine
//! calls it from many threads, for overlapping pieces of the map, and expects the same answer
//! for the same column every time.
//!
//! The current implementation is a placeholder (gentle plains with one pond) that keeps the
//! pipeline working until the real terrain lands.

pub mod presets;

use anyhow::Result;

use crate::biome::Biome;
use crate::column::{ColumnGrid, ColumnInfo, Rect, WaterKind};
use crate::noise::fbm2;
use crate::settings::WorldSettings;

/// How much whole-map work to do: previews need an answer in about a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Preview,
    Full,
}

pub struct Terrain {
    seed: u64,
    size: i32,
    sea_level: i32,
}

impl Terrain {
    pub fn new(settings: &WorldSettings, _quality: Quality) -> Result<Self> {
        Ok(Self {
            seed: settings.seed,
            size: settings.size as i32,
            sea_level: settings.sea_level,
        })
    }

    /// Side of the map in blocks.
    pub fn size(&self) -> i32 {
        self.size
    }

    fn height_at(&self, x: f64, z: f64) -> (f64, f64) {
        let mut h = self.sea_level as f64
            + 3.5
            + 4.5 * fbm2(self.seed, x / 70.0, z / 70.0, 4)
            + 1.2 * fbm2(self.seed ^ 0xA5, x / 18.0, z / 18.0, 2);
        let size = self.size as f64;
        let (px, pz, r) = (size * 0.70, size * 0.36, (size * 0.11).min(40.0));
        let d = ((x - px).powi(2) + (z - pz).powi(2)).sqrt() / r;
        if d < 2.0 {
            let t = 1.0 - d / 2.0;
            h -= 8.5 * t * t * (3.0 - 2.0 * t);
        }
        (h, d)
    }

    pub fn column(&self, x: i32, z: i32) -> ColumnInfo {
        let (fx, fz) = (x as f64, z as f64);
        let (h, pond) = self.height_at(fx, fz);
        let dx = self.height_at(fx + 1.0, fz).0 - self.height_at(fx - 1.0, fz).0;
        let dz = self.height_at(fx, fz + 1.0).0 - self.height_at(fx, fz - 1.0).0;
        let wet = h < self.sea_level as f64 + 0.9;
        let biome = if wet {
            Biome::Lake
        } else if pond < 2.0 && h < self.sea_level as f64 + 1.6 {
            Biome::Beach
        } else {
            Biome::Plains
        };
        ColumnInfo {
            water_level: wet.then_some(self.sea_level),
            water: if wet {
                WaterKind::Lake
            } else {
                WaterKind::None
            },
            slope: ((dx * dx + dz * dz).sqrt() / 2.0) as f32,
            moisture: if pond < 2.5 { 0.8 } else { 0.5 },
            ..ColumnInfo::dry(h as f32, biome)
        }
    }

    pub fn grid(&self, rect: Rect) -> ColumnGrid {
        ColumnGrid::build(rect, |x, z| self.column(x, z))
    }
}
