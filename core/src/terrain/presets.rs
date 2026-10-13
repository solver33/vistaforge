//! The preset library shown in the app: starting points for every kind of landscape the
//! terrain can make. Each preset is a biome mix plus slider settings; the user can change
//! both afterwards.

use crate::biome::Biome;
use crate::settings::{BiomeShare, Preset, Sliders};

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
            "meadow",
            "Meadow",
            "Rolling flower meadows with copses of trees.",
            &[(Meadow, 0.8), (Plains, 0.2)],
            |s| {
                s.trees = 0.3;
                s.vegetation = 0.8;
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
            "mixed_woodland",
            "Mixed woodland",
            "Oak and birch woods broken by meadows and fields.",
            &[
                (Forest, 0.45),
                (BirchForest, 0.3),
                (Meadow, 0.15),
                (Plains, 0.1),
            ],
            |s| {
                s.trees = 0.65;
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
        preset(
            "forest_lakes",
            "Forest lakes",
            "Woodland dotted with lakes and ponds joined by streams.",
            &[
                (Forest, 0.5),
                (BirchForest, 0.2),
                (Taiga, 0.1),
                (Meadow, 0.2),
            ],
            |s| {
                s.water = 0.85;
                s.trees = 0.65;
            },
        ),
        preset(
            "birch_hills",
            "Birch hills",
            "Birch-covered hills and flowery valleys.",
            &[(BirchForest, 0.6), (Meadow, 0.4)],
            |s| {
                s.relief = 0.65;
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
            "northern_taiga",
            "Northern taiga",
            "Endless spruce and pine with bogs, lakes and bare hilltops.",
            &[
                (Taiga, 0.55),
                (PineForest, 0.2),
                (Marsh, 0.12),
                (Highlands, 0.13),
            ],
            |s| {
                s.water = 0.7;
                s.trees = 0.7;
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
        preset(
            "pine_highlands",
            "Pine highlands",
            "Rugged uplands with pine woods and rocky outcrops.",
            &[(PineForest, 0.5), (Highlands, 0.5)],
            |s| {
                s.relief = 0.65;
                s.roughness = 0.65;
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
            "wetlands",
            "Wetlands",
            "A patchwork of marsh, swamp forest and wet meadow around slow rivers.",
            &[(Marsh, 0.35), (Swamp, 0.3), (Meadow, 0.2), (Forest, 0.15)],
            |s| {
                s.water = 0.8;
                s.rivers = 0.7;
                s.relief = 0.3;
            },
        ),
        preset(
            "swamp_forest_edge",
            "Swamp forest edge",
            "Dry oak woods falling away into swamp and marsh.",
            &[(Forest, 0.45), (Swamp, 0.35), (Marsh, 0.2)],
            |s| {
                s.water = 0.65;
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
            "moorland",
            "Moorland",
            "Bleak upland moor with boggy hollows and bare tops.",
            &[(Highlands, 0.55), (Marsh, 0.3), (Meadow, 0.15)],
            |s| {
                s.relief = 0.45;
                s.water = 0.7;
                s.trees = 0.08;
            },
        ),
        preset(
            "heathland",
            "Heathland",
            "Low open heath and grassland under a big sky.",
            &[(Highlands, 0.6), (Meadow, 0.4)],
            |s| {
                s.relief = 0.3;
                s.trees = 0.12;
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
            "highland_lakes",
            "Highland lakes",
            "Lochs in long valleys between heathery hills.",
            &[
                (Highlands, 0.5),
                (Taiga, 0.2),
                (Marsh, 0.15),
                (Meadow, 0.15),
            ],
            |s| {
                s.water = 0.9;
                s.erosion = 0.7;
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
            "mountain_lakes",
            "Mountain lakes",
            "Mountain cirques and valleys holding clear lakes.",
            &[
                (Mountains, 0.3),
                (AlpineMeadow, 0.2),
                (Taiga, 0.3),
                (SnowyPeaks, 0.1),
                (Meadow, 0.1),
            ],
            |s| {
                s.water = 0.9;
                s.mountains = 0.6;
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
            "temperate_mix",
            "Temperate mix",
            "A bit of everything from the lowlands to the hills.",
            &[
                (Plains, 0.2),
                (Meadow, 0.15),
                (Forest, 0.2),
                (BirchForest, 0.1),
                (Swamp, 0.1),
                (Highlands, 0.15),
                (Taiga, 0.1),
            ],
            |_| {},
        ),
        preset(
            "wild_mix",
            "Wild mix",
            "Every biome on one map, from marsh to snowy peaks.",
            &[
                (Plains, 0.1),
                (Meadow, 0.08),
                (Forest, 0.1),
                (BirchForest, 0.07),
                (Taiga, 0.1),
                (PineForest, 0.07),
                (Swamp, 0.08),
                (Marsh, 0.06),
                (Highlands, 0.1),
                (Mountains, 0.12),
                (AlpineMeadow, 0.06),
                (SnowyPeaks, 0.06),
            ],
            |s| {
                s.biome_scale = 0.45;
            },
        ),
        preset(
            "valley_and_peaks",
            "Valleys and peaks",
            "Farmland valleys enclosed by steep wooded mountains.",
            &[
                (Plains, 0.25),
                (Meadow, 0.15),
                (Forest, 0.2),
                (Mountains, 0.25),
                (AlpineMeadow, 0.1),
                (SnowyPeaks, 0.05),
            ],
            |s| {
                s.biome_scale = 0.35;
                s.biome_blend = 0.35;
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
        assert!(presets.len() >= 40);
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
