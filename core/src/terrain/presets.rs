//! The preset library shown in the app: starting points for every kind of landscape the
//! terrain can make. Each preset is a biome mix plus slider settings; the user can change
//! both afterwards.

use crate::biome::Biome;
use crate::settings::{BiomeShare, Preset, Sliders, WorldHeight};

use Biome::*;

fn preset(
    id: &str,
    name: &str,
    description: &str,
    mix: &[(Biome, f32)],
    tune: impl FnOnce(&mut Sliders),
) -> Preset {
    let mut sliders = Sliders::default();
    tune(&mut sliders);
    Preset {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        biomes: mix
            .iter()
            .map(|&(biome, share)| BiomeShare { biome, share })
            .collect(),
        sliders,
        sea_level: 63,
        world_height: None,
    }
}

/// A preset for a tall world (up to Y 2031), whose mountains use the extra height.
fn tall(
    id: &str,
    name: &str,
    description: &str,
    mix: &[(Biome, f32)],
    tune: impl FnOnce(&mut Sliders),
) -> Preset {
    Preset {
        world_height: Some(WorldHeight::Tall),
        ..preset(id, name, description, mix, tune)
    }
}

pub fn all() -> Vec<Preset> {
    vec![
        // Lowlands.
        preset(
            "plains",
            "Plains",
            "Gentle grassland with scattered oaks, a few ponds and slow streams.",
            &[(Plains, 1.0)],
            |s| {
                s.relief = 0.4;
                s.trees = 0.35;
            },
        ),
        preset(
            "flatlands",
            "Flatlands",
            "Almost flat open country, good for building, with shallow streams.",
            &[(Plains, 0.7), (Meadow, 0.3)],
            |s| {
                s.relief = 0.08;
                s.roughness = 0.25;
                s.erosion = 0.2;
                s.rivers = 0.35;
                s.water = 0.3;
            },
        ),
        preset(
            "countryside",
            "Countryside",
            "Fields, meadows and patches of woodland on low hills, cut by brooks.",
            &[
                (Plains, 0.4),
                (Meadow, 0.25),
                (Forest, 0.25),
                (BirchForest, 0.1),
            ],
            |s| {
                s.rivers = 0.6;
            },
        ),
        preset(
            "rolling_hills",
            "Rolling hills",
            "Soft green hills with wooded slopes and winding valleys.",
            &[(Meadow, 0.5), (Plains, 0.3), (Forest, 0.2)],
            |s| {
                s.relief = 0.7;
                s.erosion = 0.6;
            },
        ),
        preset(
            "great_river_plains",
            "Great river plains",
            "Wide lowland plains with big meandering rivers and oxbow ponds.",
            &[(Plains, 0.65), (Meadow, 0.2), (Marsh, 0.15)],
            |s| {
                s.relief = 0.25;
                s.rivers = 0.95;
                s.water = 0.6;
                s.trees = 0.3;
            },
        ),
        // Forests.
        preset(
            "oak_forest",
            "Oak forest",
            "Dense broadleaf woodland on gentle hills with clearings and streams.",
            &[(Forest, 0.85), (Meadow, 0.15)],
            |s| {
                s.trees = 0.75;
            },
        ),
        preset(
            "birch_forest",
            "Birch forest",
            "Light birch woods with grassy clearings.",
            &[(BirchForest, 0.85), (Meadow, 0.15)],
            |s| {
                s.trees = 0.7;
            },
        ),
        preset(
            "old_growth_forest",
            "Old-growth forest",
            "Deep, dense forest of oak, spruce and birch over rough ground.",
            &[(Forest, 0.6), (Taiga, 0.2), (BirchForest, 0.2)],
            |s| {
                s.trees = 0.95;
                s.detail = 0.8;
                s.roughness = 0.65;
            },
        ),
        // Northern.
        preset(
            "taiga",
            "Taiga",
            "Spruce forest on hilly ground with bogs and rocky knolls.",
            &[(Taiga, 0.85), (Marsh, 0.15)],
            |s| {
                s.trees = 0.75;
            },
        ),
        preset(
            "pine_forest",
            "Pine forest",
            "Dry pine woods on sandy, rocky ridges.",
            &[(PineForest, 0.85), (Plains, 0.15)],
            |s| {
                s.trees = 0.6;
                s.water = 0.3;
            },
        ),
        preset(
            "boreal_lakes",
            "Boreal lakes",
            "Glacier-scoured land of countless lakes between spruce ridges.",
            &[
                (Taiga, 0.45),
                (PineForest, 0.25),
                (Marsh, 0.15),
                (Highlands, 0.15),
            ],
            |s| {
                s.water = 1.0;
                s.rivers = 0.6;
                s.erosion = 0.65;
            },
        ),
        // Winter.
        preset(
            "snowy_taiga",
            "Snowy taiga",
            "Spruce forest under deep snow on hills, with frozen lakes and rivers.",
            &[(SnowyTaiga, 0.75), (Tundra, 0.1), (Taiga, 0.15)],
            |s| {
                s.relief = 0.55;
                s.trees = 0.7;
                s.water = 0.6;
            },
        ),
        preset(
            "frozen_tundra",
            "Frozen tundra",
            "Wide treeless snowy plains with countless frozen ponds and a few lone spruces.",
            &[(Tundra, 0.8), (SnowyTaiga, 0.2)],
            |s| {
                s.relief = 0.25;
                s.water = 0.8;
                s.rivers = 0.5;
                s.trees = 0.15;
                s.erosion = 0.4;
            },
        ),
        preset(
            "winter_mountains",
            "Winter mountains",
            "Snowbound ranges above valleys of snowy spruce forest.",
            &[
                (SnowyTaiga, 0.35),
                (Tundra, 0.1),
                (Mountains, 0.25),
                (SnowyPeaks, 0.3),
            ],
            |s| {
                s.relief = 0.7;
                s.mountains = 0.8;
                s.erosion = 0.8;
                s.trees = 0.55;
            },
        ),
        preset(
            "frozen_coast",
            "Frozen coast",
            "An arctic shore of snowy tundra, spruce and rocky skerries in a cold sea.",
            &[
                (Tundra, 0.4),
                (SnowyTaiga, 0.35),
                (Mountains, 0.1),
                (SnowyPeaks, 0.15),
            ],
            |s| {
                s.sea = 0.4;
                s.islands = 0.85;
                s.mountains = 0.35;
                s.trees = 0.35;
            },
        ),
        // Wetlands.
        preset(
            "swamp",
            "Swamp",
            "Flooded woodland of willows, pools and muddy channels.",
            &[(Swamp, 0.85), (Forest, 0.15)],
            |s| {
                s.water = 0.7;
                s.relief = 0.3;
                // Water stands in pools and slow channels; no rivers cut through.
                s.rivers = 0.0;
            },
        ),
        preset(
            "marsh",
            "Marsh",
            "Open wetland of reeds, sedges and pools.",
            &[(Marsh, 0.7), (Swamp, 0.15), (Plains, 0.15)],
            |s| {
                s.water = 0.75;
                s.relief = 0.25;
                s.trees = 0.15;
                s.rivers = 0.0;
            },
        ),
        preset(
            "river_delta",
            "River delta",
            "Rivers splitting through marsh and grassland to the sea.",
            &[(Marsh, 0.4), (Plains, 0.35), (Swamp, 0.25)],
            |s| {
                s.relief = 0.15;
                s.rivers = 1.0;
                s.water = 0.8;
                s.sea = 0.3;
                s.islands = 0.0;
            },
        ),
        // Tropics.
        preset(
            "jungle",
            "Jungle",
            "Dense tropical forest on steep little hills, with big rivers and swampy lowlands.",
            &[(Tropical, 0.85), (Swamp, 0.15)],
            |s| {
                s.relief = 0.6;
                s.roughness = 0.6;
                s.rivers = 0.85;
                s.trees = 0.9;
                s.vegetation = 0.85;
            },
        ),
        // Uplands.
        preset(
            "highlands",
            "Highlands",
            "Broad rolling uplands of heath and rock with deep valleys.",
            &[(Highlands, 0.75), (Meadow, 0.15), (Marsh, 0.1)],
            |s| {
                s.erosion = 0.65;
                s.trees = 0.2;
            },
        ),
        preset(
            "eroded_hills",
            "Eroded hills",
            "Hills worn into sharp ridges and deep branching gullies.",
            &[(Highlands, 0.4), (Meadow, 0.3), (PineForest, 0.3)],
            |s| {
                s.relief = 0.75;
                s.erosion = 1.0;
            },
        ),
        preset(
            "table_mountains",
            "Table mountains",
            "Flat-topped plateaus on sheer cliffs, rising in steps above forest and meadow.",
            &[
                (Forest, 0.3),
                (Meadow, 0.25),
                (Highlands, 0.25),
                (PineForest, 0.2),
            ],
            |s| {
                s.plateaus = 1.0;
                s.mountains = 0.2;
                s.erosion = 0.6;
                s.rivers = 0.6;
            },
        ),
        preset(
            "river_canyons",
            "River canyons",
            "High tablelands cut by deep canyons, with rivers and waterfalls far below.",
            &[(Highlands, 0.45), (PineForest, 0.35), (Meadow, 0.2)],
            |s| {
                s.plateaus = 0.85;
                s.relief = 0.6;
                s.erosion = 0.95;
                s.rivers = 0.9;
                s.water = 0.3;
                s.trees = 0.35;
            },
        ),
        preset(
            "foothills",
            "Foothills",
            "Forested hills climbing towards a mountain range.",
            &[
                (Forest, 0.3),
                (Highlands, 0.3),
                (Mountains, 0.2),
                (Taiga, 0.2),
            ],
            |s| {
                s.mountains = 0.4;
            },
        ),
        // Mountains.
        preset(
            "realistic_mountains",
            "Realistic mountains",
            "Eroded mountain ranges: forested valleys, alpine pasture, rock and snow.",
            &[
                (Mountains, 0.35),
                (SnowyPeaks, 0.12),
                (AlpineMeadow, 0.13),
                (Taiga, 0.25),
                (PineForest, 0.15),
            ],
            |s| {
                s.relief = 0.7;
                s.erosion = 0.75;
                s.mountains = 0.7;
            },
        ),
        preset(
            "alpine_valley",
            "Alpine valley",
            "A green valley with a river, framed by high peaks.",
            &[
                (Meadow, 0.25),
                (Forest, 0.1),
                (Taiga, 0.15),
                (AlpineMeadow, 0.15),
                (Mountains, 0.25),
                (SnowyPeaks, 0.1),
            ],
            |s| {
                s.rivers = 0.75;
                s.mountains = 0.65;
                s.erosion = 0.7;
            },
        ),
        preset(
            "high_alps",
            "High Alps",
            "Huge snowy peaks and glaciated ridges above rocky slopes.",
            &[(SnowyPeaks, 0.35), (Mountains, 0.45), (AlpineMeadow, 0.2)],
            |s| {
                s.relief = 0.85;
                s.mountains = 1.0;
                s.erosion = 0.8;
                s.trees = 0.2;
            },
        ),
        preset(
            "alpine_pastures",
            "Alpine pastures",
            "High grassy slopes and rounded summits above the tree line.",
            &[(AlpineMeadow, 0.55), (Mountains, 0.2), (Meadow, 0.25)],
            |s| {
                s.mountains = 0.45;
                s.trees = 0.25;
                s.vegetation = 0.8;
            },
        ),
        preset(
            "glacial_valleys",
            "Glacial valleys",
            "Deep valleys between steep rock walls, with lakes on the valley floors.",
            &[
                (Mountains, 0.4),
                (SnowyPeaks, 0.15),
                (Taiga, 0.25),
                (Meadow, 0.2),
            ],
            |s| {
                s.relief = 0.8;
                s.erosion = 0.95;
                s.water = 0.7;
                s.mountains = 0.8;
            },
        ),
        preset(
            "rocky_uplands",
            "Rocky uplands",
            "Bare, broken rock country with sparse pines.",
            &[(Mountains, 0.45), (Highlands, 0.35), (PineForest, 0.2)],
            |s| {
                s.roughness = 0.85;
                s.mountains = 0.35;
                s.trees = 0.2;
                s.water = 0.2;
            },
        ),
        preset(
            "volcanic_field",
            "Volcanic field",
            "Several volcanoes with crater lakes over ash, lava ridges and pine woods.",
            &[
                (Volcanic, 0.35),
                (Highlands, 0.25),
                (PineForest, 0.25),
                (Marsh, 0.15),
            ],
            |s| {
                s.volcanoes = 0.9;
                s.plateaus = 0.25;
                s.mountains = 0.3;
                s.water = 0.6;
                s.trees = 0.35;
            },
        ),
        // Tall worlds.
        tall(
            "epic_peaks",
            "Epic peaks",
            "Tall world: a range of rock and snow about 1500 blocks high above forested valleys.",
            &[
                (Taiga, 0.15),
                (PineForest, 0.1),
                (AlpineMeadow, 0.1),
                (Mountains, 0.35),
                (SnowyPeaks, 0.3),
            ],
            |s| {
                s.relief = 0.65;
                s.mountains = 1.0;
                s.erosion = 0.85;
                s.rivers = 0.6;
                s.trees = 0.6;
            },
        ),
        tall(
            "great_valley",
            "Great valley",
            "Tall world: a wide green valley with a river and lakes under sheer mountain walls.",
            &[
                (Meadow, 0.25),
                (Forest, 0.15),
                (PineForest, 0.15),
                (AlpineMeadow, 0.1),
                (Mountains, 0.28),
                (SnowyPeaks, 0.07),
            ],
            |s| {
                s.relief = 0.6;
                s.mountains = 1.0;
                s.erosion = 0.75;
                s.rivers = 0.8;
                s.water = 0.6;
            },
        ),
        tall(
            "lost_plateaus",
            "Lost plateaus",
            "Tall world: flat-topped mountains on sheer cliffs hundreds of blocks high above a jungle.",
            &[(Tropical, 0.7), (Highlands, 0.15), (Swamp, 0.15)],
            |s| {
                s.plateaus = 1.0;
                s.mountains = 0.0;
                s.relief = 0.5;
                s.erosion = 0.7;
                s.rivers = 0.75;
                s.water = 0.5;
                s.trees = 0.8;
            },
        ),
        tall(
            "mega_world",
            "Mega world",
            "Tall world: every biome on land that steps from the sea to plateaus and peaks near the sky.",
            &[
                (Plains, 0.06),
                (Meadow, 0.06),
                (Forest, 0.08),
                (BirchForest, 0.05),
                (Taiga, 0.07),
                (PineForest, 0.06),
                (Swamp, 0.04),
                (Marsh, 0.04),
                (Highlands, 0.07),
                (Mountains, 0.12),
                (AlpineMeadow, 0.06),
                (SnowyPeaks, 0.08),
                (Tropical, 0.06),
                (SnowyTaiga, 0.06),
                (Tundra, 0.04),
                (Volcanic, 0.05),
            ],
            |s| {
                s.plateaus = 0.9;
                s.mountains = 0.8;
                s.relief = 0.6;
                s.erosion = 0.8;
                s.rivers = 0.7;
                s.water = 0.6;
                s.sea = 0.12;
                s.islands = 0.0;
                s.volcanoes = 0.2;
                s.biome_scale = 0.65;
            },
        ),
        // Coasts and islands.
        preset(
            "coast",
            "Coast",
            "Farmland and woods running down to beaches and the sea.",
            &[
                (Plains, 0.4),
                (Meadow, 0.2),
                (Forest, 0.3),
                (Highlands, 0.1),
            ],
            |s| {
                s.sea = 0.35;
                s.islands = 0.0;
            },
        ),
        preset(
            "island",
            "Island",
            "A single large island with hills, woods and beaches.",
            &[
                (Forest, 0.35),
                (Meadow, 0.25),
                (Plains, 0.2),
                (Highlands, 0.2),
            ],
            |s| {
                s.sea = 0.45;
                s.islands = 0.5;
            },
        ),
        preset(
            "archipelago",
            "Archipelago",
            "Many wooded islands in sheltered water.",
            &[
                (Forest, 0.3),
                (Plains, 0.25),
                (Meadow, 0.2),
                (Highlands, 0.15),
                (PineForest, 0.1),
            ],
            |s| {
                s.sea = 0.62;
                s.islands = 1.0;
            },
        ),
        preset(
            "fjords",
            "Fjords",
            "Steep mountains plunging into long arms of the sea.",
            &[
                (Mountains, 0.35),
                (Taiga, 0.3),
                (SnowyPeaks, 0.1),
                (AlpineMeadow, 0.1),
                (PineForest, 0.15),
            ],
            |s| {
                s.sea = 0.35;
                s.islands = 0.75;
                s.relief = 0.75;
                s.erosion = 0.85;
                s.mountains = 0.75;
            },
        ),
        preset(
            "volcano_island",
            "Tropical volcano island",
            "A jungle island around a great volcano with a crater lake, palm beaches and lava fields.",
            &[(Tropical, 0.88), (Volcanic, 0.12)],
            |s| {
                s.sea = 0.5;
                s.islands = 0.5;
                s.volcanoes = 0.6;
                s.water = 0.5;
                s.rivers = 0.55;
                s.trees = 0.7;
                s.vegetation = 0.7;
            },
        ),
        preset(
            "tropical_archipelago",
            "Tropical archipelago",
            "Jungle islands with white beaches in warm shallow water, a few of them volcanic.",
            &[(Tropical, 0.9), (Volcanic, 0.1)],
            |s| {
                s.sea = 0.6;
                s.islands = 1.0;
                s.volcanoes = 0.3;
                s.trees = 0.7;
                s.vegetation = 0.7;
            },
        ),
        preset(
            "pine_coast",
            "Pine coast",
            "Rocky, pine-clad shore with coves and skerries.",
            &[(PineForest, 0.5), (Highlands, 0.2), (Plains, 0.3)],
            |s| {
                s.sea = 0.4;
                s.islands = 0.85;
            },
        ),
        preset(
            "mountain_island",
            "Mountain island",
            "An island rising from wooded shores to a rocky, snowy summit.",
            &[
                (Forest, 0.3),
                (Taiga, 0.2),
                (Mountains, 0.25),
                (SnowyPeaks, 0.1),
                (AlpineMeadow, 0.15),
            ],
            |s| {
                s.sea = 0.4;
                s.islands = 0.5;
                s.mountains = 0.7;
            },
        ),
        // Mixes.
        preset(
            "river_valley",
            "River valley",
            "A broad valley with a big river between wooded hills.",
            &[
                (Plains, 0.35),
                (Meadow, 0.2),
                (Forest, 0.3),
                (Highlands, 0.15),
            ],
            |s| {
                s.rivers = 0.9;
                s.relief = 0.6;
            },
        ),
        preset(
            "lake_district",
            "Lake district",
            "Fells, woods and many long lakes.",
            &[
                (Highlands, 0.3),
                (Meadow, 0.25),
                (Forest, 0.25),
                (Taiga, 0.2),
            ],
            |s| {
                s.water = 1.0;
                s.relief = 0.65;
                s.erosion = 0.7;
            },
        ),
        preset(
            "wild_mix",
            "Wild mix",
            "Every biome on one map, from marsh to snowy peaks.",
            &[
                (Plains, 0.08),
                (Meadow, 0.06),
                (Forest, 0.08),
                (BirchForest, 0.06),
                (Taiga, 0.07),
                (PineForest, 0.06),
                (Swamp, 0.06),
                (Marsh, 0.05),
                (Highlands, 0.07),
                (Mountains, 0.09),
                (AlpineMeadow, 0.05),
                (SnowyPeaks, 0.05),
                (Tropical, 0.06),
                (SnowyTaiga, 0.06),
                (Tundra, 0.05),
                (Volcanic, 0.05),
            ],
            |s| {
                s.biome_scale = 0.45;
            },
        ),
    ]
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
        assert!(presets.len() >= 30);
        for (i, p) in presets.iter().enumerate() {
            assert!(presets[i + 1..].iter().all(|q| q.id != p.id), "{}", p.id);
            assert!(p.settings("t", 512, 1).validate().is_ok(), "{}", p.id);
            assert!(
                !p.description.is_empty() && p.description.is_ascii(),
                "{}",
                p.id
            );
            let total: f32 = p.biomes.iter().map(|b| b.share).sum();
            assert!((total - 1.0).abs() < 0.02, "{} shares sum to {total}", p.id);
        }
        assert!(find("plains").is_some());
    }
}
