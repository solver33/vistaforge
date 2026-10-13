//! World dimensions (Minecraft 1.20.1 overworld) and [`Area`], the in-memory buffer one piece
//! of the map is generated into.

use crate::column::Rect;
use crate::sink::{BlockId, BlockSink, Palette, AIR};

/// The bottom of every world, as in vanilla.
pub const MIN_Y: i32 = -64;
/// The vanilla overworld: 384 blocks up to Y 319. Tall worlds go higher (`WorldHeight`).
pub const HEIGHT: i32 = 384;
pub const MAX_Y: i32 = MIN_Y + HEIGHT - 1;
pub const SECTIONS: usize = (HEIGHT / 16) as usize;
pub const MIN_SECTION: i32 = MIN_Y / 16;

/// Block storage for a rectangle of columns, from `MIN_Y` up to the world's top. Each column is
/// contiguous, bottom up. Only the lower part of the columns is stored, in whole sections: it
/// grows when a block is set above it, and everything above it is air. Tall worlds are mostly
/// sky, so this keeps a piece's memory in line with its ground rather than the world's height.
pub struct Area<'p> {
    pub rect: Rect,
    palette: &'p Palette,
    /// The highest Y the world allows.
    max_y: i32,
    /// Blocks stored per column, from `MIN_Y`; a multiple of 16.
    stored: usize,
    blocks: Vec<BlockId>,
}

impl<'p> Area<'p> {
    /// An area of a vanilla-height world (up to Y 319).
    pub fn new(rect: Rect, palette: &'p Palette) -> Self {
        Self::for_world(rect, palette, MAX_Y, MAX_Y)
    }

    /// An area of a world that ends at `max_y`, with room up front for blocks up to
    /// `expected_top` (it grows if something goes higher).
    pub fn for_world(rect: Rect, palette: &'p Palette, max_y: i32, expected_top: i32) -> Self {
        assert!(
            (max_y - MIN_Y + 1) % 16 == 0,
            "world height must be whole sections"
        );
        let mut area = Self {
            rect,
            palette,
            max_y,
            stored: 0,
            blocks: Vec::new(),
        };
        area.grow_to(expected_top.clamp(MIN_Y, max_y));
        area
    }

    /// The highest Y the world allows.
    pub fn max_y(&self) -> i32 {
        self.max_y
    }

    /// Makes room for blocks up to `y` and some way above it (inside the world), keeping what
    /// is stored. The slack spares a re-layout for every section when something builds upward.
    fn grow_past(&mut self, y: i32) {
        self.grow_to((y + 64).min(self.max_y));
    }

    /// Makes room for blocks up to `y` (inside the world), keeping what is stored.
    fn grow_to(&mut self, y: i32) {
        let need = ((y - MIN_Y + 1) as usize).div_ceil(16) * 16;
        if need <= self.stored {
            return;
        }
        let columns = self.rect.area();
        let mut blocks = vec![AIR; columns * need];
        for c in 0..columns {
            blocks[c * need..c * need + self.stored]
                .copy_from_slice(&self.blocks[c * self.stored..(c + 1) * self.stored]);
        }
        self.blocks = blocks;
        self.stored = need;
    }

    fn column_start(&self, x: i32, z: i32) -> Option<usize> {
        self.rect.contains(x, z).then(|| {
            ((x - self.rect.x0) as usize * self.rect.depth() as usize + (z - self.rect.z0) as usize)
                * self.stored
        })
    }

    /// The blocks of one column, bottom (Y = −64) first, up to the highest stored cell. Every
    /// cell above the end of the slice is air.
    pub fn column(&self, x: i32, z: i32) -> &[BlockId] {
        let c = self.column_start(x, z).expect("column outside the area");
        &self.blocks[c..c + self.stored]
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
        if y < MIN_Y || y - MIN_Y >= self.stored as i32 {
            return AIR;
        }
        self.column_start(x, z)
            .map(|c| self.blocks[c + (y - MIN_Y) as usize])
            .unwrap_or(AIR)
    }

    fn set(&mut self, x: i32, y: i32, z: i32, id: BlockId) {
        if !(MIN_Y..=self.max_y).contains(&y) || !self.rect.contains(x, z) {
            return;
        }
        if y - MIN_Y >= self.stored as i32 {
            if id == AIR {
                return;
            }
            self.grow_past(y);
        }
        let c = self.column_start(x, z).unwrap();
        self.blocks[c + (y - MIN_Y) as usize] = id;
    }

    fn palette(&self) -> &Palette {
        self.palette
    }

    fn fill_column(&mut self, x: i32, z: i32, y0: i32, y1: i32, id: BlockId) {
        let y0 = y0.max(MIN_Y);
        let mut y1 = y1.min(self.max_y + 1);
        if y0 >= y1 || !self.rect.contains(x, z) {
            return;
        }
        if y1 - MIN_Y > self.stored as i32 {
            if id == AIR {
                y1 = MIN_Y + self.stored as i32;
                if y0 >= y1 {
                    return;
                }
            } else {
                self.grow_past(y1 - 1);
            }
        }
        let c = self.column_start(x, z).unwrap();
        let (a, b) = ((y0 - MIN_Y) as usize, (y1 - MIN_Y) as usize);
        self.blocks[c + a..c + b].fill(id);
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
        a.set(-16, MAX_Y, 31, stone);
        a.set(0, 70, 20, stone); // outside: ignored
        a.set(-5, MAX_Y + 1, 20, stone); // above the world: ignored
        assert_eq!(a.get(-1, -64, 17), stone);
        assert_eq!(a.get(-16, MAX_Y, 31), stone);
        assert_eq!(a.get(0, 70, 20), AIR);
        a.fill_column(-8, 20, -100, 10, stone);
        assert_eq!(a.top(-8, 20), Some(9));
        assert_eq!(a.get(-8, -64, 20), stone);
        assert!(a.set_if_air(-8, 10, 20, stone));
        assert!(!a.set_if_air(-8, 10, 20, stone));
    }

    #[test]
    fn tall_areas_grow_on_demand() {
        let palette = Palette::new();
        let stone = palette.intern(&BlockState {
            name: "minecraft:stone".into(),
            props: vec![],
        });
        let mut a = Area::for_world(Rect::new(0, 0, 4, 4), &palette, 2031, 100);
        assert_eq!(a.column(1, 1).len(), 176);
        a.fill_column(1, 1, -64, 90, stone);
        a.set(1, 1500, 1, AIR); // air above the stored part changes nothing
        assert_eq!(a.column(1, 1).len(), 176);
        a.set(2, 1500, 3, stone);
        a.fill_column(3, 0, 1990, 2040, stone); // clipped at the top of the world
        assert_eq!(a.column(1, 1).len(), 2096);
        // What was there before growing is kept.
        assert_eq!(a.top(1, 1), Some(89));
        assert_eq!(a.get(1, -64, 1), stone);
        assert_eq!(a.get(2, 1500, 3), stone);
        assert_eq!(a.top(3, 0), Some(2031));
        assert_eq!(a.get(3, 2032, 0), AIR);
        assert_eq!(a.max_y(), 2031);
    }
}
