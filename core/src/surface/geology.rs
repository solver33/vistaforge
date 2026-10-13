//! Geology: which rocks a region is made of. Regions a few hundred blocks across each get a rock
//! suite; inside it, rocks alternate in tilted, wavy strata a few blocks thick, so cliffs,
//! ravines and steep slopes show banded rock. Scree, boulders and loose rocks use the same suite,
//! so they match the cliffs they fell from.

use crate::noise::{fbm2, hash2};

use super::field::{hash_unit, patch, pick, salt};
use super::materials::{Kit, Mat};

pub(crate) struct Suite {
    #[allow(dead_code)]
    pub name: &'static str,
    /// Rocks of the strata, by weight.
    pub rocks: &'static [(&'static str, u32)],
    /// Typical stratum thickness in blocks.
    pub band: f64,
    /// Moss-grown versions of some rocks, for damp, shaded places.
    pub mossy: &'static [(&'static str, &'static str)],
    /// The loose gravel or debris the rock weathers into.
    pub debris: &'static str,
    /// Conquest `Rocks` scatter blocks matching the suite.
    pub rubble: &'static [&'static str],
    /// Rocks boulders are made of.
    pub boulder: &'static [&'static str],
    /// Region weight.
    pub weight: u32,
}

pub(crate) const SUITES: &[Suite] = &[
    Suite {
        name: "granite",
        rocks: &[
            ("weathered_granite", 5),
            ("light_gray_granite", 3),
            ("dark_gray_granite", 2),
            ("white_granite", 1),
        ],
        band: 5.0,
        mossy: &[],
        debris: "light_gravel",
        rubble: &["conquest:granite_rocks"],
        boulder: &["weathered_granite", "light_gray_granite"],
        weight: 3,
    },
    Suite {
        name: "gneiss",
        rocks: &[
            ("gneiss", 4),
            ("eroded_gneiss", 3),
            ("green_schist", 2),
            ("blue_schist", 2),
            ("weathered_blue_schist", 1),
        ],
        band: 3.0,
        mossy: &[
            ("blue_schist", "mossy_blue_schist"),
            ("weathered_blue_schist", "lichen_blue_schist"),
        ],
        debris: "schist_debris",
        rubble: &["conquest:blue_schist_rocks", "conquest:andesite_rocks"],
        boulder: &["gneiss", "eroded_gneiss"],
        weight: 3,
    },
    Suite {
        name: "slate",
        rocks: &[
            ("slate", 4),
            ("shale", 3),
            ("graywacke", 3),
            ("warped_slate", 1),
        ],
        band: 2.0,
        mossy: &[],
        debris: "gray_gravel",
        rubble: &["conquest:slate_rocks", "conquest:graywacke_rocks"],
        boulder: &["graywacke", "slate"],
        weight: 2,
    },
    Suite {
        name: "limestone",
        rocks: &[
            ("gray_limestone", 4),
            ("pale_limestone", 3),
            ("dark_limestone", 2),
            ("tan_limestone", 1),
            ("light_limestone_boulders", 1),
        ],
        band: 4.0,
        mossy: &[
            ("gray_limestone", "mossy_limestone"),
            ("pale_limestone", "moss_covered_limestone"),
            ("light_limestone_boulders", "mossy_light_limestone_boulders"),
        ],
        debris: "limestone_debris",
        rubble: &[
            "conquest:limestone_rocks",
            "conquest:pale_limestone_rocks",
            "conquest:light_limestone_rocks",
        ],
        boulder: &[
            "light_limestone_boulders",
            "gray_limestone",
            "pale_limestone",
        ],
        weight: 3,
    },
    Suite {
        name: "sandstone",
        rocks: &[
            ("tan_sandstone", 4),
            ("faded_tan_sandstone", 3),
            ("brown_sandstone", 2),
            ("faded_gray_sandstone", 2),
            ("orange_sandstone", 1),
        ],
        band: 3.0,
        mossy: &[
            ("faded_tan_sandstone", "mossy_faded_tan_sandstone"),
            ("tan_sandstone", "mossy_faded_tan_sandstone"),
        ],
        debris: "sandstone_debris",
        rubble: &["conquest:sandstone_rocks"],
        boulder: &["tan_sandstone", "faded_tan_sandstone"],
        weight: 2,
    },
];

struct Resolved {
    rocks: Vec<(Mat, u32)>,
    band: f64,
    mossy: Vec<(Mat, Mat)>,
    debris: Mat,
    boulder: Vec<(Mat, u32)>,
}

pub(crate) struct Geology {
    seed: u64,
    suites: Vec<Resolved>,
    weights: Vec<(usize, u32)>,
}

/// The strata of one column: which rock is at each height.
#[derive(Clone, Copy)]
pub(crate) struct Strata {
    pub suite: usize,
    warp: f64,
    salt: u64,
}

/// Size of a geological region in blocks.
const REGION: f64 = 700.0;

impl Geology {
    pub fn new(seed: u64, kit: &Kit) -> Self {
        let suites = SUITES
            .iter()
            .map(|s| Resolved {
                rocks: s.rocks.iter().map(|&(k, w)| (kit.get(k), w)).collect(),
                band: s.band,
                mossy: s
                    .mossy
                    .iter()
                    .map(|&(a, b)| (kit.get(a), kit.get(b)))
                    .collect(),
                debris: kit.get(s.debris),
                boulder: s.boulder.iter().map(|&k| (kit.get(k), 1)).collect(),
            })
            .collect();
        Self {
            seed,
            suites,
            weights: SUITES
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.weight))
                .collect(),
        }
    }

    pub fn suite_at(&self, x: i32, z: i32) -> usize {
        let h = patch(self.seed, salt::GEOLOGY, x, z, REGION);
        pick(&self.weights, hash_unit(h))
    }

    pub fn strata(&self, x: i32, z: i32) -> Strata {
        let (fx, fz) = (x as f64, z as f64);
        let s = self.seed ^ salt::GEOLOGY;
        // Gently tilted, wavy beds.
        let warp = 7.0 * fbm2(s ^ 1, fx / 140.0, fz / 140.0, 3)
            + 1.5 * fbm2(s ^ 2, fx / 17.0, fz / 17.0, 2)
            + 0.02 * fx
            - 0.013 * fz;
        let suite = self.suite_at(x, z);
        Strata {
            suite,
            warp,
            salt: s ^ ((suite as u64) << 8),
        }
    }

    /// The rock at height `y` of a column.
    pub fn rock(&self, st: &Strata, y: i32) -> Mat {
        let s = &self.suites[st.suite];
        let band = ((y as f64 + st.warp) / s.band).floor() as i64;
        pick(&s.rocks, hash_unit(hash2(st.salt, band, 0x5747)))
    }

    /// The moss-grown version of a rock, if the suite has one.
    pub fn mossy(&self, st: &Strata, rock: Mat) -> Option<Mat> {
        self.suites[st.suite]
            .mossy
            .iter()
            .find(|(a, _)| *a == rock)
            .map(|(_, b)| *b)
    }

    pub fn debris(&self, suite: usize) -> Mat {
        self.suites[suite].debris
    }

    /// A rock for a boulder of the suite, for a uniform `u`.
    pub fn boulder(&self, suite: usize, u: f32) -> Mat {
        pick(&self.suites[suite].boulder, u)
    }

    /// The moss-grown version of a rock in any suite.
    pub fn mossy_any(&self, rock: Mat) -> Option<Mat> {
        self.suites
            .iter()
            .flat_map(|s| s.mossy.iter())
            .find(|(a, _)| *a == rock)
            .map(|(_, b)| *b)
    }
}
