//! Vistaforge core: generates Minecraft 1.20.1 maps built from Conquest Reforged blocks.
//!
//! Pipeline (see `docs/architecture.md`): [`terrain`] shapes the land and places biomes →
//! [`surface`] picks the blocks of each column → [`decor`] plans trees, which [`trees`] builds,
//! and adds plants and ground detail → [`engine`] streams it all to Anvil files.

pub mod anvil;
pub mod biome;
pub mod catalog;
pub mod column;
pub mod decor;
pub mod engine;
pub mod level;
pub mod nbt;
pub mod noise;
pub mod png;
pub mod settings;
pub mod sink;
pub mod surface;
pub mod terrain;
pub mod trees;
pub mod world;
