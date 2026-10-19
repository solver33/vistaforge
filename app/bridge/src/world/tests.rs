//! Opened worlds, tested on small worlds written here with the core's own Anvil writer: a
//! sealed cave and one with a mouth, a lake with kelp, an overhang, a tree, a chunk never
//! saved, and coordinates on both sides of zero.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vistaforge_core::anvil::{chunk_tag, compress_chunk, RegionWriter};
use vistaforge_core::catalog::{BlockState, Catalog};
use vistaforge_core::column::Rect;
use vistaforge_core::level::{write_level_dat, LevelSettings};
use vistaforge_core::settings::WorldHeight;
use vistaforge_core::sink::{BlockSink, Palette};
use vistaforge_core::world::Area;

use super::map::{area_preview, overview};
use super::*;
use crate::render::library::{CatalogBlock, Library};
use crate::render::tests::decode as decode_scene;
use crate::render::{SceneRequest, FULL, WATERLOGGED, WATER_TOP};

const CATALOG: &str = r#"{"format": 1, "blocks": {
  "minecraft:air": {"class": "net.minecraft.class_2189", "opaque": false},
  "minecraft:stone": {"class": "net.minecraft.class_2248", "top": {"rgb": "7f7f7f"}},
  "minecraft:dirt": {"class": "net.minecraft.class_2248", "top": {"rgb": "86603f"}},
  "minecraft:grass_block": {"class": "net.minecraft.class_2372",
    "properties": {"snowy": ["true", "false"]}, "default": {"snowy": "false"},
    "top": {"rgb": "72716c", "tint": "7ba24f"}},
  "minecraft:water": {"class": "net.minecraft.class_2404", "opaque": false,
    "properties": {"level": ["0"]}, "top": {"rgb": "6e787d"}},
  "minecraft:kelp": {"class": "net.minecraft.class_2393", "opaque": false,
    "properties": {"age": ["0"]}, "top": {"rgb": "5a8a3a", "coverage": 0.3}},
  "minecraft:oak_log": {"class": "net.minecraft.class_2465",
    "properties": {"axis": ["x", "y", "z"]}, "default": {"axis": "y"}, "top": {"rgb": "a0824f"}},
  "minecraft:oak_leaves": {"class": "net.minecraft.class_2397", "opaque": false,
    "top": {"rgb": "a09698", "coverage": 0.3, "tint": "48b518"}},
  "minecraft:poppy": {"class": "net.minecraft.class_2356", "opaque": false,
    "top": {"rgb": "c03030", "coverage": 0.2}}
}}"#;

#[derive(serde::Deserialize)]
struct Faces {
    blocks: HashMap<String, CatalogBlock>,
}

pub(crate) fn test_catalog() -> (Arc<Catalog>, Arc<HashMap<String, CatalogBlock>>) {
    let catalog = Catalog::from_json(CATALOG).unwrap();
    let faces = serde_json::from_str::<Faces>(CATALOG).unwrap().blocks;
    (Arc::new(catalog), Arc::new(faces))
}

fn state(name: &str, props: &[(&str, &str)]) -> BlockState {
    let mut props: Vec<(String, String)> = props
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    props.sort();
    BlockState {
        name: name.into(),
        props,
    }
}

pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vistaforge-world-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The test world's blocks, x and z in -32..32. Chunk (1, 1) is never saved.
const RECT: Rect = Rect {
    x0: -32,
    z0: -32,
    x1: 32,
    z1: 32,
};

fn write_world(dir: &Path) {
    let palette = Palette::new();
    let mut area = Area::new(RECT, &palette);
    let id = |s: BlockState| palette.intern(&s);
    let stone = id(state("minecraft:stone", &[]));
    let dirt = id(state("minecraft:dirt", &[]));
    let grass = id(state("minecraft:grass_block", &[("snowy", "false")]));
    let water = id(state("minecraft:water", &[("level", "0")]));
    let kelp = id(state("minecraft:kelp", &[("age", "0")]));
    let log = id(state("minecraft:oak_log", &[("axis", "y")]));
    let leaves = id(state("minecraft:oak_leaves", &[]));
    let poppy = id(state("minecraft:poppy", &[]));
    for (x, z) in RECT.columns() {
        area.fill_column(x, z, -64, 60, stone);
        area.fill_column(x, z, 60, 63, dirt);
        area.set(x, 63, z, grass);
    }
    // A sealed cave deep down.
    for x in -20..-10 {
        for z in -20..-10 {
            area.fill_column(x, z, 0, 10, 0);
        }
    }
    // A shaft from the surface down to Y 30, and a tunnel leading off it.
    for x in 5..8 {
        for z in 5..8 {
            area.fill_column(x, z, 30, 64, 0);
        }
    }
    for x in 8..14 {
        for z in 5..8 {
            area.fill_column(x, z, 30, 34, 0);
        }
    }
    // A lake with kelp in it.
    for x in 10..20 {
        for z in -20..-10 {
            area.fill_column(x, z, 58, 64, water);
        }
    }
    area.fill_column(15, -15, 58, 61, kelp);
    // A ledge with air under it.
    for x in -5..0 {
        for z in 20..26 {
            area.fill_column(x, z, 70, 73, stone);
        }
    }
    // A tree, and a flower.
    for x in -2..=2 {
        for z in -2..=2 {
            area.fill_column(x, z, 67, 71, leaves);
        }
    }
    area.fill_column(0, 0, 64, 70, log);
    area.set(-10, 64, 0, poppy);

    let mut regions: HashMap<(i32, i32), RegionWriter> = HashMap::new();
    let mut scratch = Vec::new();
    for cz in -2..2 {
        for cx in -2..2 {
            if (cx, cz) == (1, 1) {
                continue;
            }
            let tag = chunk_tag(
                cx,
                cz,
                &area,
                &palette,
                &|_, y, _| {
                    if y < 60 {
                        "minecraft:river"
                    } else {
                        "minecraft:forest"
                    }
                },
                &mut scratch,
            );
            regions
                .entry((cx >> 5, cz >> 5))
                .or_default()
                .add(cx, cz, &compress_chunk(&tag).unwrap())
                .unwrap();
        }
    }
    for ((rx, rz), r) in regions {
        r.write(&dir.join("region"), rx, rz).unwrap();
    }
    write_level_dat(
        &LevelSettings {
            name: "Test Valley".into(),
            seed: 1,
            spawn: (3, 64, -4),
            border_center: (0.0, 0.0),
            border_size: 64.0,
            creative: true,
            height: WorldHeight::Standard,
        },
        dir,
    )
    .unwrap();
}

fn open_test_world(name: &str) -> (OpenWorld, PathBuf) {
    let dir = temp_dir(name);
    write_world(&dir);
    let (catalog, faces) = test_catalog();
    (OpenWorld::open(&dir, None, catalog, faces).unwrap(), dir)
}

#[test]
fn opens_a_world_and_finds_its_chunks() {
    let (world, dir) = open_test_world("info");
    let info = &world.info;
    assert_eq!(info.name, "Test Valley");
    assert_eq!(info.version.as_deref(), Some("1.20.1"));
    assert_eq!(info.dimension, "overworld");
    assert_eq!(info.dimensions, vec!["overworld".to_string()]);
    assert_eq!(info.spawn, [3, 64, -4]);
    assert_eq!(info.regions, 4);
    assert_eq!(info.chunks, 15);
    assert_eq!(info.bounds, Some([-32, -32, 32, 32]));
    // Opening level.dat itself opens its folder; a folder without regions is refused.
    let (catalog, faces) = test_catalog();
    let again = OpenWorld::open(&dir.join("level.dat"), None, catalog.clone(), faces.clone());
    assert_eq!(again.unwrap().info.chunks, 15);
    let empty = temp_dir("empty");
    let err = OpenWorld::open(&empty, None, catalog, faces).err().unwrap();
    assert!(err.to_string().contains("no saved chunks"), "{err}");
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(empty).ok();
}

#[test]
fn draws_a_map_of_the_saved_chunks() {
    let (world, dir) = open_test_world("map");
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = overview(&world, &|_, _| {}, &cancel).unwrap();
    assert_eq!(&bytes[..4], super::map::MAGIC);
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&bytes[8..8 + len]).unwrap();
    assert_eq!(header["x0"], -32);
    assert_eq!(header["scale"], 1);
    assert_eq!(header["width"], 64);
    assert_eq!(header["chunks"], 15);
    let rgba = &bytes[8 + len..];
    assert_eq!(rgba.len(), 64 * 64 * 4);
    let px = |x: i32, z: i32| {
        let i = ((z + 32) * 64 + (x + 32)) as usize * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    assert_eq!(px(20, 20)[3], 0, "the chunk never saved is empty");
    let grass = px(-25, 10);
    assert_eq!(grass[3], 255);
    assert!(
        grass[1] > grass[0] && grass[1] > grass[2],
        "grass is green: {grass:?}"
    );
    let lake = px(12, -12);
    assert!(
        lake[2] > lake[0] && lake[2] > lake[1],
        "the lake is blue: {lake:?}"
    );
    // The flower is looked through, the crown is not.
    let flower = px(-10, 0);
    assert!(
        flower[1] > flower[0],
        "under the flower is grass: {flower:?}"
    );
    let crown = px(1, 1);
    assert!(
        crown[1] > crown[2] && crown != grass,
        "the crown shows: {crown:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn previews_an_area_like_a_generated_map() {
    let (world, dir) = open_test_world("preview");
    let bytes = area_preview(&world, -32, -32, 64, 64).unwrap();
    let d = crate::encode::tests::decode(&bytes);
    assert_eq!(d.res, 64);
    let at = |x: i32, z: i32| ((z + 32) * 64 + (x + 32)) as usize;
    assert_eq!(d.heights[at(-25, 10)], 64.0);
    assert!(d.water[at(-25, 10)].is_nan());
    // Under the crown and the trunk is the grass.
    assert_eq!(d.heights[at(0, 0)], 64.0);
    // The lake: water at the top of Y 63, the bed of stone at Y 57.
    assert_eq!(d.water[at(12, -12)], 64.0);
    assert_eq!(d.heights[at(12, -12)], 58.0);
    assert_eq!(d.biomes[at(-25, 10)], "forest");
    assert_eq!(d.header["trees"], 0);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn renders_what_can_be_seen_and_leaves_sealed_caves_out() {
    let (world, dir) = open_test_world("scene");
    let (catalog, faces) = test_catalog();
    let mut library = Library::new(None, (*faces).clone());
    let req = SceneRequest {
        x0: -32,
        z0: -32,
        size: 64,
        game_dir: None,
        catalog_path: None,
    };
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let bytes = scene::build(&world, &catalog, &mut library, &req, &|_, _| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    })
    .unwrap();
    assert_eq!(calls.into_inner(), 1);
    let d = decode_scene(&bytes);
    assert_eq!(d.header["side"], 64);
    assert_eq!(d.header["x0"], -32);
    let column = |x: i32, z: i32| {
        let i = ((z + 32) * 64 + (x + 32)) as usize;
        let (offset, packed) = (d.columns[4 * i] as usize, d.columns[4 * i + 1]);
        let bottom = (packed & 0xFFFF) as i32 - 64;
        let count = (packed >> 16) as usize;
        (bottom, d.voxels[offset..offset + count].to_vec())
    };
    let flags = |state: u16| d.states[12 * state as usize];

    // Plain ground: only what is next to the surface.
    let (bottom, cells) = column(-25, 10);
    assert_eq!(bottom + cells.len() as i32 - 1, 63);
    assert!(bottom >= 62, "plain ground from {bottom}");
    // The sealed cave is not kept.
    let (bottom, _) = column(-15, -15);
    assert!(bottom >= 62, "over the sealed cave from {bottom}");
    // The shaft and the tunnel off it are seen from the sky, down to their floor.
    let (bottom, cells) = column(6, 6);
    assert_eq!(bottom, 29);
    assert_ne!(flags(cells[0]) & FULL, 0, "the floor is a full block");
    let (bottom, _) = column(12, 6);
    assert_eq!(bottom, 29, "the tunnel");
    let (bottom, _) = column(12, 4);
    assert!(bottom <= 30, "beside the tunnel: {bottom}");
    // The lake keeps its bed; its top is the water's surface; kelp stands in water.
    let (bottom, cells) = column(12, -12);
    assert!(bottom <= 57);
    assert_ne!(flags(*cells.last().unwrap()) & WATER_TOP, 0);
    let (_, cells) = column(15, -15);
    assert!(cells.iter().any(|&c| flags(c) & WATERLOGGED != 0), "kelp");
    // Under the ledge is open air down to the grass.
    let (bottom, cells) = column(-3, 22);
    assert!(bottom <= 63);
    assert_eq!(bottom + cells.len() as i32 - 1, 72);
    // The chunk that was never saved is empty.
    let (_, cells) = column(20, 20);
    assert!(cells.is_empty());
    // Every kept column starts on a full block.
    for z in -32..32 {
        for x in -32..32 {
            let (_, cells) = column(x, z);
            if let Some(&first) = cells.first() {
                assert_ne!(flags(first) & FULL, 0, "column {x},{z} starts on a gap");
            }
        }
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn chunks_with_unfinished_or_old_format_are_skipped() {
    use vistaforge_core::nbt::{to_bytes, Compound, Tag};
    let (catalog, faces) = test_catalog();
    let table = blocks::BlockTable::new(catalog, faces);
    let names = blocks::Names::default();
    let unfinished = Compound::new()
        .with("Status", "minecraft:features")
        .with("sections", Tag::List(vec![]));
    let bytes = to_bytes("", &unfinished);
    assert_eq!(
        chunk::decode(&bytes, &table, &names).unwrap().unwrap_err(),
        Skipped::Unfinished
    );
    let old = Compound::new().with("Level", Compound::new());
    let bytes = to_bytes("", &old);
    assert_eq!(
        chunk::decode(&bytes, &table, &names).unwrap().unwrap_err(),
        Skipped::OldFormat
    );
}

/// A map Vistaforge generated, exported and opened again, renders like the map itself.
#[test]
fn an_exported_map_opens_and_renders_like_the_generated_one() {
    let Some(path) = crate::tests::catalog_path() else {
        eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
        return;
    };
    let bridge = crate::Bridge::new();
    bridge.load_catalog(&path).unwrap();
    let settings = vistaforge_core::settings::WorldSettings {
        name: "Reopened".into(),
        size: 256,
        ..Default::default()
    };
    let dir = temp_dir("reopen");
    bridge.export(&settings, &dir, false, &|_| {}).unwrap();
    let info = bridge.open_world(&dir.join("Reopened"), None).unwrap();
    assert_eq!(info.chunks, 256);
    assert_eq!(info.bounds, Some([0, 0, 256, 256]));
    let map = bridge.world_map(&|_, _| {}).unwrap();
    assert_eq!(&map[..4], super::map::MAGIC);
    let req = SceneRequest {
        x0: 64,
        z0: 64,
        size: 128,
        game_dir: None,
        catalog_path: Some(path.clone()),
    };
    let started = std::time::Instant::now();
    let opened = decode_scene(&bridge.world_scene(&req, &|_, _| {}).unwrap());
    let took = started.elapsed();
    let generated = decode_scene(&bridge.render_scene(&settings, &req, &|_, _| {}).unwrap());
    let (a, b) = (
        opened.header["voxels"].as_f64().unwrap(),
        generated.header["voxels"].as_f64().unwrap(),
    );
    eprintln!("opened {a} voxels in {took:?}, generated {b}");
    assert!((a - b).abs() / b < 0.25, "{a} vs {b} voxels");
    assert_eq!(opened.header["max_y"], generated.header["max_y"]);
    std::fs::remove_dir_all(dir).ok();
}

/// How long a real world takes: `VISTAFORGE_TEST_WORLD=<world folder> cargo test --release
/// -p vistaforge-bridge timing -- --ignored --nocapture`.
#[test]
#[ignore]
fn timing_on_a_real_world() {
    let (Some(path), Ok(world)) = (
        crate::tests::catalog_path(),
        std::env::var("VISTAFORGE_TEST_WORLD"),
    ) else {
        eprintln!("skipped: set VISTAFORGE_TEST_WORLD and VISTAFORGE_CATALOG");
        return;
    };
    let size: u32 = std::env::var("VISTAFORGE_TEST_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);
    let bridge = crate::Bridge::new();
    bridge.load_catalog(&path).unwrap();
    let t = std::time::Instant::now();
    let info = bridge.open_world(Path::new(&world), None).unwrap();
    eprintln!(
        "open: {:?}, {} chunks, bounds {:?}",
        t.elapsed(),
        info.chunks,
        info.bounds
    );
    let t = std::time::Instant::now();
    let map = bridge.world_map(&|_, _| {}).unwrap();
    eprintln!("map: {:?}, {} bytes", t.elapsed(), map.len());
    let [x0, z0, ..] = info.bounds.unwrap();
    let t = std::time::Instant::now();
    let preview = bridge.world_preview(x0, z0, size, 512).unwrap();
    eprintln!(
        "preview {size} at 512: {:?}, {} bytes",
        t.elapsed(),
        preview.len()
    );
    let req = SceneRequest {
        x0,
        z0,
        size,
        game_dir: None,
        catalog_path: Some(path),
    };
    let t = std::time::Instant::now();
    let scene = bridge.world_scene(&req, &|_, _| {}).unwrap();
    let d = decode_scene(&scene);
    eprintln!(
        "scene {size}: {:?}, {} voxels, {} states, {} MB",
        t.elapsed(),
        d.header["voxels"],
        d.header["states"],
        scene.len() / 1_000_000
    );
}
