//! The preset library shown in the app.

use crate::biome::Biome;
use crate::settings::{BiomeShare, Preset, Sliders};

pub fn all() -> Vec<Preset> {
    vec![Preset {
        id: "plains".into(),
        name: "Plains".into(),
        description: "Gentle grassland with scattered oaks and a pond.".into(),
        biomes: vec![BiomeShare {
            biome: Biome::Plains,
            share: 1.0,
        }],
        sliders: Sliders::default(),
        sea_level: 63,
    }]
}

pub fn find(id: &str) -> Option<Preset> {
    all().into_iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_settings_valid() {
        let presets = all();
        for (i, p) in presets.iter().enumerate() {
            assert!(presets[i + 1..].iter().all(|q| q.id != p.id), "{}", p.id);
            assert!(p.settings("t", 512, 1).validate().is_ok(), "{}", p.id);
        }
    }
}
