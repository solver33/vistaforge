//! What the ground of each biome is made of. A column falls into one zone (open ground, under
//! trees, damp, stony slope, scree, shore, or a water bed of some depth), and each zone mixes a
//! few materials in irregular patches: large `base` patches, with small `accent` patches
//! scattered through them.
//!
//! `@debris` stands for the loose rock of the local geology (see `geology`).

use crate::biome::Biome;

pub(crate) struct Mix {
    pub base: &'static [(&'static str, u32)],
    /// Typical size of a base patch, in blocks.
    pub scale: f64,
    pub accent: &'static [(&'static str, u32)],
    pub accent_scale: f64,
    /// Share of the ground the accents cover.
    pub accent_share: f32,
}

const fn mix(base: &'static [(&'static str, u32)], scale: f64) -> Mix {
    Mix {
        base,
        scale,
        accent: &[],
        accent_scale: 4.0,
        accent_share: 0.0,
    }
}

const fn mixa(
    base: &'static [(&'static str, u32)],
    scale: f64,
    accent: &'static [(&'static str, u32)],
    accent_scale: f64,
    accent_share: f32,
) -> Mix {
    Mix {
        base,
        scale,
        accent,
        accent_scale,
        accent_share,
    }
}

pub(crate) struct Recipe {
    /// Open ground: grassland, clearings.
    pub open: Mix,
    /// Under trees (see `field::woodland`).
    pub wood: Mix,
    /// Damp ground (high moisture).
    pub wet: Mix,
    /// Moderate slopes and runnels.
    pub stony: Mix,
    /// Steep slopes and erosion channels.
    pub scree: Mix,
    /// Dry ground right next to water.
    pub shore: Mix,
    /// Water beds: shallow (< 1.2 blocks), medium (< 3.5) and deep.
    pub beds: [Mix; 3],
    /// Soil under organic covers, and the soil below that.
    pub topsoil: &'static str,
    pub subsoil: &'static str,
    /// Typical soil depth on flat ground, in blocks.
    pub soil_depth: f32,
    /// Slopes above which the ground turns stony, then to scree.
    pub stony_slope: f32,
    pub scree_slope: f32,
    /// Slope range over which bare rock takes over.
    pub rock_slope: (f32, f32),
    /// Snow lies on everything but steep rock.
    pub snow: bool,
    /// Bare rock is moss-grown where the rock allows it.
    pub mossy_rock: bool,
}

const LAKE_BEDS: [Mix; 3] = [
    mixa(
        &[("wet_sand", 4), ("wet_sand_gravel", 3)],
        7.0,
        &[("pebbles", 2), ("wet_sand_veg", 1)],
        3.0,
        0.15,
    ),
    mixa(
        &[("sand", 3), ("sand_gravel", 3), ("light_gravel", 1)],
        9.0,
        &[("pebbles", 1), ("gravel", 1)],
        4.0,
        0.12,
    ),
    mixa(
        &[("light_gray_sand", 2), ("sand_gravel", 2), ("gravel", 2)],
        11.0,
        &[("clay", 1), ("light_gravel", 1)],
        5.0,
        0.15,
    ),
];

const RIVER_BEDS: [Mix; 3] = [
    mixa(
        &[
            ("pebbles", 3),
            ("wet_river_sand", 2),
            ("wet_sand_gravel", 2),
        ],
        5.0,
        &[("mossy_stones", 1), ("small_stones", 1)],
        3.0,
        0.15,
    ),
    mixa(
        &[("gravel", 3), ("pebbles", 2), ("sand_gravel", 1)],
        6.0,
        &[("small_stones", 1), ("light_gravel", 1)],
        3.0,
        0.15,
    ),
    mix(
        &[("gravel", 3), ("gray_gravel", 1), ("small_stones", 1)],
        7.0,
    ),
];

const SWAMP_BEDS: [Mix; 3] = [
    mixa(
        &[("mud", 3), ("thick_mud", 2), ("clay_mud", 1)],
        5.0,
        &[("mossy_silt", 1), ("peat", 1)],
        3.0,
        0.15,
    ),
    mix(&[("thick_mud", 2), ("dark_silt", 2), ("mud", 1)], 6.0),
    mix(&[("dark_silt", 2), ("clay", 1), ("thick_mud", 1)], 8.0),
];

const STONY_BEDS: [Mix; 3] = [
    mix(
        &[("pebbles", 3), ("gray_gravel", 2), ("wet_gray_sand", 1)],
        5.0,
    ),
    mix(&[("gray_gravel", 2), ("gravel", 2), ("pebbles", 1)], 6.0),
    mix(
        &[("gravel", 2), ("dark_gravel", 1), ("gray_gravel", 1)],
        8.0,
    ),
];

const GRASS_WOOD: Mix = mixa(
    &[
        ("overgrown_fir_floor", 4),
        ("mossy_fir_floor", 3),
        ("grass", 2),
        ("autumn_floor", 2),
    ],
    9.0,
    &[
        ("mossy_roots", 3),
        ("shredded_leaves", 2),
        ("green_sphagnum", 1),
    ],
    4.0,
    0.15,
);

const PLAINS: Recipe = Recipe {
    open: mixa(
        &[("grass", 7), ("clover", 3)],
        14.0,
        &[
            ("grassy_dirt", 3),
            ("overgrown_silt", 2),
            ("coarse_dirt", 1),
            ("overgrown_stones", 1),
        ],
        5.0,
        0.1,
    ),
    wood: GRASS_WOOD,
    wet: mixa(
        &[("grass", 3), ("mossy_silt", 2), ("overgrown_silt", 2)],
        8.0,
        &[("mud", 2), ("green_sphagnum", 1)],
        4.0,
        0.15,
    ),
    stony: mixa(
        &[("overgrown_stones", 3), ("grassy_stones", 3), ("grass", 2)],
        6.0,
        &[("mossy_stones", 2), ("small_stones", 1)],
        4.0,
        0.2,
    ),
    scree: mix(
        &[("@debris", 3), ("small_stones", 3), ("mossy_stones", 2)],
        5.0,
    ),
    shore: mixa(
        &[
            ("wet_sand_veg", 3),
            ("sand_veg", 2),
            ("mossy_silt", 2),
            ("overgrown_silt", 1),
        ],
        6.0,
        &[("mud", 1), ("pebbles", 1)],
        3.0,
        0.15,
    ),
    beds: LAKE_BEDS,
    topsoil: "dirt",
    subsoil: "brown_earth",
    soil_depth: 4.0,
    stony_slope: 0.55,
    scree_slope: 1.0,
    rock_slope: (1.15, 1.8),
    snow: false,
    mossy_rock: false,
};

const MEADOW: Recipe = Recipe {
    open: mixa(
        &[("grass", 5), ("clover", 5)],
        12.0,
        &[("overgrown_silt", 2), ("grassy_dirt", 2)],
        5.0,
        0.08,
    ),
    ..PLAINS
};

const FOREST: Recipe = Recipe {
    open: mixa(
        &[("grass", 6), ("clover", 2), ("overgrown_fir_floor", 2)],
        10.0,
        &[("grassy_dirt", 2), ("mossy_fir_floor", 2)],
        4.0,
        0.15,
    ),
    wood: mixa(
        &[
            ("mossy_fir_floor", 4),
            ("overgrown_fir_floor", 3),
            ("autumn_floor", 3),
            ("fir_floor", 1),
            ("grass", 1),
        ],
        8.0,
        &[
            ("mossy_roots", 3),
            ("autumn_roots", 2),
            ("shredded_leaves", 2),
            ("green_sphagnum", 2),
            ("coarse_dirt", 1),
            ("muddy_floor", 1),
        ],
        4.0,
        0.22,
    ),
    wet: mixa(
        &[
            ("mossy_fir_floor", 2),
            ("green_sphagnum", 2),
            ("muddy_floor", 2),
        ],
        6.0,
        &[("mud", 1), ("light_sphagnum", 1), ("mossy_soil", 1)],
        3.0,
        0.2,
    ),
    stony: mixa(
        &[
            ("mossy_stones", 3),
            ("overgrown_stones", 2),
            ("mossy_fir_floor", 2),
        ],
        6.0,
        &[("mossy_roots", 1), ("small_stones", 1)],
        3.0,
        0.15,
    ),
    scree: mix(
        &[("@debris", 3), ("mossy_stones", 2), ("small_stones", 2)],
        5.0,
    ),
    shore: mixa(
        &[("muddy_floor", 2), ("wet_sand_veg", 2), ("mossy_silt", 2)],
        5.0,
        &[("mud", 1), ("pebbles", 1)],
        3.0,
        0.2,
    ),
    subsoil: "rocky_soil",
    mossy_rock: true,
    ..PLAINS
};

const BIRCH_FOREST: Recipe = Recipe {
    wood: mixa(
        &[
            ("overgrown_fir_floor", 4),
            ("grass", 3),
            ("shredded_leaves", 2),
            ("mossy_fir_floor", 2),
        ],
        9.0,
        &[
            ("autumn_floor", 2),
            ("light_sphagnum", 1),
            ("mossy_roots", 1),
        ],
        4.0,
        0.15,
    ),
    ..FOREST
};

const TAIGA: Recipe = Recipe {
    open: mixa(
        &[("taiga_grass", 6), ("grass", 2), ("mossy_fir_floor", 2)],
        10.0,
        &[
            ("light_sphagnum", 2),
            ("lichen_dirt", 1),
            ("mossy_stones", 1),
        ],
        4.0,
        0.15,
    ),
    wood: mixa(
        &[
            ("taiga_floor", 4),
            ("fir_floor", 3),
            ("mossy_fir_floor", 3),
            ("podzol", 2),
        ],
        8.0,
        &[
            ("green_sphagnum", 3),
            ("light_sphagnum", 2),
            ("mossy_roots", 2),
            ("cushion_moss", 2),
        ],
        4.0,
        0.25,
    ),
    wet: mix(
        &[
            ("green_sphagnum", 3),
            ("light_sphagnum", 2),
            ("brown_sphagnum", 1),
            ("peat", 1),
        ],
        6.0,
    ),
    stony: mix(
        &[
            ("mossy_stones", 3),
            ("mossy_fir_floor", 2),
            ("lichen_dirt", 2),
        ],
        5.0,
    ),
    scree: mix(
        &[("@debris", 3), ("mossy_stones", 2), ("small_stones", 2)],
        5.0,
    ),
    shore: mix(
        &[
            ("mossy_silt", 2),
            ("wet_gray_sand", 2),
            ("peat", 1),
            ("wet_sand_gravel", 1),
        ],
        5.0,
    ),
    beds: STONY_BEDS,
    topsoil: "rocky_sandy_soil",
    subsoil: "rocky_soil",
    soil_depth: 3.0,
    mossy_rock: true,
    ..PLAINS
};

const PINE_FOREST: Recipe = Recipe {
    open: mixa(
        &[
            ("taiga_grass", 4),
            ("overgrown_sand", 2),
            ("coarse_sandy_soil", 2),
            ("heather_moss", 1),
        ],
        10.0,
        &[("lichen_dirt", 2), ("sandy_soil", 1)],
        4.0,
        0.15,
    ),
    wood: mixa(
        &[
            ("fir_floor", 4),
            ("taiga_floor", 2),
            ("coarse_sandy_soil", 2),
            ("mossy_fir_floor", 2),
        ],
        8.0,
        &[
            ("heather_moss", 2),
            ("lichen_dirt", 2),
            ("light_sphagnum", 1),
            ("sandy_soil", 1),
        ],
        4.0,
        0.2,
    ),
    wet: mix(
        &[("light_sphagnum", 2), ("mossy_silt", 1), ("peat", 1)],
        6.0,
    ),
    stony: mix(
        &[
            ("rocky_sandy_soil", 3),
            ("overgrown_rocky_sandy", 2),
            ("small_stones", 1),
        ],
        5.0,
    ),
    scree: mix(
        &[("@debris", 3), ("small_stones", 2), ("rocky_sandy_soil", 1)],
        5.0,
    ),
    shore: mix(
        &[("wet_tan_sand", 2), ("tan_sand", 2), ("sand_veg", 1)],
        5.0,
    ),
    beds: [
        mix(&[("wet_tan_sand", 3), ("wet_sand_gravel", 1)], 6.0),
        mix(&[("tan_sand", 2), ("sand_gravel", 2)], 8.0),
        mix(&[("sand_gravel", 2), ("gravel", 1)], 10.0),
    ],
    topsoil: "sandy_soil",
    subsoil: "coarse_sandy_soil",
    soil_depth: 3.0,
    mossy_rock: false,
    ..PLAINS
};

const SWAMP: Recipe = Recipe {
    open: mixa(
        &[
            ("mossy_silt", 3),
            ("grass", 2),
            ("overgrown_silt", 2),
            ("muddy_floor", 1),
        ],
        8.0,
        &[("mud", 2), ("green_sphagnum", 1)],
        4.0,
        0.2,
    ),
    wood: mixa(
        &[
            ("muddy_floor", 3),
            ("mossy_fir_floor", 2),
            ("mossy_soil", 2),
            ("mossy_silt", 2),
        ],
        7.0,
        &[
            ("mud", 2),
            ("green_sphagnum", 2),
            ("thick_mud", 1),
            ("mossy_roots", 2),
        ],
        4.0,
        0.25,
    ),
    wet: mixa(
        &[
            ("mud", 3),
            ("thick_mud", 2),
            ("mossy_soil", 2),
            ("green_sphagnum", 2),
            ("peat", 1),
        ],
        6.0,
        &[("clay_mud", 1), ("dark_silt", 1)],
        3.0,
        0.15,
    ),
    stony: mix(&[("mossy_stones", 2), ("mossy_soil", 2)], 5.0),
    scree: mix(&[("mossy_stones", 2), ("@debris", 1)], 5.0),
    shore: mix(
        &[
            ("mud", 3),
            ("thick_mud", 1),
            ("mossy_silt", 2),
            ("clay_mud", 1),
        ],
        5.0,
    ),
    beds: SWAMP_BEDS,
    topsoil: "mud",
    subsoil: "clay_mud",
    soil_depth: 4.0,
    mossy_rock: true,
    ..PLAINS
};

const MARSH: Recipe = Recipe {
    open: mixa(
        &[
            ("grass", 3),
            ("overgrown_silt", 3),
            ("mossy_silt", 2),
            ("green_sphagnum", 1),
        ],
        9.0,
        &[("peat", 2), ("mud", 1), ("brown_sphagnum", 1)],
        4.0,
        0.2,
    ),
    wood: mixa(
        &[("mossy_silt", 2), ("muddy_floor", 2), ("green_sphagnum", 1)],
        7.0,
        &[("mud", 1)],
        4.0,
        0.2,
    ),
    wet: mix(
        &[
            ("green_sphagnum", 3),
            ("light_sphagnum", 2),
            ("mud", 2),
            ("peat", 2),
            ("brown_sphagnum", 1),
        ],
        6.0,
    ),
    stony: mix(&[("mossy_stones", 2), ("overgrown_silt", 2)], 5.0),
    shore: mix(&[("mud", 3), ("wet_sand_veg", 1), ("mossy_silt", 2)], 5.0),
    beds: [
        mix(&[("mud", 2), ("wet_sand_veg", 1), ("dark_silt", 1)], 5.0),
        mix(&[("dark_silt", 2), ("mud", 1), ("clay", 1)], 7.0),
        mix(&[("dark_silt", 2), ("clay", 1)], 9.0),
    ],
    topsoil: "peat",
    subsoil: "mud",
    ..SWAMP
};

const HIGHLANDS: Recipe = Recipe {
    open: mixa(
        &[
            ("heather_moss", 4),
            ("taiga_grass", 3),
            ("grass", 2),
            ("brown_sphagnum", 1),
        ],
        16.0,
        &[
            ("lichen_dirt", 2),
            ("mossy_stones", 2),
            ("overgrown_stones", 2),
            ("yellow_sphagnum", 1),
            ("peat", 1),
        ],
        4.0,
        0.2,
    ),
    wood: mixa(
        &[
            ("mossy_fir_floor", 3),
            ("taiga_floor", 2),
            ("heather_moss", 2),
        ],
        7.0,
        &[("cushion_moss", 1), ("green_sphagnum", 1)],
        4.0,
        0.2,
    ),
    wet: mix(
        &[
            ("peat", 3),
            ("brown_sphagnum", 2),
            ("green_sphagnum", 2),
            ("yellow_sphagnum", 1),
            ("red_sphagnum", 1),
        ],
        6.0,
    ),
    stony: mix(
        &[
            ("mossy_stones", 3),
            ("overgrown_stones", 2),
            ("lichen_dirt", 2),
            ("heather_moss", 1),
        ],
        5.0,
    ),
    scree: mix(
        &[("@debris", 3), ("mossy_stones", 2), ("small_stones", 2)],
        5.0,
    ),
    shore: mix(
        &[
            ("peat", 2),
            ("wet_gray_sand", 2),
            ("mossy_silt", 1),
            ("pebbles", 1),
        ],
        5.0,
    ),
    beds: STONY_BEDS,
    topsoil: "peat",
    subsoil: "rocky_soil",
    soil_depth: 2.0,
    stony_slope: 0.45,
    scree_slope: 0.85,
    rock_slope: (0.9, 1.5),
    snow: false,
    mossy_rock: true,
};

const MOUNTAINS: Recipe = Recipe {
    open: mixa(
        &[
            ("taiga_grass", 3),
            ("overgrown_stones", 3),
            ("lichen_dirt", 2),
            ("grass", 1),
        ],
        9.0,
        &[("mossy_stones", 2), ("small_stones", 1)],
        4.0,
        0.2,
    ),
    wood: mix(
        &[
            ("taiga_floor", 2),
            ("mossy_fir_floor", 2),
            ("fir_floor", 2),
            ("lichen_dirt", 1),
        ],
        7.0,
    ),
    wet: mix(
        &[
            ("mossy_silt", 1),
            ("light_sphagnum", 1),
            ("overgrown_stones", 1),
        ],
        5.0,
    ),
    stony: mix(
        &[
            ("small_stones", 3),
            ("@debris", 3),
            ("mossy_stones", 2),
            ("overgrown_stones", 1),
        ],
        5.0,
    ),
    scree: mix(
        &[("@debris", 4), ("small_stones", 2), ("light_gravel", 1)],
        5.0,
    ),
    shore: mix(
        &[("pebbles", 2), ("gray_gravel", 2), ("wet_gray_sand", 1)],
        4.0,
    ),
    beds: STONY_BEDS,
    topsoil: "rocky_soil",
    subsoil: "rocky_soil",
    soil_depth: 1.5,
    stony_slope: 0.35,
    scree_slope: 0.6,
    rock_slope: (0.75, 1.25),
    snow: false,
    mossy_rock: false,
};

const ALPINE_MEADOW: Recipe = Recipe {
    open: mixa(
        &[("grass", 5), ("taiga_grass", 3), ("clover", 2)],
        11.0,
        &[
            ("overgrown_stones", 2),
            ("lichen_dirt", 1),
            ("mossy_stones", 1),
        ],
        4.0,
        0.12,
    ),
    wood: mix(
        &[
            ("taiga_floor", 2),
            ("mossy_fir_floor", 2),
            ("taiga_grass", 2),
        ],
        7.0,
    ),
    wet: mix(
        &[("green_sphagnum", 2), ("mossy_silt", 2), ("grass", 1)],
        6.0,
    ),
    stony: mix(
        &[
            ("overgrown_stones", 3),
            ("mossy_stones", 2),
            ("lichen_dirt", 1),
        ],
        5.0,
    ),
    scree: mix(&[("@debris", 3), ("small_stones", 2)], 5.0),
    soil_depth: 2.0,
    stony_slope: 0.45,
    scree_slope: 0.8,
    rock_slope: (0.95, 1.5),
    ..MOUNTAINS
};

const SNOWY_PEAKS: Recipe = Recipe {
    open: mixa(&[("snow", 1)], 20.0, &[("glacier_ice", 1)], 9.0, 0.03),
    wood: mix(&[("snow", 1)], 20.0),
    wet: mix(&[("snow", 1)], 20.0),
    stony: mix(&[("snow", 4), ("@debris", 1)], 6.0),
    scree: mix(&[("@debris", 2), ("snow", 2)], 5.0),
    shore: mix(&[("snow", 2), ("gray_gravel", 1)], 5.0),
    topsoil: "frozen_dirt",
    subsoil: "rocky_soil",
    soil_depth: 1.0,
    stony_slope: 0.6,
    scree_slope: 0.95,
    rock_slope: (1.05, 1.6),
    snow: true,
    ..MOUNTAINS
};

const BEACH: Recipe = Recipe {
    open: mixa(
        &[("sand", 5), ("sand_veg", 2), ("tan_sand", 1)],
        12.0,
        &[("sand_gravel", 2), ("pebbles", 1), ("mossy_sand", 1)],
        4.0,
        0.12,
    ),
    wood: mix(&[("sand_veg", 2), ("mossy_sand", 1), ("sand", 1)], 8.0),
    wet: mix(&[("wet_sand", 3), ("wet_sand_veg", 2)], 6.0),
    stony: mix(
        &[("sand_gravel", 2), ("pebbles", 2), ("small_stones", 1)],
        5.0,
    ),
    scree: mix(&[("@debris", 2), ("pebbles", 2)], 5.0),
    shore: mixa(
        &[("wet_sand", 4), ("wet_sand_gravel", 2)],
        6.0,
        &[("pebbles", 1)],
        3.0,
        0.1,
    ),
    beds: [
        mix(&[("wet_sand", 4), ("wet_sand_gravel", 1)], 7.0),
        mix(&[("sand", 3), ("sand_gravel", 1)], 9.0),
        mix(
            &[("sand_gravel", 2), ("light_gray_sand", 2), ("gravel", 1)],
            11.0,
        ),
    ],
    topsoil: "sand",
    subsoil: "sand_gravel",
    soil_depth: 4.0,
    ..PLAINS
};

const RIVER: Recipe = Recipe {
    open: mixa(
        &[("grass", 3), ("overgrown_silt", 2), ("mossy_silt", 1)],
        7.0,
        &[("overgrown_stones", 1)],
        3.0,
        0.1,
    ),
    wet: mix(&[("mossy_silt", 2), ("wet_river_sand", 2), ("mud", 1)], 5.0),
    stony: mix(
        &[("pebbles", 2), ("mossy_stones", 2), ("overgrown_stones", 2)],
        4.0,
    ),
    scree: mix(&[("pebbles", 2), ("@debris", 2)], 4.0),
    shore: mixa(
        &[
            ("wet_river_sand", 3),
            ("pebbles", 2),
            ("wet_sand_gravel", 2),
        ],
        4.0,
        &[("mossy_stones", 1)],
        3.0,
        0.12,
    ),
    beds: RIVER_BEDS,
    topsoil: "dirt",
    subsoil: "sand_gravel",
    soil_depth: 3.0,
    ..PLAINS
};

const LAKE: Recipe = Recipe {
    open: mix(&[("grass", 4), ("overgrown_silt", 2), ("clover", 1)], 8.0),
    wet: mix(&[("mossy_silt", 2), ("wet_sand_veg", 2), ("mud", 1)], 5.0),
    stony: mix(&[("pebbles", 2), ("overgrown_stones", 2)], 4.0),
    scree: mix(&[("pebbles", 2), ("@debris", 2)], 4.0),
    shore: mixa(
        &[("wet_sand_veg", 2), ("wet_sand", 2), ("wet_sand_gravel", 2)],
        5.0,
        &[("pebbles", 1), ("mossy_silt", 1)],
        3.0,
        0.15,
    ),
    beds: LAKE_BEDS,
    topsoil: "dirt",
    subsoil: "sand_gravel",
    soil_depth: 3.0,
    ..PLAINS
};

pub(crate) fn recipe(biome: Biome) -> &'static Recipe {
    match biome {
        Biome::Plains => &PLAINS,
        Biome::Meadow => &MEADOW,
        Biome::Forest => &FOREST,
        Biome::BirchForest => &BIRCH_FOREST,
        Biome::Taiga => &TAIGA,
        Biome::PineForest => &PINE_FOREST,
        Biome::Swamp => &SWAMP,
        Biome::Marsh => &MARSH,
        Biome::Highlands => &HIGHLANDS,
        Biome::Mountains => &MOUNTAINS,
        Biome::AlpineMeadow => &ALPINE_MEADOW,
        Biome::SnowyPeaks => &SNOWY_PEAKS,
        Biome::Beach => &BEACH,
        Biome::River => &RIVER,
        Biome::Lake => &LAKE,
    }
}

/// Water beds by kind of water, for water that differs from the column's biome.
pub(crate) fn river_beds() -> &'static [Mix; 3] {
    &RIVER.beds
}

pub(crate) fn swamp_beds() -> &'static [Mix; 3] {
    &SWAMP.beds
}

/// Every material key the recipes use, for checking them.
#[cfg(test)]
pub(crate) fn all_keys() -> impl Iterator<Item = &'static str> {
    Biome::ALL.iter().flat_map(|&b| {
        let r = recipe(b);
        [&r.open, &r.wood, &r.wet, &r.stony, &r.scree, &r.shore]
            .into_iter()
            .chain(r.beds.iter())
            .flat_map(|m| m.base.iter().chain(m.accent.iter()).map(|e| e.0))
            .chain([r.topsoil, r.subsoil])
            .collect::<Vec<_>>()
    })
}
