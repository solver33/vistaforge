//! Geology: which rocks a region is made of. Regions a few hundred blocks across each get a rock
//! suite; inside it, rocks alternate in tilted, wavy strata a few blocks thick, so cliffs,
//! ravines and steep slopes show banded rock. The beds run on across region borders: there the
//! two suites' beds interfinger over a wide band, each bed ending at its own ragged line, so a
//! mountain never changes rock along one seam. Scree, boulders and loose rocks use the same
//! suites, so they match the cliffs they fell from.

use crate::noise::{fbm2, hash2, unit2};

use super::field::{hash_unit, patch, patches_near, pick, salt, smoothstep};
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
    // Only under volcanic land (see [`VOLCANIC`]), never picked for a region.
    Suite {
        name: "volcanic",
        rocks: &[
            ("weathered_basalt", 4),
            ("porous_basalt", 3),
            ("weathered_andesite", 2),
            ("dark_tuff", 2),
            ("porous_andesite", 1),
            ("tuff", 1),
        ],
        band: 2.5,
        mossy: &[],
        debris: "dark_gravel",
        rubble: &["conquest:dark_tuff_rocks", "conquest:andesite_rocks"],
        boulder: &["weathered_basalt", "porous_basalt"],
        weight: 0,
    },
];

/// The suite under volcanic land: basalt and andesite flows with tuff between them.
pub(crate) const VOLCANIC: usize = SUITES.len() - 1;

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

/// Most suites whose beds show in one column (a corner where three regions meet, plus volcanic
/// rock).
const MAX_BEDS: usize = 4;

/// The strata of one column: which rock is at each height.
#[derive(Clone, Copy)]
pub(crate) struct Strata {
    /// The suite the column's loose rock, debris and boulders come from: near a border, one of
    /// the suites meeting there, picked in patches.
    pub suite: usize,
    warp: f64,
    /// The suites whose beds show in the column, with how strongly (1 inside their region,
    /// fading to 0 a little way past its border).
    beds: [(usize, f32); MAX_BEDS],
    count: usize,
}

/// Size of a geological region in blocks.
const REGION: f64 = 700.0;

/// How far past a region's border its beds still reach, in warped blocks. Near a border the
/// beds of the two regions interfinger, each bed ending at its own place, so a cliff never
/// changes rock along one seam.
const INTERFINGER: f64 = 150.0;

/// How far, in blocks, the reach of a region's beds wanders in and out along its border.
const WANDER: f64 = 60.0;

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
                .filter(|(_, s)| s.weight > 0)
                .map(|(i, s)| (i, s.weight))
                .collect(),
        }
    }

    /// The strata of a column. `volcanic` is how much of the column's land is volcanic (0..1):
    /// volcanic rock takes over from the region's rock bed by bed as it rises towards 1.
    pub fn strata(&self, x: i32, z: i32, volcanic: f32) -> Strata {
        let (fx, fz) = (x as f64, z as f64);
        let s = self.seed ^ salt::GEOLOGY;
        // Gently tilted, wavy beds, the same in every suite so that beds run on across borders.
        let warp = 7.0 * fbm2(s ^ 1, fx / 140.0, fz / 140.0, 3)
            + 1.5 * fbm2(s ^ 2, fx / 17.0, fz / 17.0, 2)
            + 0.02 * fx
            - 0.013 * fz;

        let mut beds = [(0usize, 0.0f32); MAX_BEDS];
        let mut count = 0;
        let mut add = |suite: usize, w: f32| {
            if w <= 0.0 {
                return;
            }
            match beds[..count].iter_mut().find(|b| b.0 == suite) {
                Some(b) => b.1 = b.1.max(w),
                None if count < MAX_BEDS => {
                    beds[count] = (suite, w);
                    count += 1;
                }
                None => {}
            }
        };
        let volcanic = volcanic.clamp(0.0, 1.0);
        let land = (2.0 * (1.0 - volcanic)).min(1.0);
        if land > 0.0 {
            let near: Vec<_> = patches_near(self.seed, salt::GEOLOGY, x, z, REGION).collect();
            let nearest = near.iter().map(|p| p.0).fold(f64::MAX, f64::min);
            let mut regions: Vec<_> = near
                .into_iter()
                .filter(|&(d, _)| d - nearest < INTERFINGER + WANDER)
                .map(|(d, h)| {
                    // Each region reaches further in some places than others, so where a bed
                    // runs flat over a wide area it ends along a ragged line, not a straight one.
                    let reach = WANDER * fbm2(h ^ s, fx / 50.0, fz / 50.0, 3)
                        + 3.0 * (unit2(h ^ s, x as i64, z as i64) - 0.5);
                    let past = ((d - nearest + reach) as f32).max(0.0);
                    let w = 1.0 - smoothstep(0.0, INTERFINGER as f32, past);
                    (pick(&self.weights, hash_unit(h)), w)
                })
                .filter(|r| r.1 > 0.0)
                .collect();
            regions.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (suite, w) in regions {
                add(suite, w * land);
            }
        }
        add(VOLCANIC, (2.0 * volcanic).min(1.0));

        // Loose rock comes from the suites in patches, in proportion to how strongly each shows.
        let total: f32 = beds[..count].iter().map(|b| b.1).sum();
        let mut u = hash_unit(patch(self.seed, salt::GEOLOGY ^ 0x1005E, x, z, 9.0)) * total;
        let mut suite = beds[0].0;
        for &(sv, w) in &beds[..count] {
            suite = sv;
            if u < w {
                break;
            }
            u -= w;
        }
        Strata {
            suite,
            warp,
            beds,
            count,
        }
    }

    fn salt(&self, suite: usize) -> u64 {
        self.seed ^ salt::GEOLOGY ^ ((suite as u64) << 8)
    }

    /// The rock at height `y` of a column. Where several suites show, each bed belongs to one
    /// of them: the stronger a suite shows, the more of its beds win.
    pub fn rock(&self, st: &Strata, y: i32) -> Mat {
        let band = |suite: usize| ((y as f64 + st.warp) / self.suites[suite].band).floor() as i64;
        let mut best = (st.beds[0].0, band(st.beds[0].0));
        if st.count > 1 {
            let mut top = f32::MIN;
            for &(suite, w) in &st.beds[..st.count] {
                let b = band(suite);
                let score = w * hash_unit(hash2(self.salt(suite), b, 0xBED5));
                if score > top {
                    top = score;
                    best = (suite, b);
                }
            }
        }
        let (suite, b) = best;
        pick(
            &self.suites[suite].rocks,
            hash_unit(hash2(self.salt(suite), b, 0x5747)),
        )
    }

    /// The moss-grown version of a rock, if its suite has one.
    pub fn mossy(&self, rock: Mat) -> Option<Mat> {
        self.suites
            .iter()
            .flat_map(|s| s.mossy.iter())
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
}
