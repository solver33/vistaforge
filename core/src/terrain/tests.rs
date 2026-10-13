use super::*;
use crate::settings::{BiomeShare, ShapeEdit};

pub(super) fn settings(size: u32, mix: &[(Biome, f32)]) -> WorldSettings {
    WorldSettings {
        size,
        seed: 42,
        biomes: mix
            .iter()
            .map(|&(biome, share)| BiomeShare { biome, share })
            .collect(),
        ..WorldSettings::default()
    }
}

#[test]
fn column_and_grid_agree() {
    let mut s = settings(256, &[(Biome::Plains, 0.5), (Biome::Swamp, 0.5)]);
    s.sliders.sea = 0.3;
    s.edits.push(ShapeEdit::River {
        points: vec![[10.0, 128.0], [250.0, 140.0]],
        width: 8.0,
        depth: 2.0,
    });
    let t = Terrain::new(&s, Quality::Full).unwrap();
    let rect = Rect::new(-5, 100, 40, 160);
    let g = t.grid(rect);
    for (x, z) in rect.columns() {
        assert_eq!(*g.get(x, z), t.column(x, z), "({x}, {z})");
    }
    let again = Terrain::new(&s, Quality::Full).unwrap();
    assert_eq!(again.column(17, 131), t.column(17, 131));
}

#[test]
fn every_edit_shapes_the_land() {
    let base = settings(256, &[(Biome::Plains, 1.0)]);
    let plain = Terrain::new(&base, Quality::Preview).unwrap();
    let with = |edit: ShapeEdit| {
        let mut s = base.clone();
        s.edits.push(edit);
        Terrain::new(&s, Quality::Preview).unwrap()
    };
    let before = plain.column(128, 128);

    let t = with(ShapeEdit::Hill {
        x: 128.0,
        z: 128.0,
        radius: 60.0,
        height: 30.0,
    });
    assert!(t.column(128, 128).height > before.height + 15.0);

    let t = with(ShapeEdit::Hill {
        x: 128.0,
        z: 128.0,
        radius: 50.0,
        height: -12.0,
    });
    let c = t.column(128, 128);
    assert!(c.height < before.height - 5.0 || c.is_wet(), "{c:?}");

    let t = with(ShapeEdit::Mountain {
        x: 128.0,
        z: 128.0,
        radius: 90.0,
        height: 140.0,
    });
    let c = t.column(128, 128);
    assert!(c.height > before.height + 70.0, "{c:?}");
    assert!(layout::is_mountain(c.biome) || c.rock > 0.3, "{c:?}");

    let t = with(ShapeEdit::Plateau {
        x: 128.0,
        z: 128.0,
        radius: 70.0,
        level: 100.0,
    });
    let c = t.column(128, 128);
    assert!((c.height - 100.0).abs() < 4.0, "{c:?}");

    let t = with(ShapeEdit::Lake {
        x: 128.0,
        z: 128.0,
        radius: 40.0,
        depth: 6.0,
    });
    let c = t.column(128, 128);
    assert_eq!(c.water, WaterKind::Lake, "{c:?}");
    assert!(c.depth() > 2.0, "{c:?}");

    let t = with(ShapeEdit::River {
        points: vec![[5.0, 120.0], [250.0, 136.0]],
        width: 10.0,
        depth: 2.5,
    });
    let wet = (100..156)
        .filter(|&z| t.column(128, z).water == WaterKind::River)
        .count();
    assert!(wet >= 4, "{wet}");

    let t = with(ShapeEdit::Ridge {
        points: vec![[40.0, 128.0], [216.0, 128.0]],
        width: 50.0,
        height: 60.0,
    });
    assert!(t.column(128, 128).height > before.height + 25.0);

    let t = with(ShapeEdit::BiomeZone {
        x: 128.0,
        z: 128.0,
        radius: 60.0,
        biome: Biome::Taiga,
    });
    assert_eq!(t.column(128, 128).biome, Biome::Taiga);
    assert_eq!(t.column(10, 10).biome, plain.column(10, 10).biome);

    let t = with(ShapeEdit::BiomeZone {
        x: 128.0,
        z: 128.0,
        radius: 50.0,
        biome: Biome::Lake,
    });
    assert!(t.column(128, 128).is_wet());
}

#[test]
fn columns_are_in_range() {
    for id in [
        "realistic_mountains",
        "swamp",
        "archipelago",
        "temperate_mix",
    ] {
        let s = presets::find(id).unwrap().settings("t", 256, 5);
        let t = Terrain::new(&s, Quality::Preview).unwrap();
        let g = t.grid(Rect::new(0, 0, 256, 256));
        for (x, z) in g.rect.columns() {
            let c = g.get(x, z);
            let at = format!("{id} ({x}, {z}) {c:?}");
            assert!(c.height > MIN_Y as f32 && c.height <= TOP, "{at}");
            assert!((0.0..=0.5).contains(&c.blend), "{at}");
            for v in [c.moisture, c.rock, c.flow] {
                assert!((0.0..=1.0).contains(&v), "{at}");
            }
            assert!(c.slope >= 0.0 && c.slope.is_finite(), "{at}");
            assert_eq!(c.water_level.is_some(), c.water != WaterKind::None, "{at}");
            if let Some(level) = c.water_level {
                assert!(c.ground().0 <= level, "{at}");
            }
        }
    }
}

fn biome_rgb(b: Biome) -> [f32; 3] {
    match b {
        Biome::Plains => [126.0, 170.0, 80.0],
        Biome::Meadow => [150.0, 190.0, 90.0],
        Biome::Forest => [60.0, 120.0, 50.0],
        Biome::BirchForest => [100.0, 150.0, 70.0],
        Biome::Taiga => [50.0, 95.0, 70.0],
        Biome::PineForest => [90.0, 115.0, 60.0],
        Biome::Swamp => [80.0, 100.0, 60.0],
        Biome::Marsh => [110.0, 130.0, 80.0],
        Biome::Highlands => [140.0, 140.0, 95.0],
        Biome::Mountains => [125.0, 120.0, 115.0],
        Biome::AlpineMeadow => [150.0, 165.0, 105.0],
        Biome::SnowyPeaks => [240.0, 245.0, 250.0],
        Biome::Beach => [220.0, 205.0, 150.0],
        Biome::River => [70.0, 130.0, 210.0],
        Biome::Lake => [50.0, 100.0, 190.0],
    }
}

/// Renders a shaded map of the terrain: biome colours, rock, water and hill shading.
pub(super) fn render(t: &Terrain, step: i32) -> (u32, Vec<[u8; 3]>) {
    let size = t.size();
    let res = (size / step) as usize;
    let mut cols = vec![ColumnInfo::dry(0.0, Biome::Plains); res * res];
    let tile = 64;
    for tz in (0..size).step_by(tile) {
        for tx in (0..size).step_by(tile) {
            if step == 1 {
                let g = t.grid(Rect::new(
                    tx,
                    tz,
                    (tx + tile as i32).min(size),
                    (tz + tile as i32).min(size),
                ));
                for (x, z) in g.rect.columns() {
                    cols[z as usize * res + x as usize] = *g.get(x, z);
                }
            }
        }
    }
    if step > 1 {
        for j in 0..res {
            for i in 0..res {
                cols[j * res + i] = t.column(i as i32 * step, j as i32 * step);
            }
        }
    }
    let px = (0..res * res)
        .map(|k| {
            let (i, j) = (k % res, k / res);
            let c = &cols[k];
            let h = |a: usize, b: usize| cols[b.min(res - 1) * res + a.min(res - 1)].height;
            let dx = h(i + 1, j) - h(i.saturating_sub(1), j);
            let dz = h(i, j + 1) - h(i, j.saturating_sub(1));
            let light = (1.0 - (dx + dz) / step as f32 * 0.45).clamp(0.35, 1.6);
            let mut rgb = biome_rgb(c.biome);
            let o = biome_rgb(c.biome2);
            for q in 0..3 {
                rgb[q] = rgb[q] * (1.0 - c.blend) + o[q] * c.blend;
                rgb[q] = rgb[q] * (1.0 - c.rock * 0.8) + 118.0 * c.rock * 0.8;
            }
            if let Some(w) = c.water_level {
                let depth = (w as f32 + 1.0 - c.height).max(0.0);
                let base = match c.water {
                    WaterKind::River => [60.0, 140.0, 230.0],
                    WaterKind::Swamp => [40.0, 140.0, 160.0],
                    _ => [40.0, 90.0, 190.0],
                };
                let t = (0.35 + depth * 0.08).min(0.9);
                for q in 0..3 {
                    rgb[q] = rgb[q] * (1.0 - t) + base[q] * t;
                }
                return rgb.map(|v| v.clamp(0.0, 255.0) as u8);
            }
            rgb.map(|v| (v * light).clamp(0.0, 255.0) as u8)
        })
        .collect();
    (res as u32, px)
}

/// Writes renders of a few settings to $VF_RENDER (run with --ignored --nocapture).
#[test]
#[ignore]
fn render_maps() {
    let dir = std::env::var("VF_RENDER").unwrap_or_else(|_| "/tmp/vf-render".into());
    std::fs::create_dir_all(&dir).unwrap();
    let only = std::env::var("VF_ONLY").ok();
    let size: u32 = std::env::var("VF_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);
    let quality = if std::env::var("VF_PREVIEW").is_ok() {
        Quality::Preview
    } else {
        Quality::Full
    };
    for p in presets::all() {
        if only
            .as_ref()
            .is_some_and(|o| !o.split(',').any(|x| x == p.id))
        {
            continue;
        }
        let s = p.settings("t", size, 7);
        let start = std::time::Instant::now();
        let t = Terrain::new(&s, quality).unwrap();
        let built = start.elapsed().as_secs_f64();
        let step = (size / 1024).max(1) as i32;
        let (res, px) = render(&t, step);
        let path = format!("{dir}/{}.png", p.id);
        std::fs::write(&path, crate::png::encode_rgb(res, res, &px).unwrap()).unwrap();
        println!(
            "{} built in {built:.2}s, rendered in {:.2}s -> {path}",
            p.id,
            start.elapsed().as_secs_f64() - built
        );
    }
}

/// Every column of the map, fetched through `grid` in engine-sized units.
fn whole_map(t: &Terrain) -> Vec<ColumnInfo> {
    let size = t.size();
    let mut cols = vec![ColumnInfo::dry(0.0, Biome::Plains); (size * size) as usize];
    for tz in (0..size).step_by(64) {
        for tx in (0..size).step_by(64) {
            let g = t.grid(Rect::new(tx, tz, (tx + 64).min(size), (tz + 64).min(size)));
            for (x, z) in g.rect.columns() {
                cols[(z * size + x) as usize] = *g.get(x, z);
            }
        }
    }
    cols
}

/// Water that would flow when the world loads: (water beside dry ground below its level,
/// still water beside lower water). Rivers may step down, so they only count in the first.
fn leaks(t: &Terrain, cols: &[ColumnInfo]) -> (usize, usize) {
    let size = t.size() as usize;
    let (mut spills, mut steps) = (0, 0);
    for z in 0..size {
        for x in 0..size {
            let c = &cols[z * size + x];
            let Some(level) = c.water_level else { continue };
            for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                if nx < 0 || nz < 0 || nx >= size as i32 || nz >= size as i32 {
                    continue;
                }
                let o = &cols[nz as usize * size + nx as usize];
                match o.water_level {
                    None if o.ground().0 <= level => spills += 1,
                    Some(l)
                        if l < level
                            && c.water != WaterKind::River
                            && o.water != WaterKind::River =>
                    {
                        steps += 1
                    }
                    _ => {}
                }
            }
        }
    }
    (spills, steps)
}

#[test]
fn water_stays_in_place() {
    for id in [
        "swamp",
        "wetlands",
        "archipelago",
        "river_valley",
        "mountain_lakes",
    ] {
        let s = presets::find(id).unwrap().settings("t", 256, 3);
        let t = Terrain::new(&s, Quality::Full).unwrap();
        let cols = whole_map(&t);
        let wet = cols.iter().filter(|c| c.is_wet()).count();
        let (spills, steps) = leaks(&t, &cols);
        assert!(
            spills * 2000 <= wet.max(1),
            "{id}: {spills} spills over {wet} wet columns"
        );
        assert!(
            steps * 2000 <= wet.max(1),
            "{id}: {steps} steps over {wet} wet columns"
        );
    }
}

/// Leak counts for every preset (run with --ignored --nocapture).
#[test]
#[ignore]
fn leak_stats() {
    let size: u32 = std::env::var("VF_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(512);
    let only = std::env::var("VF_ONLY").ok();
    for p in presets::all() {
        if only
            .as_ref()
            .is_some_and(|o| !o.split(',').any(|x| x == p.id))
        {
            continue;
        }
        let t = Terrain::new(&p.settings("t", size, 7), Quality::Full).unwrap();
        let cols = whole_map(&t);
        let wet = cols.iter().filter(|c| c.is_wet()).count();
        let (spills, steps) = leaks(&t, &cols);
        println!("{:24} wet {wet:8} spills {spills:6} steps {steps:6}", p.id);
    }
}

/// Build times at a given size (run with --release --ignored --nocapture).
#[test]
#[ignore]
fn timing() {
    let size: u32 = std::env::var("VF_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4096);
    for id in std::env::var("VF_ONLY")
        .unwrap_or("realistic_mountains,swamp,archipelago,temperate_mix".into())
        .split(',')
    {
        let s = presets::find(id).unwrap().settings("t", size, 7);
        for q in [Quality::Preview, Quality::Full] {
            let start = std::time::Instant::now();
            let t = Terrain::new(&s, q).unwrap();
            let built = start.elapsed().as_secs_f64();
            // The preview samples about 512 x 512 columns.
            let step = (size / 512).max(1) as i32;
            let start = std::time::Instant::now();
            let mut sum = 0.0;
            for j in 0..512 {
                for i in 0..512 {
                    sum += t.column(i * step, j * step).height;
                }
            }
            println!(
                "{id:20} {q:?} built {built:.2}s, 512x512 columns {:.2}s ({sum:.0})",
                start.elapsed().as_secs_f64()
            );
        }
    }
}

/// Dry ground that water levels raised above the natural ground, for every preset: walls
/// holding rivers ("aqueducts") and lakes on slopes (run with --release --ignored
/// --nocapture). With VF_RENDER set, also writes a map with the raised columns in red.
#[test]
#[ignore]
fn wall_stats() {
    let size: u32 = std::env::var("VF_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(512);
    let only = std::env::var("VF_ONLY").ok();
    let dir = std::env::var("VF_RENDER").ok();
    if let Some(d) = &dir {
        std::fs::create_dir_all(d).unwrap();
    }
    for p in presets::all() {
        if only
            .as_ref()
            .is_some_and(|o| !o.split(',').any(|x| x == p.id))
        {
            continue;
        }
        let t = Terrain::new(&p.settings("t", size, 7), Quality::Full).unwrap();
        let n = size as i32;
        // Per kind (river, lake, sea, wetland): columns raised by 2+, 4+ and 8+ blocks.
        let mut counts = [[0usize; 3]; 4];
        let mut worst = [0f32; 4];
        let mut raised = vec![0f32; (n * n) as usize];
        let (mut falls, mut drop) = (0, 0);
        for z in 0..n {
            for x in 0..n {
                let s = t.sample(x, z);
                if let (Some(f), Some(l)) = (s.fall, s.level) {
                    falls += 1;
                    drop = drop.max(f - l);
                    raised[(z * n + x) as usize] = -1.0;
                }
                if s.water != WaterKind::None {
                    continue;
                }
                let (fx, fz) = (x as f64, z as f64);
                let swampy = t.swamp.bilinear(fx, fz);
                let base = t.natural(fx, fz, swampy)
                    + t.rough.bilinear(fx, fz) * t.detail_amp * t.micro(fx, fz);
                let r = s.h - base as f32;
                if r < 2.0 {
                    continue;
                }
                raised[(z * n + x) as usize] = r;
                let kind = if t.rivers.at(fx, fz).is_some_and(|c| c.bank.is_some()) {
                    0
                } else if t.lake_rim(fx, fz).is_some() {
                    1
                } else if t.standing_water(fx, fz).1 > 0.0 {
                    2
                } else {
                    3
                };
                for (i, lim) in [2.0, 4.0, 8.0].iter().enumerate() {
                    if r >= *lim {
                        counts[kind][i] += 1;
                    }
                }
                worst[kind] = worst[kind].max(r);
            }
        }
        let line: Vec<String> = ["river", "lake", "sea", "wet"]
            .iter()
            .enumerate()
            .map(|(k, name)| {
                let c = counts[k];
                format!(
                    "{name} {:6}/{:5}/{:4} max {:5.1}",
                    c[0], c[1], c[2], worst[k]
                )
            })
            .collect();
        println!(
            "{:22} {}  falls {falls:5} max {drop:3}",
            p.id,
            line.join("  ")
        );
        if let Some(dir) = &dir {
            let step = (size / 1024).max(1) as i32;
            let (res, mut px) = render(&t, step);
            for j in 0..res as i32 {
                for i in 0..res as i32 {
                    let r = raised[(j * step * n + i * step) as usize];
                    if r < 0.0 {
                        px[(j * res as i32 + i) as usize] = [255, 0, 255];
                    } else if r >= 2.0 {
                        let c = &mut px[(j * res as i32 + i) as usize];
                        let a = (r / 8.0).min(1.0);
                        c[0] = 255;
                        c[1] = (c[1] as f32 * (1.0 - a)) as u8;
                        c[2] = (c[2] as f32 * (1.0 - a)) as u8;
                    }
                }
            }
            let path = format!("{dir}/{}-walls.png", p.id);
            std::fs::write(&path, crate::png::encode_rgb(res, res, &px).unwrap()).unwrap();
        }
    }
}

/// Dumps a square of columns for a 3D look (run with --release --ignored --nocapture):
/// VF_ONLY (one preset), VF_SIZE, VF_X0, VF_Z0, VF_W, VF_OUT. Each column is height (f32),
/// water level and fall top (i32, -9999 when none) and r, g, b (u8), little-endian.
#[test]
#[ignore]
fn dump_columns() {
    let id = std::env::var("VF_ONLY").unwrap();
    let size: u32 = std::env::var("VF_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);
    let var = |k: &str| std::env::var(k).unwrap().parse::<i32>().unwrap();
    let (x0, z0, w) = (var("VF_X0"), var("VF_Z0"), var("VF_W"));
    let mut s = presets::find(&id).unwrap().settings("t", size, 7);
    if std::env::var("VF_TALL").is_ok() {
        s.world_height = crate::settings::WorldHeight::Tall;
    }
    let t = Terrain::new(&s, Quality::Full).unwrap();
    let g = t.grid(Rect::new(x0, z0, x0 + w, z0 + w));
    let mut out = Vec::new();
    for z in z0..z0 + w {
        for x in x0..x0 + w {
            let c = g.get(x, z);
            out.extend_from_slice(&c.height.to_le_bytes());
            out.extend_from_slice(&c.water_level.unwrap_or(-9999).to_le_bytes());
            out.extend_from_slice(&c.fall.unwrap_or(-9999).to_le_bytes());
            let mut rgb = biome_rgb(c.biome);
            let o = biome_rgb(c.biome2);
            for q in 0..3 {
                rgb[q] = rgb[q] * (1.0 - c.blend) + o[q] * c.blend;
                rgb[q] = rgb[q] * (1.0 - c.rock * 0.8) + 118.0 * c.rock * 0.8;
            }
            out.extend(rgb.map(|v| v.clamp(0.0, 255.0) as u8));
        }
    }
    std::fs::write(std::env::var("VF_OUT").unwrap(), out).unwrap();
}
