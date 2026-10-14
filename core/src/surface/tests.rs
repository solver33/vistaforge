//! Surface tests, plus a synthetic terrain with every biome that the decor tests share.
//!
//! Tests that need block ids load the Conquest catalog from `VISTAFORGE_CATALOG` (or the
//! project's copy) and skip when it isn't there, as the engine tests do.

use std::path::Path;

use super::*;
use crate::catalog::Catalog;
use crate::column::{ColumnGrid, Rect};
use crate::noise::fbm2;
use crate::sink::Palette;
use crate::world::Area;

pub(crate) fn catalog() -> Option<Catalog> {
    let path = std::env::var("VISTAFORGE_CATALOG")
        .unwrap_or_else(|_| "/mnt/project-files/vistaforge/catalog-cr-1.7.0.json".into());
    Catalog::load(Path::new(&path)).ok()
}

/// Side of a biome region in the synthetic terrain.
pub(crate) const REGION: i32 = 96;
pub(crate) const SEA: i32 = 63;

/// The biome of a region of the synthetic terrain: every biome, five regions across.
pub(crate) fn region_biome(rx: i32, rz: i32) -> Biome {
    let n = Biome::ALL.len() as i32;
    Biome::ALL[(rx.rem_euclid(5) + rz.rem_euclid(3) * 5).rem_euclid(n) as usize]
}

fn biome_height(b: Biome, seed: u64, x: f64, z: f64) -> f64 {
    let n = |s: u64, scale: f64, oct: u32| fbm2(seed ^ s, x / scale, z / scale, oct);
    match b.fallback() {
        Biome::Plains | Biome::Meadow => 66.5 + 3.0 * n(1, 60.0, 3) + 0.8 * n(2, 12.0, 2),
        Biome::Forest | Biome::BirchForest | Biome::Taiga => {
            68.0 + 5.0 * n(1, 70.0, 3) + 1.0 * n(2, 15.0, 2)
        }
        Biome::PineForest => 70.0 + 6.0 * n(1, 50.0, 3) + 1.2 * n(2, 12.0, 2),
        Biome::Swamp => 63.6 + 1.1 * n(3, 22.0, 3),
        Biome::Marsh => 63.7 + 0.9 * n(3, 18.0, 3),
        Biome::Highlands => 76.0 + 9.0 * n(4, 55.0, 4),
        Biome::Mountains => 92.0 + 30.0 * n(5, 45.0, 4).abs() + 6.0 * n(6, 14.0, 2),
        Biome::AlpineMeadow => 96.0 + 8.0 * n(7, 50.0, 3),
        Biome::SnowyPeaks => 112.0 + 22.0 * n(8, 40.0, 4),
        Biome::Beach => 62.0 + 2.5 * n(9, 40.0, 2) + 0.5 * n(2, 8.0, 2),
        Biome::River => {
            let wave = (z.rem_euclid(REGION as f64) - 48.0 + 10.0 * (x / 23.0).sin()).abs();
            let bank = 66.0 + 2.0 * n(1, 40.0, 3);
            if wave < 9.0 {
                bank - 6.0 * (1.0 - wave / 9.0).powf(0.6)
            } else {
                bank
            }
        }
        Biome::Lake => {
            let (lx, lz) = (
                x.rem_euclid(REGION as f64) - 48.0,
                z.rem_euclid(REGION as f64) - 48.0,
            );
            let d = (lx * lx + lz * lz).sqrt() / 36.0;
            66.0 + 1.5 * n(1, 30.0, 2) - 9.0 * (1.0 - d.min(1.0)).powf(0.7)
        }
        Biome::Tropical | Biome::SnowyTaiga | Biome::Tundra | Biome::Volcanic => {
            unreachable!("fallback() returns one of the original biomes")
        }
    }
}

fn moisture_of(b: Biome) -> f32 {
    match b {
        Biome::Swamp | Biome::Marsh => 0.85,
        Biome::Forest | Biome::Taiga | Biome::River | Biome::Lake => 0.6,
        Biome::PineForest | Biome::Beach | Biome::Mountains => 0.35,
        _ => 0.5,
    }
}

/// Region weights around a position: pure biome in the middle of a region, blending over the
/// outer 15 % on each side.
fn weights(x: f64, z: f64) -> [(Biome, f64); 4] {
    let r = REGION as f64;
    let (fx, fz) = (x / r - 0.5, z / r - 0.5);
    let (ix, iz) = (fx.floor(), fz.floor());
    let s = |t: f64| {
        let t = ((t - 0.35) / 0.3).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let (tx, tz) = (s(fx - ix), s(fz - iz));
    let (ix, iz) = (ix as i32, iz as i32);
    [
        (region_biome(ix, iz), (1.0 - tx) * (1.0 - tz)),
        (region_biome(ix + 1, iz), tx * (1.0 - tz)),
        (region_biome(ix, iz + 1), (1.0 - tx) * tz),
        (region_biome(ix + 1, iz + 1), tx * tz),
    ]
}

fn synth_height(seed: u64, x: f64, z: f64) -> f64 {
    weights(x, z)
        .iter()
        .map(|&(b, w)| {
            if w > 0.0 {
                w * biome_height(b, seed, x, z)
            } else {
                0.0
            }
        })
        .sum()
}

/// A column of the synthetic terrain: every biome in turn, with hills, mountains, lakes, a
/// river, swamp pools and a beach, blended at region borders.
pub(crate) fn synth_column(seed: u64, x: i32, z: i32) -> ColumnInfo {
    let (fx, fz) = (x as f64, z as f64);
    let h = synth_height(seed, fx, fz);
    let dx = synth_height(seed, fx + 1.0, fz) - synth_height(seed, fx - 1.0, fz);
    let dz = synth_height(seed, fx, fz + 1.0) - synth_height(seed, fx, fz - 1.0);
    let slope = ((dx * dx + dz * dz).sqrt() / 2.0) as f32;
    let mut w = weights(fx, fz);
    w.sort_by(|a, b| b.1.total_cmp(&a.1));
    // Merge equal biomes so the runner-up is a different one.
    let first = w[0].0;
    let total_first: f64 = w.iter().filter(|e| e.0 == first).map(|e| e.1).sum();
    let second = w.iter().find(|e| e.0 != first).copied();
    let wet = h < SEA as f64 + 0.9;
    let biome = first;
    let water = match biome {
        _ if !wet => WaterKind::None,
        Biome::River => WaterKind::River,
        Biome::Swamp | Biome::Marsh => WaterKind::Swamp,
        Biome::Beach => WaterKind::Sea,
        _ => WaterKind::Lake,
    };
    let moisture: f32 = w.iter().map(|e| e.1 as f32 * moisture_of(e.0)).sum::<f32>()
        + 0.15 * fbm2(seed ^ 0x40, fx / 30.0, fz / 30.0, 2) as f32;
    let mountain = matches!(biome, Biome::Mountains | Biome::SnowyPeaks);
    ColumnInfo {
        height: h as f32,
        water_level: wet.then_some(SEA),
        water,
        biome,
        biome2: second.map_or(biome, |s| s.0),
        blend: second.map_or(0.0, |s| (s.1 / (s.1 + total_first)) as f32),
        slope,
        moisture: moisture.clamp(0.0, 1.0),
        rock: if mountain {
            smoothstep(0.9, 1.7, slope)
        } else {
            0.0
        },
        flow: 0.0,
        fall: None,
        road: None,
    }
}

pub(crate) fn synth_grid(seed: u64, rect: Rect) -> ColumnGrid {
    ColumnGrid::build(rect, |x, z| synth_column(seed, x, z))
}

fn settings() -> WorldSettings {
    WorldSettings {
        size: 512,
        seed: 11,
        ..WorldSettings::default()
    }
}

#[test]
fn every_ground_material_is_in_the_catalog() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let b = Blocks::new(&catalog, &palette);
    let s = Surface::new(&b, &settings()).unwrap();
    assert!(s.kit.missing.is_empty(), "missing: {:?}", s.kit.missing);
    // Every key a recipe names exists (an unknown key panics).
    for key in recipes::all_keys() {
        if key != "@debris" {
            s.kit.get(key);
        }
    }
}

#[test]
fn columns_are_solid_up_to_the_ground_and_water_up_to_its_level() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let b = Blocks::new(&catalog, &palette);
    let s = Surface::new(&b, &settings()).unwrap();
    let water = b.id("minecraft:water", &[("level", "0")]).unwrap();
    let rect = Rect::new(0, 0, REGION * 5, REGION * 3);
    let grid = synth_grid(11, rect);
    let mut area = Area::new(rect, &palette);
    let mut wet = 0;
    for (x, z) in rect.columns() {
        s.fill(x, z, &grid, &mut area);
        let col = grid.get(x, z);
        let (y, layers) = col.ground();
        let column = area.column(x, z);
        let at = |y: i32| column[(y - MIN_Y) as usize];
        for yy in MIN_Y..y {
            assert_ne!(at(yy), AIR, "hole at {x} {yy} {z}");
            assert_ne!(at(yy), water, "water in the ground at {x} {yy} {z}");
        }
        if layers == 0 {
            assert!(at(y) == AIR || at(y) == water, "{x} {y} {z}");
        }
        if let Some(w) = col.water_level {
            wet += 1;
            // The top water cell is water, or a waterlogged layer of ground just under the
            // surface.
            if y < w {
                assert_eq!(at(w), water, "{x} {w} {z}");
            }
            assert_eq!(at(w + 1), AIR);
        } else {
            assert_eq!(at(y + 1), AIR);
        }
    }
    assert!(wet > 1000, "{wet}");
}

#[test]
fn ground_varies_within_every_biome() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let b = Blocks::new(&catalog, &palette);
    let s = Surface::new(&b, &settings()).unwrap();
    for rz in 0..3 {
        for rx in 0..5 {
            let biome = region_biome(rx, rz);
            let rect = Rect::new(
                rx * REGION + 20,
                rz * REGION + 20,
                rx * REGION + 76,
                rz * REGION + 76,
            );
            let grid = synth_grid(11, rect);
            let seen: std::collections::HashSet<_> = rect
                .columns()
                .filter_map(|(x, z)| s.top_at(x, z, &grid).map(|t| t.mat))
                .collect();
            assert!(
                seen.len() >= 3,
                "{biome:?} shows only {} materials",
                seen.len()
            );
        }
    }
}

#[test]
fn water_gets_a_light_biome() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let b = Blocks::new(&catalog, &palette);
    let s = Surface::new(&b, &settings()).unwrap();
    let mut lake = ColumnInfo::dry(60.5, Biome::Lake);
    lake.water_level = Some(63);
    lake.water = WaterKind::Lake;
    assert_eq!(s.minecraft_biome(&lake), "minecraft:lukewarm_ocean");
    lake.height = 62.5;
    assert_eq!(s.minecraft_biome(&lake), "minecraft:warm_ocean");
    lake.water = WaterKind::Swamp;
    lake.biome = Biome::Swamp;
    assert_eq!(s.minecraft_biome(&lake), "minecraft:warm_ocean");
    // Above the water, trees and reeds keep the land's colours.
    assert_eq!(s.minecraft_biome_at(&lake, 60), "minecraft:warm_ocean");
    assert_eq!(s.minecraft_biome_at(&lake, 64), "minecraft:swamp");
    let land = ColumnInfo::dry(70.0, Biome::Forest);
    assert_eq!(s.minecraft_biome(&land), "minecraft:forest");
    // Water in the preview is clearly blue, not black.
    let deep = ColumnInfo {
        height: 60.5,
        water: WaterKind::Lake,
        ..lake
    };
    let c = s.preview_color(5, 5, &deep);
    assert!(
        c[2] as u32 > 90 && (c[0] as u32 + c[1] as u32 + c[2] as u32) > 200,
        "{c:?}"
    );
}

/// Rivers through swamps and marshes run over mud and silt, and every river's banks are soft
/// ground rather than a band of stones, however strongly the terrain marks them with flow.
#[test]
fn river_banks_blend_into_the_land() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let b = Blocks::new(&catalog, &palette);
    let s = Surface::new(&b, &settings()).unwrap();
    let kind = |t: &Top| s.kit.mat(t.mat).kind;
    for land in [Biome::Swamp, Biome::Marsh] {
        let mut river = ColumnInfo::dry(61.5, Biome::River);
        river.biome2 = land;
        river.water_level = Some(63);
        river.water = WaterKind::River;
        river.flow = 1.0;
        for (x, z) in Rect::new(0, 0, 40, 40).columns() {
            let k = kind(&s.top(x, z, &river, None, None));
            assert!(matches!(k, Kind::Mud | Kind::Moss), "{land:?} bed {k:?}");
        }
    }
    let near = Near {
        level: 63,
        dist: 1.0,
        river: true,
    };
    let (mut stones, mut total) = (0, 0);
    for land in [Biome::Plains, Biome::Forest, Biome::Swamp] {
        let mut bank = ColumnInfo::dry(63.5, land);
        bank.biome2 = Biome::River;
        bank.blend = 0.4;
        bank.flow = 0.85;
        bank.moisture = 0.7;
        for (x, z) in Rect::new(0, 0, 40, 40).columns() {
            for near in [Some(near), Some(Near { dist: 3.5, ..near }), None] {
                let t = s.top(x, z, &bank, near, None);
                assert!(
                    !matches!(t.zone, Zone::Scree | Zone::Stony),
                    "{land:?} {x} {z}"
                );
                total += 1;
                if kind(&t) == Kind::Gravel {
                    stones += 1;
                }
            }
        }
    }
    assert!(
        stones * 10 < total,
        "{stones} of {total} bank columns are stones"
    );
}
