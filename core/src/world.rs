//! World dimensions (Minecraft 1.20.1 overworld) and [`Area`], the in-memory buffer one piece
//! of the map is generated into.

use crate::column::Rect;
use crate::sink::{BlockId, BlockSink, Palette, AIR};

pub const MIN_Y: i32 = -64;
pub const HEIGHT: i32 = 384;
pub const MAX_Y: i32 = MIN_Y + HEIGHT - 1;
pub const SECTIONS: usize = (HEIGHT / 16) as usize;
pub const MIN_SECTION: i32 = MIN_Y / 16;

/// Full-height block storage for a rectangle of columns. Each column is contiguous, bottom up.
pub struct Area<'p> {
    pub rect: Rect,
    palette: &'p Palette,
    blocks: Vec<BlockId>,
}

impl<'p> Area<'p> {
    pub fn new(rect: Rect, palette: &'p Palette) -> Self {
        Self {
            rect,
            palette,
            blocks: vec![AIR; rect.area() * HEIGHT as usize],
        }
    }

    fn column_start(&self, x: i32, z: i32) -> Option<usize> {
        self.rect.contains(x, z).then(|| {
            ((x - self.rect.x0) as usize * self.rect.depth() as usize + (z - self.rect.z0) as usize)
                * HEIGHT as usize
        })
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if !(MIN_Y..=MAX_Y).contains(&y) {
            return None;
        }
        self.column_start(x, z).map(|c| c + (y - MIN_Y) as usize)
    }

    /// The blocks of one column, bottom (Y = −64) first.
    pub fn column(&self, x: i32, z: i32) -> &[BlockId] {
        let c = self.column_start(x, z).expect("column outside the area");
        &self.blocks[c..c + HEIGHT as usize]
    }

    /// Y of the highest non-air block in a column, if any.
    pub fn top(&self, x: i32, z: i32) -> Option<i32> {
        self.column(x, z)
            .iter()
            .rposition(|&b| b != AIR)
            .map(|i| MIN_Y + i as i32)
    }
}

impl BlockSink for Area<'_> {
    fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        self.index(x, y, z).map(|i| self.blocks[i]).unwrap_or(AIR)
    }

    fn set(&mut self, x: i32, y: i32, z: i32, id: BlockId) {
        if let Some(i) = self.index(x, y, z) {
            self.blocks[i] = id;
        }
    }

    fn palette(&self) -> &Palette {
        self.palette
    }

    fn fill_column(&mut self, x: i32, z: i32, y0: i32, y1: i32, id: BlockId) {
        let (y0, y1) = (y0.max(MIN_Y), y1.min(MAX_Y + 1));
        if y0 >= y1 {
            return;
        }
        if let Some(c) = self.column_start(x, z) {
            let (a, b) = ((y0 - MIN_Y) as usize, (y1 - MIN_Y) as usize);
            self.blocks[c + a..c + b].fill(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::BlockState;

    #[test]
    fn set_get_and_clip() {
        let palette = Palette::new();
        let stone = palette.intern(&BlockState {
            name: "minecraft:stone".into(),
            props: vec![],
        });
        let mut a = Area::new(Rect::new(-16, 16, 0, 32), &palette);
        a.set(-1, -64, 17, stone);
        a.set(-16, 319, 31, stone);
        a.set(0, 70, 20, stone); // outside: ignored
        a.set(-5, 320, 20, stone); // above the world: ignored
        assert_eq!(a.get(-1, -64, 17), stone);
        assert_eq!(a.get(-16, 319, 31), stone);
        assert_eq!(a.get(0, 70, 20), AIR);
        a.fill_column(-8, 20, -100, 10, stone);
        assert_eq!(a.top(-8, 20), Some(9));
        assert_eq!(a.get(-8, -64, 20), stone);
        assert!(a.set_if_air(-8, 10, 20, stone));
        assert!(!a.set_if_air(-8, 10, 20, stone));
    }
}
