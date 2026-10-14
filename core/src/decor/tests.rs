//! Decor tests on the synthetic terrain with every biome (see `surface::tests`). They need the
//! Conquest catalog and skip without it.

use super::*;
use crate::catalog::Catalog;
use crate::column::ColumnGrid;
use crate::sink::Palette;
use crate::surface::tests::{catalog, region_biome, synth_column, REGION, ROWS};
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
        self.piece_of(rect, |x, z| synth_column(11, x, z), trees, decorate)
    }

    /// A piece of any terrain.
    fn piece_of(
        &self,
        rect: Rect,
        terrain: impl Fn(i32, i32) -> ColumnInfo,
        trees: bool,
        decorate: bool,
    ) -> (Area<'p>, ColumnGrid) {
        let buffer = rect.expand(MARGIN);
        let grid = ColumnGrid::build(buffer, &terrain);
        let mut area = Area::new(buffer, self.palette);
        for (x, z) in buffer.columns() {
            self.surface.fill(x, z, &grid, &mut area);
        }
        let column = |x: i32, z: i32| match grid.try_get(x, z) {
            Some(c) => *c,
            None => terrain(x, z),
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
    for rz in 0..ROWS {
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
    for rz in 0..ROWS {
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
                Biome::SnowyPeaks | Biome::Volcanic => assert!(changed > 0, "{biome:?}"),
                // Mostly under snow: plants only between the drifts.
                Biome::Tundra | Biome::SnowyTaiga => {
                    assert!(per_column > 0.04, "{biome:?}: {per_column}");
                    assert!(names.len() >= 5, "{biome:?}: {names:?}");
                }
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
        (0..5 * ROWS)
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
    let volcanic = count(find(Biome::Volcanic).0, find(Biome::Volcanic).1);
    assert!(volcanic.is_empty(), "{}", volcanic.len());
    let tundra = count(find(Biome::Tundra).0, find(Biome::Tundra).1);
    assert!(
        tundra.len() * 10 < forest.len()
            && tundra
                .iter()
                .all(|t| t.species == crate::trees::Species::Bush),
        "{}",
        tundra.len()
    );
    let snowy = count(find(Biome::SnowyTaiga).0, find(Biome::SnowyTaiga).1);
    assert!(
        snowy
            .iter()
            .filter(|t| t.species == crate::trees::Species::Spruce)
            .count()
            > snowy.len() / 2
    );
    assert!(
        taiga
            .iter()
            .filter(|t| t.species == crate::trees::Species::Spruce)
            .count()
            > taiga.len() / 2
    );
}

/// Plants are built the way a player builds them by hand. A tall Conquest reed is one block whose
/// `height` draws its model up into empty cells, never another plant block. Stackable grasses
/// stack as real blocks that all keep `height` 1, sharing `random` and `layers` so the stack
/// reads as one plant; a stacked `height` 2 would draw a third, detached piece on top.
#[test]
fn tall_plants_are_built_as_by_hand() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    let class = |name: &str| catalog.class_of(name).unwrap_or_default().to_string();
    assert_eq!(class("conquest:cattail_reeds"), "PlantsTall4");
    assert_eq!(class("conquest:rushes"), "PlantsDenseStackableHeight2");
    let (mut tall, mut taller, mut stacked) = (0, 0, 0);
    for rz in 0..ROWS {
        for rx in 0..5 {
            let rect = Rect::new(
                rx * REGION + 16,
                rz * REGION + 16,
                rx * REGION + 80,
                rz * REGION + 80,
            );
            let (area, _) = w.piece(rect, true, true);
            for (x, z) in rect.columns() {
                let column = area.column(x, z);
                let at = |i: usize| palette.state(column.get(i).copied().unwrap_or(AIR));
                for i in 1..column.len() {
                    let state = at(i);
                    let c = class(&state.name);
                    if let Some(n) = c.strip_prefix("PlantsTall") {
                        tall += 1;
                        let n: usize = n.parse().unwrap();
                        let height: usize = state.get("height").unwrap().parse().unwrap();
                        assert!(height <= n, "{state:?} at {x} {z}");
                        taller += (height > 1) as usize;
                        for k in 1..height {
                            let above = at(i + k);
                            assert!(
                                above.name == "minecraft:air" || above.name == "minecraft:water",
                                "{state:?} at {x} {z} reaches into {above:?}"
                            );
                        }
                        assert!(!class(&at(i - 1).name).starts_with("PlantsTall"));
                    } else if c == "PlantsDenseStackableHeight2" {
                        assert_eq!(state.get("height"), Some("1"), "{state:?} at {x} {z}");
                        let below = at(i - 1);
                        if below.name == state.name {
                            stacked += 1;
                            assert_eq!(below.get("random"), state.get("random"));
                            assert_eq!(below.get("layers"), state.get("layers"));
                        }
                    }
                }
            }
        }
    }
    eprintln!("{tall} tall reeds, {taller} of them taller than a block; {stacked} stacked grasses");
    assert!(tall > 50 && taller > tall / 3, "{tall} {taller}");
    assert!(stacked > 50, "{stacked}");
}

/// A forest river that drops eight blocks, from water at Y 72 to a pool at Y 64, over a ledge
/// at x = 24. The curtain is two columns thick, and the pool under it is deeper.
fn waterfall(x: i32, z: i32) -> ColumnInfo {
    let up = x < 24;
    if (12..=17).contains(&z) {
        let (level, bed) = match x {
            _ if up => (72, 69.0),
            24..=25 => (64, 60.0),
            26..=29 => (64, 59.5),
            _ => (64, 61.0),
        };
        ColumnInfo {
            height: bed,
            water_level: Some(level),
            water: WaterKind::River,
            biome2: Biome::Forest,
            slope: 0.2,
            moisture: 0.8,
            flow: 1.0,
            fall: (24..=25).contains(&x).then_some(72),
            ..ColumnInfo::dry(bed, Biome::River)
        }
    } else {
        let d = if z < 12 { 12 - z } else { z - 17 } as f32;
        let h = if up { 73.0 } else { 65.5 } + 0.4 * d.min(6.0);
        ColumnInfo {
            biome2: Biome::River,
            blend: if d < 3.0 { 0.4 } else { 0.0 },
            slope: 0.4,
            moisture: 0.6,
            flow: (0.9 * (1.0 - d / 6.0)).max(0.0),
            ..ColumnInfo::dry(h, Biome::Forest)
        }
    }
}

/// A waterfall's curtain is plain falling water whose colour matches the pools, the pool under
/// it is scoured to dark stones, the spray keeps the banks wet and mossy, nothing white (white
/// water, steam, clouds) is put anywhere near it, and nothing grows in the churning water.
#[test]
fn waterfalls_are_plain_falling_water() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    let rect = Rect::new(0, 0, 48, 32);
    let (area, grid) = w.piece_of(rect, waterfall, true, true);
    let name = |x: i32, y: i32, z: i32| palette.state(area.get(x, y, z)).name.clone();

    // The curtain.
    for x in 24..=25 {
        for z in 12..=17 {
            for y in 65..=72 {
                let state = palette.state(area.get(x, y, z));
                assert_eq!(state.name, "minecraft:water", "{x} {y} {z}");
                assert_eq!(state.get("level"), Some("8"), "{x} {y} {z}");
            }
            assert_eq!(name(x, 73, z), "minecraft:air");
            let col = grid.get(x, z);
            assert!(w.surface.minecraft_biome_at(col, 68).ends_with("ocean"));
        }
    }

    // The ground around it.
    let kit = &w.surface.kit;
    let stones: Vec<_> = ["mossy_stones", "dark_gravel", "mossy_gravel", "pebbles"]
        .iter()
        .map(|k| kit.get(k))
        .collect();
    for z in 13..=16 {
        for x in 24..=27 {
            let top = w.surface.top_at(x, z, &grid).unwrap();
            assert!(stones.contains(&top.mat), "pool bed at {x} {z}");
        }
    }
    for x in 23..=27 {
        for z in [10, 11, 18, 19] {
            let top = w.surface.top_at(x, z, &grid).unwrap();
            assert!(
                matches!(top.zone, Zone::Wet | Zone::Rock | Zone::Wood),
                "{:?} bank at {x} {z}",
                top.zone
            );
        }
    }

    // Nothing white, and no plants in the pool.
    for (x, z) in rect.columns() {
        for y in 55..85 {
            let n = name(x, y, z);
            assert!(
                !["waterfall", "steam", "cloud", "foam"]
                    .iter()
                    .any(|w| n.contains(w)),
                "{n} at {x} {y} {z}"
            );
        }
        if (22..=27).contains(&x) && (12..=17).contains(&z) {
            let col = grid.get(x, z);
            for y in col.ground().0..=72 {
                let n = name(x, y, z);
                assert!(
                    !n.contains("reed") && !n.contains("rush") && !n.contains("lily"),
                    "{n} at {x} {y} {z}"
                );
            }
        }
    }
}

/// Overlapping pieces agree on every block around a waterfall.
#[test]
fn pieces_agree_over_a_waterfall() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    let a = Rect::new(0, 0, 40, 30);
    let b = Rect::new(13, 7, 53, 37);
    let (pa, _) = w.piece_of(a, waterfall, true, true);
    let (pb, _) = w.piece_of(b, waterfall, true, true);
    for (x, z) in a.intersect(&b).columns() {
        assert_eq!(pa.column(x, z), pb.column(x, z), "differs at {x} {z}");
    }
}

/// A mountainside: Mountains rising along x, rough enough in places for stony ground and scree.
fn mountainside(x: i32, z: i32) -> ColumnInfo {
    let wave = ((z as f32) * 0.07).sin() * 3.0 + ((x as f32) * 0.05).cos() * 2.0;
    ColumnInfo {
        slope: 0.35 + 0.25 * ((x + 2 * z) as f32 * 0.03).sin().abs(),
        moisture: 0.35,
        rock: (((x - z) as f32) * 0.02).sin().max(0.0),
        ..ColumnInfo::dry(110.0 + 0.45 * x as f32 + wave, Biome::Mountains)
    }
}

/// Boulders on mountains are few and lie low: they stand at most two blocks out of the slope.
#[test]
fn mountain_boulders_are_few_and_low() {
    let Some(catalog) = catalog() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let palette = Palette::new();
    let w = World::new(&catalog, &palette);
    let rect = Rect::new(0, 0, 200, 200);
    let (bare, grid) = w.piece_of(rect, mountainside, false, false);
    let (full, _) = w.piece_of(rect, mountainside, false, true);
    let kit = &w.surface.kit;
    let (mut rock, mut tallest, mut proud) = (0, 0, 0);
    for (x, z) in rect.columns() {
        let (gy, _) = grid.get(x, z).ground();
        let mut top = None;
        for y in gy..gy + 8 {
            let id = full.get(x, y, z);
            if id != bare.get(x, y, z) && kit.of_block(id).is_some_and(|(_, l)| l == 0) {
                top = Some(y);
            }
        }
        if let Some(t) = top {
            rock += 1;
            tallest = tallest.max(t - gy + 1);
            if t - gy + 1 > 1 {
                proud += 1;
            }
        }
    }
    let share = rock as f32 / rect.columns().count() as f32;
    eprintln!(
        "boulder columns {rock} ({:.2}%), {proud} over a block, tallest {tallest}",
        share * 100.0
    );
    assert!(share < 0.004, "{rock}");
    assert!(tallest <= 2, "{tallest}");
}
