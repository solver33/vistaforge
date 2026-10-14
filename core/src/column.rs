//! What the terrain says about one column of the map. This is the hand-off from `terrain`
//! (shape, water, biomes) to `surface` and `decor` (which blocks go where).

use serde::{Deserialize, Serialize};

use crate::biome::Biome;

/// A half-open rectangle of columns: `x0..x1` × `z0..z1`, in world block coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
}

impl Rect {
    pub fn new(x0: i32, z0: i32, x1: i32, z1: i32) -> Self {
        Self { x0, z0, x1, z1 }
    }

    /// The square `0..size` × `0..size`.
    pub fn square(size: i32) -> Self {
        Self::new(0, 0, size, size)
    }

    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    pub fn depth(&self) -> i32 {
        self.z1 - self.z0
    }

    pub fn area(&self) -> usize {
        (self.width().max(0) as usize) * (self.depth().max(0) as usize)
    }

    pub fn contains(&self, x: i32, z: i32) -> bool {
        x >= self.x0 && x < self.x1 && z >= self.z0 && z < self.z1
    }

    pub fn expand(&self, by: i32) -> Self {
        Self::new(self.x0 - by, self.z0 - by, self.x1 + by, self.z1 + by)
    }

    pub fn intersect(&self, other: &Rect) -> Self {
        Self::new(
            self.x0.max(other.x0),
            self.z0.max(other.z0),
            self.x1.min(other.x1),
            self.z1.min(other.z1),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.z1 <= self.z0
    }

    /// Every column, x-major.
    pub fn columns(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (self.x0..self.x1).flat_map(move |x| (self.z0..self.z1).map(move |z| (x, z)))
    }
}

/// What kind of water covers a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaterKind {
    #[default]
    None,
    Lake,
    River,
    /// Shallow standing water among vegetation.
    Swamp,
    Sea,
}

/// Terrain facts for one column. Fields may be added; existing ones keep their meaning.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnInfo {
    /// Top of the ground in blocks. The integer part is the Y of the first air (or water) cell
    /// above full blocks; the fraction becomes Conquest layers in 1/8 steps.
    pub height: f32,
    /// Y of the topmost water block when the column is under water.
    pub water_level: Option<i32>,
    pub water: WaterKind,
    /// Main biome, and the one it blends into near a border with `blend` weight (0..=0.5).
    pub biome: Biome,
    pub biome2: Biome,
    pub blend: f32,
    /// Steepness: height change per block (0 flat, 1 is 45°).
    pub slope: f32,
    /// 0 dry .. 1 waterlogged ground.
    pub moisture: f32,
    /// 0 .. 1: how much bare rock shows (cliffs, eroded ridges, high peaks).
    pub rock: f32,
    /// 0 .. 1: water flow from erosion (gullies, stream beds, scree runs).
    pub flow: f32,
    /// Waterfall: the Y of the topmost block of a curtain of falling water standing on this
    /// column's water, which fills `water_level + 1 ..= fall`. Set on river columns at the
    /// foot of a drop; the water above the lip is at `fall` in the column upstream.
    #[serde(default)]
    pub fall: Option<i32>,
    /// Road: how far inside a road's edge the column is, in blocks. Negative on the verge
    /// beside the road, where the ground was levelled but no road was laid; `None` away from
    /// roads. Surface lays the road's earth, decor keeps plants, trees and rocks off it.
    #[serde(default)]
    pub road: Option<f32>,
}

impl ColumnInfo {
    pub fn dry(height: f32, biome: Biome) -> Self {
        Self {
            height,
            water_level: None,
            water: WaterKind::None,
            biome,
            biome2: biome,
            blend: 0.0,
            slope: 0.0,
            moisture: 0.5,
            rock: 0.0,
            flow: 0.0,
            fall: None,
            road: None,
        }
    }

    pub fn is_wet(&self) -> bool {
        self.water_level.is_some()
    }

    /// Where the ground ends, using the shared rounding rule: the Y of the cell that holds the
    /// partial Conquest layer (or of the first air/water cell when `layers` is 0), and the
    /// layer count 0..=7. A tree trunk or a plant on this column starts at that Y.
    pub fn ground(&self) -> (i32, u8) {
        let base = self.height.floor();
        let layers = ((self.height - base) * 8.0).round() as i32;
        if layers >= 8 {
            (base as i32 + 1, 0)
        } else {
            (base as i32, layers as u8)
        }
    }

    /// Water depth in blocks (0 when dry).
    pub fn depth(&self) -> f32 {
        self.water_level
            .map(|w| (w as f32 + 1.0 - self.height).max(0.0))
            .unwrap_or(0.0)
    }
}

/// Column facts for every column of a rectangle.
#[derive(Clone, Debug)]
pub struct ColumnGrid {
    pub rect: Rect,
    pub columns: Vec<ColumnInfo>,
}

impl ColumnGrid {
    pub fn build(rect: Rect, mut column: impl FnMut(i32, i32) -> ColumnInfo) -> Self {
        let columns = rect.columns().map(|(x, z)| column(x, z)).collect();
        Self { rect, columns }
    }

    fn index(&self, x: i32, z: i32) -> usize {
        ((x - self.rect.x0) * self.rect.depth() + (z - self.rect.z0)) as usize
    }

    /// The column at (x, z), which must lie inside `rect`.
    pub fn get(&self, x: i32, z: i32) -> &ColumnInfo {
        &self.columns[self.index(x, z)]
    }

    pub fn try_get(&self, x: i32, z: i32) -> Option<&ColumnInfo> {
        self.rect.contains(x, z).then(|| self.get(x, z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_indexing() {
        let rect = Rect::new(-3, 5, 4, 9);
        let grid = ColumnGrid::build(rect, |x, z| {
            ColumnInfo::dry((x * 100 + z) as f32, Biome::Plains)
        });
        assert_eq!(grid.columns.len(), rect.area());
        assert_eq!(grid.get(-3, 5).height, -295.0);
        assert_eq!(grid.get(3, 8).height, 308.0);
        assert!(grid.try_get(4, 8).is_none());
    }

    #[test]
    fn ground_rounding() {
        let c = |h| ColumnInfo::dry(h, Biome::Plains).ground();
        assert_eq!(c(64.0), (64, 0));
        assert_eq!(c(64.3), (64, 2));
        assert_eq!(c(64.97), (65, 0));
        assert_eq!(c(-0.5), (-1, 4));
    }
}
