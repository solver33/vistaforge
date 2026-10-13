//! Block ids shared by one generation run, and the interface every generator writes through.
//!
//! Generators resolve the block states they need once, up front, through [`Blocks`] (which
//! checks them against the user's catalog) and then work with plain [`BlockId`]s. All writes go
//! through a [`BlockSink`], so the same code can fill a small test area or a streamed piece of a
//! 4096×4096 map.

use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::Result;

use crate::catalog::{BlockState, Catalog};

/// Index of a block state in a [`Palette`]. 0 is always air.
pub type BlockId = u32;
pub const AIR: BlockId = 0;

/// Interns block states into small ids. One palette is shared by every worker thread of a run.
pub struct Palette {
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    states: Vec<BlockState>,
    ids: HashMap<BlockState, BlockId>,
}

impl Default for Palette {
    fn default() -> Self {
        let p = Self {
            inner: RwLock::new(Inner::default()),
        };
        p.intern(&BlockState::air());
        p
    }
}

impl Palette {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&self, state: &BlockState) -> BlockId {
        if let Some(id) = self.inner.read().unwrap().ids.get(state) {
            return *id;
        }
        let mut inner = self.inner.write().unwrap();
        if let Some(id) = inner.ids.get(state) {
            return *id;
        }
        let id = inner.states.len() as BlockId;
        inner.states.push(state.clone());
        inner.ids.insert(state.clone(), id);
        id
    }

    /// Runs `f` on the state behind `id` without cloning it.
    pub fn with<R>(&self, id: BlockId, f: impl FnOnce(&BlockState) -> R) -> R {
        f(&self.inner.read().unwrap().states[id as usize])
    }

    pub fn state(&self, id: BlockId) -> BlockState {
        self.with(id, BlockState::clone)
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every interned state, in id order.
    pub fn states(&self) -> Vec<BlockState> {
        self.inner.read().unwrap().states.clone()
    }
}

/// Resolves block states against the user's catalog and interns them. Generators use this in
/// their `new` functions; a block missing from the catalog is an error there, not a silent
/// gap in the world.
#[derive(Clone, Copy)]
pub struct Blocks<'a> {
    pub catalog: &'a Catalog,
    pub palette: &'a Palette,
}

impl<'a> Blocks<'a> {
    pub fn new(catalog: &'a Catalog, palette: &'a Palette) -> Self {
        Self { catalog, palette }
    }

    /// A complete, validated state; unlisted properties take the block's defaults.
    pub fn id(&self, name: &str, props: &[(&str, &str)]) -> Result<BlockId> {
        Ok(self.palette.intern(&self.catalog.state(name, props)?))
    }

    pub fn default_id(&self, name: &str) -> Result<BlockId> {
        self.id(name, &[])
    }

    pub fn has(&self, name: &str) -> bool {
        self.catalog.contains(name)
    }
}

/// Somewhere blocks can be written: a buffered piece of the world. Positions are world
/// coordinates. Writes outside the sink's area are ignored, so a tree or rock that crosses the
/// edge of a piece is clipped there and finished by the neighbouring piece.
pub trait BlockSink {
    /// The block at a position; air outside the area.
    fn get(&self, x: i32, y: i32, z: i32) -> BlockId;

    fn set(&mut self, x: i32, y: i32, z: i32, id: BlockId);

    /// The palette the ids belong to, for looking at a block placed by another generator.
    fn palette(&self) -> &Palette;

    /// Sets blocks `y0..y1` (half-open) of one column.
    fn fill_column(&mut self, x: i32, z: i32, y0: i32, y1: i32, id: BlockId) {
        for y in y0..y1 {
            self.set(x, y, z, id);
        }
    }

    /// Sets a block only where there is air now.
    fn set_if_air(&mut self, x: i32, y: i32, z: i32, id: BlockId) -> bool {
        if self.get(x, y, z) == AIR {
            self.set(x, y, z, id);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_interns_once() {
        let p = Palette::new();
        let stone = BlockState {
            name: "minecraft:stone".into(),
            props: vec![],
        };
        assert_eq!(p.intern(&BlockState::air()), AIR);
        let a = p.intern(&stone);
        assert_eq!(p.intern(&stone), a);
        assert_eq!(p.len(), 2);
        assert_eq!(p.with(a, |s| s.name.clone()), "minecraft:stone");
    }
}
