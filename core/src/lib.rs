//! Vistaforge core: the block catalog, an Anvil writer for Minecraft 1.20.1 and generators.

pub mod anvil;
pub mod catalog;
pub mod level;
pub mod nbt;
pub mod noise;
pub mod testworld;
pub mod world;

use std::collections::BTreeSet;

use anyhow::{bail, Result};

use crate::catalog::Catalog;
use crate::world::World;

/// Checks that every block state used in the world is complete and valid for the catalog.
pub fn validate(world: &World, catalog: &Catalog) -> Result<usize> {
    let mut used = BTreeSet::new();
    for chunk in world.chunks.values() {
        for section in chunk.sections.iter().flatten() {
            used.extend(section.iter().copied());
        }
    }
    for &id in &used {
        let state = world.palette.state(id);
        let props: Vec<(&str, &str)> = state
            .props
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let complete = catalog.state(&state.name, &props)?;
        if &complete != state {
            bail!("incomplete block state {state}, expected {complete}");
        }
    }
    Ok(used.len())
}
