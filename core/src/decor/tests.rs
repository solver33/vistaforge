//! Decor tests on the synthetic terrain with every biome (see `surface::tests`). They need the
//! Conquest catalog and skip without it.

use super::*;
use crate::catalog::Catalog;
use crate::column::ColumnGrid;
use crate::sink::Palette;
use crate::surface::tests::{catalog, region_biome, synth_column, synth_grid, REGION};
use crate::trees::Trees;
use crate::world::Area;

const MARGIN: i32 = 16;

struct World<'p> {
    surface: Surface,
    decor: Decor,
    trees: Trees,
    palette: &'p Palette,
    map: Rect,
}

impl<'p> World<'p> {
    fn new(catalog: &Catalog, palette: &'p Palette) -> Self {
        let b = Blocks::new(catalog, palette);
        let settings = WorldSettings {
            size: (REGION * 5) as u32,
            seed: 11,
            ..WorldSettings::default()
        };
        Self {
            surface: Surface::new(&b, &settings).unwrap(),
            decor: Decor::new(&b, &settings).unwrap(),
            trees: Trees::new(&b).unwrap(),
            palette,
            map: Rect::square(REGION * 5),
        }
    }

    /// A piece as the engine makes it: `rect` and its margin, with or without trees and decor.
    fn piece(&self, rect: Rect, trees: bool, decorate: bool) -> (Area<'p>, ColumnGrid) {
        let buffer = rect.expand(MARGIN);
        let grid = synth_grid(11, buffer);
        let mut area = Area::new(buffer, self.palette);
        for (x, z) in buffer.columns() {
            self.surface.fill(x, z, &grid, &mut area);
        }
        let column = |x: i32, z: i32| match grid.try_get(x, z) {
            Some(c) => *c,
            None => synth_column(11, x, z),
        };
        if trees {
            // In the engine's order: overlapping crowns resolve the same way everywhere.
            let mut planned = self.decor.plan_trees(buffer.intersect(&self.map), &column);
            planned.sort_by_key(|t| (t.x, t.z, t.seed));
            for tree in &planned {
                self.trees.build(tree, &mut area);
            }
        }
        if decorate {
            self.decor.decorate(buffer, &grid, &mut area);
        }
        (area, grid)
    }
}

#[test]
fn every_plant_is_in_the_catalog() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    assert!(
        w.decor.flora.missing.is_empty(),
        "{:?}",
        w.decor.flora.missing
    );
    assert_eq!(w.decor.litter.len(), LITTER.len());
    assert!(w.decor.rubble.iter().all(|r| !r.is_empty()));
}

/// Overlapping pieces agree on every block in every biome: trees, plants, rocks, logs and
/// boulders come out the same whichever piece builds them.
#[test]
fn pieces_agree_in_every_biome() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    for rz in 0..3 {
        for rx in 0..5 {
            let (x0, z0) = (rx * REGION + 10, rz * REGION + 10);
            let a = Rect::new(x0, z0, x0 + 64, z0 + 64);
            let b = Rect::new(x0 + 23, z0 + 37, x0 + 87, z0 + 101);
            let (pa, _) = w.piece(a, true, true);
            let (pb, _) = w.piece(b, true, true);
            let shared = a.intersect(&b);
            for (x, z) in shared.columns() {
                assert_eq!(
                    pa.column(x, z),
                    pb.column(x, z),
                    "{:?} differs at {x} {z}",
                    region_biome(rx, rz)
                );
            }
        }
    }
}

/// Every land biome gets ground detail, and the wooded ones get trees, logs and litter.
#[test]
fn every_biome_is_detailed() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    for rz in 0..3 {
        for rx in 0..5 {
            let biome = region_biome(rx, rz);
            let (x0, z0) = (rx * REGION + 16, rz * REGION + 16);
            let rect = Rect::new(x0, z0, x0 + 64, z0 + 64);
            let (bare, grid) = w.piece(rect, true, false);
            let (full, _) = w.piece(rect, true, true);
            let mut changed = 0;
            let mut names = std::collections::HashMap::<String, usize>::new();
            let (mut dry, mut open) = (0, 0);
            for (x, z) in rect.columns() {
                let (a, b) = (bare.column(x, z), full.column(x, z));
                let col = grid.get(x, z);
                if !col.is_wet() {
                    dry += 1;
                    // Trunks and low branches take some of the floor; decor goes on the rest.
                    let (y, _) = col.ground();
                    if a[(y + 1 - crate::world::MIN_Y) as usize] == crate::sink::AIR {
                        open += 1;
                    }
                }
                for (&p, &q) in a.iter().zip(b) {
                    if p != q {
                        changed += 1;
                        *names.entry(palette.state(q).name.clone()).or_default() += 1;
                    }
                }
            }
            let per_column = changed as f32 / dry.max(1) as f32;
            let mut top: Vec<_> = names.iter().collect();
            top.sort_by(|a, b| b.1.cmp(a.1));
            let top: Vec<String> = top
                .iter()
                .take(12)
                .map(|(n, c)| format!("{}×{c}", n.trim_start_matches("conquest:")))
                .collect();
            eprintln!(
                "{biome:?}: {changed} decor blocks over {dry} dry columns ({open} open), {} kinds: {}",
                names.len(),
                top.join(" ")
            );
            if dry < 800 {
                continue; // mostly water
            }
            match biome {
                Biome::SnowyPeaks => assert!(changed > 0, "{biome:?}"),
                Biome::Beach => assert!(per_column > 0.05, "{biome:?}: {per_column}"),
                Biome::Mountains => assert!(per_column > 0.15, "{biome:?}: {per_column}"),
                _ => {
                    assert!(per_column > 0.3, "{biome:?}: {per_column}");
                    assert!(names.len() >= 8, "{biome:?}: {names:?}");
                }
            }
        }
    }
}

#[test]
fn trees_follow_the_biome() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    let count = |rx: i32, rz: i32| {
        let rect = Rect::new(
            rx * REGION + 16,
            rz * REGION + 16,
            rx * REGION + 80,
            rz * REGION + 80,
        );
        let column = |x: i32, z: i32| synth_column(11, x, z);
        w.decor.plan_trees(rect, &column)
    };
    let find = |b: Biome| {
        (0..15)
            .map(|i| (i % 5, i / 5))
            .find(|&(x, z)| region_biome(x, z) == b)
            .unwrap()
    };
    let forest = count(find(Biome::Forest).0, find(Biome::Forest).1);
    let plains = count(find(Biome::Plains).0, find(Biome::Plains).1);
    let peaks = count(find(Biome::SnowyPeaks).0, find(Biome::SnowyPeaks).1);
    let taiga = count(find(Biome::Taiga).0, find(Biome::Taiga).1);
    assert!(
        forest.len() > 3 * plains.len().max(1),
        "{} vs {}",
        forest.len(),
        plains.len()
    );
    assert!(peaks.is_empty(), "{}", peaks.len());
    assert!(
        taiga
            .iter()
            .filter(|t| t.species == crate::trees::Species::Spruce)
            .count()
            > taiga.len() / 2
    );
}
