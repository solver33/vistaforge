//! In-memory block storage for a generated area, organised like Minecraft: 16×16 chunks of
//! 16×16×16 sections, covering Y −64..319 (Minecraft 1.20.1 overworld).

use std::collections::{BTreeMap, HashMap};

use crate::catalog::BlockState;

pub const MIN_Y: i32 = -64;
pub const HEIGHT: i32 = 384;
pub const SECTIONS: usize = (HEIGHT / 16) as usize;
pub const MIN_SECTION: i32 = MIN_Y / 16;

/// Interns block states so the world stores small ids. Id 0 is always air.
pub struct Palette {
    states: Vec<BlockState>,
    ids: HashMap<BlockState, u32>,
}

impl Default for Palette {
    fn default() -> Self {
        let mut p = Self {
            states: Vec::new(),
            ids: HashMap::new(),
        };
        p.id(&BlockState::air());
        p
    }
}

impl Palette {
    pub fn id(&mut self, state: &BlockState) -> u32 {
        if let Some(id) = self.ids.get(state) {
            return *id;
        }
        let id = self.states.len() as u32;
        self.states.push(state.clone());
        self.ids.insert(state.clone(), id);
        id
    }

    pub fn state(&self, id: u32) -> &BlockState {
        &self.states[id as usize]
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

/// 4096 block ids in Minecraft's order: index = (y * 16 + z) * 16 + x.
pub type Section = Box<[u32; 4096]>;

#[derive(Default)]
pub struct Chunk {
    /// Index 0 is the section at Y −64.
    pub sections: [Option<Section>; SECTIONS],
}

pub struct World {
    pub palette: Palette,
    pub chunks: BTreeMap<(i32, i32), Chunk>,
    pub biome: String,
}

impl Default for World {
    fn default() -> Self {
        Self {
            palette: Palette::default(),
            chunks: BTreeMap::new(),
            biome: "minecraft:plains".into(),
        }
    }
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    fn locate(x: i32, y: i32, z: i32) -> Option<((i32, i32), usize, usize)> {
        if !(MIN_Y..MIN_Y + HEIGHT).contains(&y) {
            return None;
        }
        let section = ((y - MIN_Y) >> 4) as usize;
        let index = ((((y & 15) << 4) | (z & 15)) << 4 | (x & 15)) as usize;
        Some(((x >> 4, z >> 4), section, index))
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, state: &BlockState) {
        let Some((chunk, section, index)) = Self::locate(x, y, z) else {
            return;
        };
        let id = self.palette.id(state);
        let chunk = self.chunks.entry(chunk).or_default();
        let section = chunk.sections[section].get_or_insert_with(|| Box::new([0u32; 4096]));
        section[index] = id;
    }

    pub fn set_id(&mut self, x: i32, y: i32, z: i32, id: u32) {
        let Some((chunk, section, index)) = Self::locate(x, y, z) else {
            return;
        };
        let chunk = self.chunks.entry(chunk).or_default();
        let section = chunk.sections[section].get_or_insert_with(|| Box::new([0u32; 4096]));
        section[index] = id;
    }

    pub fn get_id(&self, x: i32, y: i32, z: i32) -> u32 {
        let Some((chunk, section, index)) = Self::locate(x, y, z) else {
            return 0;
        };
        self.chunks
            .get(&chunk)
            .and_then(|c| c.sections[section].as_ref())
            .map(|s| s[index])
            .unwrap_or(0)
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> &BlockState {
        self.palette.state(self.get_id(x, y, z))
    }

    /// Makes sure a chunk exists even if it stays all air, so it is written as generated.
    pub fn touch_chunk(&mut self, cx: i32, cz: i32) {
        self.chunks.entry((cx, cz)).or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get() {
        let mut w = World::new();
        let stone = BlockState {
            name: "minecraft:stone".into(),
            props: vec![],
        };
        w.set(-1, -64, 17, &stone);
        w.set(5, 319, 5, &stone);
        w.set(5, 320, 5, &stone); // out of range, ignored
        assert_eq!(w.get(-1, -64, 17).name, "minecraft:stone");
        assert_eq!(w.get(5, 319, 5).name, "minecraft:stone");
        assert!(w.get(0, 0, 0).is_air());
        assert!(w.chunks.contains_key(&(-1, 1)));
    }
}
