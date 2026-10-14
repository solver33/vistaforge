use crate::biome::Biome;
use crate::column::{ColumnGrid, ColumnInfo, Rect, WaterKind};
use crate::settings::{ShapeEdit, WorldSettings};
use crate::terrain::presets;
use crate::terrain::{Quality, Terrain};

use super::super::tests::settings;

/// Water beside dry ground below its level, and water beside water two or more blocks lower
/// that no curtain falls into: what would flow or show a wall of water in game.
fn leaks(g: &ColumnGrid, rect: Rect) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for (x, z) in rect.columns() {
        let c = g.get(x, z);
        let Some(level) = c.water_level else { continue };
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let Some(o) = g.try_get(x + dx, z + dz) else {
                continue;
            };
            let leak = match o.water_level {
                None => o.ground().0 <= level,
                Some(l) => l + 2 <= level && o.fall.is_none_or(|f| f < level),
            };
            if leak {
                out.push((x, z));
            }
        }
    }
    out
}

fn with(mut s: WorldSettings, edit: ShapeEdit) -> Terrain {
    s.edits.push(edit);
    Terrain::new(&s, Quality::Full).unwrap()
}

fn waterfall(x: f32, z: f32, radius: f32, drop: f32, angle: f32) -> ShapeEdit {
    ShapeEdit::Waterfall {
        x,
        z,
        radius,
        drop,
        angle,
    }
}

#[test]
fn a_waterfall_drops_into_its_pool() {
    let s = settings(256, &[(Biome::Plains, 1.0)]);
    let t = with(s, waterfall(128.0, 128.0, 3.0, 12.0, 0.0));
    // The stream above the lip, the curtain and the pool.
    let stream = t.column(122, 128);
    let top = stream.water_level.expect("a stream above the lip");
    assert_eq!(stream.water, WaterKind::River);
    let rect = Rect::new(40, 40, 216, 216);
    let g = t.grid(rect.expand(1));
    let curtain: Vec<&ColumnInfo> = g.columns.iter().filter(|c| c.fall.is_some()).collect();
    assert!(curtain.len() >= 5, "{} curtain columns", curtain.len());
    for c in &curtain {
        assert_eq!(c.fall, Some(top));
        assert_eq!(c.water_level, Some(top - 12));
    }
    let pool = t.column(135, 128);
    assert_eq!(pool.water_level, Some(top - 12));
    assert!(pool.depth() >= 2.0, "{}", pool.depth());
    // The cliff beside the fall stands over the pool.
    let beside = t.column(124, 136);
    assert!(beside.height >= (top + 1) as f32, "{} {top}", beside.height);
    assert_eq!(leaks(&g, rect), vec![]);
}

#[test]
fn waterfalls_hold_their_water_anywhere() {
    let base = presets::find("highlands")
        .unwrap()
        .settings("t", 384, 11);
    for (k, (radius, drop, angle)) in [
        (1.0, 4.0, 0.0),
        (2.0, 9.0, 37.0),
        (5.0, 30.0, 135.0),
        (12.0, 22.0, 270.0),
        (4.0, 80.0, 200.0),
    ]
    .into_iter()
    .enumerate()
    {
        let (x, z) = (120.0 + 40.0 * k as f32, 260.0 - 30.0 * k as f32);
        let t = with(base.clone(), waterfall(x, z, radius, drop, angle));
        let rect = Rect::new(0, 0, 384, 384);
        let g = t.grid(rect.expand(1));
        let falls = g.columns.iter().filter(|c| c.fall.is_some()).count();
        assert!(falls > 0, "case {k}: no curtain");
        let l = leaks(&g, rect);
        // The land's own water may already leak in a few places; the waterfall adds none.
        let plain = Terrain::new(&base, Quality::Full).unwrap();
        let pg = plain.grid(rect.expand(1));
        let before = leaks(&pg, rect).len();
        assert!(l.len() <= before, "case {k}: {} leaks, {before} before: {l:?}", l.len());
    }
}

#[test]
fn a_road_is_laid_smoothly_into_the_land() {
    let base = presets::find("highlands")
        .unwrap()
        .settings("t", 384, 5);
    let plain = Terrain::new(&base, Quality::Full).unwrap();
    let road = ShapeEdit::Road {
        points: vec![[20.0, 60.0], [180.0, 150.0], [360.0, 300.0]],
        width: 6.0,
    };
    let t = with(base, road);
    // Along a straight piece of the course: on the road, and smoother than the land was.
    let line = |f: f32| (20.0 + 160.0 * f, 60.0 + 90.0 * f);
    let (mut rough_before, mut rough_after, mut on) = (0.0, 0.0, 0);
    let mut prev: Option<(f32, f32)> = None;
    for k in 10..90 {
        let (x, z) = line(k as f32 / 100.0);
        let (x, z) = (x.round() as i32, z.round() as i32);
        let (a, b) = (t.column(x, z), plain.column(x, z));
        if a.road.is_some_and(|r| r > 1.0) {
            on += 1;
        }
        if let Some((pa, pb)) = prev {
            rough_after += (a.height - pa).abs();
            rough_before += (b.height - pb).abs();
        }
        prev = Some((a.height, b.height));
    }
    assert!(on >= 70, "{on} of 80 points on the road");
    assert!(
        rough_after < rough_before * 0.8,
        "{rough_after} vs {rough_before}"
    );
    // Away from it the land is untouched.
    assert_eq!(t.column(300, 40), plain.column(300, 40));
    // No water gets out.
    let rect = Rect::new(0, 0, 384, 384);
    let (g, pg) = (t.grid(rect.expand(1)), plain.grid(rect.expand(1)));
    assert!(leaks(&g, rect).len() <= leaks(&pg, rect).len());
}

#[test]
fn brushes_agree_between_columns_and_grids() {
    let mut s = settings(256, &[(Biome::Plains, 0.5), (Biome::Forest, 0.5)]);
    s.edits.push(ShapeEdit::Road {
        points: vec![[0.0, 100.0], [255.0, 120.0]],
        width: 9.0,
    });
    s.edits.push(waterfall(120.0, 140.0, 4.0, 15.0, 80.0));
    let t = Terrain::new(&s, Quality::Full).unwrap();
    let rect = Rect::new(100, 90, 150, 170);
    let g = t.grid(rect);
    for (x, z) in rect.columns() {
        assert_eq!(*g.get(x, z), t.column(x, z), "({x}, {z})");
    }
}

/// Roads and waterfalls across rivers and lakes never let their water out.
#[test]
fn brushes_keep_rivers_and_lakes_in() {
    for id in ["river_valley", "lake_district"] {
        let base = presets::find(id).unwrap().settings("t", 384, 3);
        let plain = Terrain::new(&base, Quality::Full).unwrap();
        let mut s = base.clone();
        for k in 0..6 {
            let f = k as f32 / 6.0;
            s.edits.push(ShapeEdit::Road {
                points: vec![[10.0, 20.0 + 340.0 * f], [190.0, 380.0 - 300.0 * f], [370.0, 30.0 + 320.0 * f]],
                width: 2.0 + 6.0 * k as f32,
            });
            s.edits.push(waterfall(60.0 + 50.0 * k as f32, 330.0 - 55.0 * k as f32, 1.0 + 3.0 * k as f32, 5.0 + 9.0 * k as f32, 60.0 * k as f32));
        }
        let t = Terrain::new(&s, Quality::Full).unwrap();
        let rect = Rect::new(0, 0, 384, 384);
        let (g, pg) = (t.grid(rect.expand(1)), plain.grid(rect.expand(1)));
        let (after, before) = (leaks(&g, rect), leaks(&pg, rect));
        assert!(after.len() <= before.len(), "{id}: {} leaks, {} before: {after:?}", after.len(), before.len());
    }
}

#[test]
#[ignore]
fn debug_case() {
    let base = presets::find("highlands").unwrap().settings("t", 384, 11);
    let t = with(base.clone(), waterfall(160.0, 230.0, 2.0, 9.0, 37.0));
    let plain = Terrain::new(&base, Quality::Full).unwrap();
    for (cx, cz) in [(173, 251), (184, 241)] {
        for z in cz - 2..=cz + 2 {
            let mut line = String::new();
            for x in cx - 2..=cx + 2 {
                let (a, b) = (t.column(x, z), plain.column(x, z));
                line += &format!(
                    " [{:6.2} {:?} {:?} | {:6.2} {:?}]",
                    a.height, a.water_level, a.water, b.height, b.water_level
                );
            }
            println!("{z}: {line}");
        }
        println!();
    }
}

/// Prints the blocks around a road and a waterfall in a generated piece (run with --ignored
/// --nocapture; needs the catalog).
#[test]
#[ignore]
fn dump_blocks() {
    use crate::catalog::Catalog;
    use crate::engine::Generator;
    let path = std::env::var("VISTAFORGE_CATALOG")
        .unwrap_or("/mnt/project-files/vistaforge/catalog-cr-1.7.0.json".into());
    let catalog = Catalog::load(std::path::Path::new(&path)).unwrap();
    let mut s = settings(512, &[(Biome::Plains, 0.6), (Biome::Forest, 0.4)]);
    s.seed = 7;
    s.edits.push(ShapeEdit::Road {
        points: vec![[30.0, 250.0], [150.0, 220.0], [260.0, 270.0], [400.0, 240.0], [490.0, 300.0]],
        width: 5.0,
    });
    s.edits.push(waterfall(128.0, 380.0, 3.0, 14.0, 90.0));
    let g = Generator::new(&s, &catalog, Quality::Full).unwrap();
    let var = |k: &str, d: i32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (x0, z0, w) = (var("VF_X0", 130), var("VF_Z0", 205), var("VF_W", 40));
    let piece = g.generate(Rect::new(x0, z0, x0 + w, z0 + w));
    let short = |id| {
        g.palette.with(id, |st| {
            let n = st.name.trim_start_matches("conquest:").trim_start_matches("minecraft:");
            let l = st.get("layers").map(|l| format!("{l}")).unwrap_or_default();
            format!("{n}{l}")
        })
    };
    let mut names = std::collections::BTreeMap::new();
    for z in z0..z0 + w {
        let mut line = String::new();
        for x in x0..x0 + w {
            let top = piece.area.top(x, z).unwrap();
            let col = piece.area.column(x, z);
            let id = col[(top - crate::world::MIN_Y) as usize];
            let n = short(id);
            let next = names.len();
            let c = *names.entry(n).or_insert(next);
            let ch = if c < 26 { (b'a' + c as u8) as char } else { (b'A' + (c - 26) as u8 % 26) as char };
            line.push(ch);
        }
        println!("{z:4} {line}");
    }
    for (n, c) in &names {
        let ch = if *c < 26 { (b'a' + *c as u8) as char } else { (b'A' + (*c - 26) as u8 % 26) as char };
        println!("{ch} {n}");
    }
}
