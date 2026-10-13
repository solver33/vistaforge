//! Draws side views of every tree species at a range of sizes, with and without leaves, from
//! the shapes of the Conquest pieces they are built of. With `--world`, also writes a small
//! Minecraft world with the same trees planted in rows, to walk around in the game. With
//! `--scene x,z,size`, also draws that square of a generated map (`--preset`, default plains)
//! as the generator builds it, trees and all.
//!
//! ```text
//! cargo run --release --manifest-path tools/tree-render/Cargo.toml -- \
//!     --catalog catalog.json --out renders/ [--species oak,birch] [--seed n] [--yaw degrees]
//!     [--scale px] [--world <saves folder>] [--scene x,z,size [--preset id]]
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use vistaforge_core::anvil::{chunk_tag, compress_chunk, RegionWriter};
use vistaforge_core::catalog::Catalog;
use vistaforge_core::column::Rect;
use vistaforge_core::engine::{validate_palette, Generator};
use vistaforge_core::level::{write_level_dat, LevelSettings};
use vistaforge_core::sink::{BlockId, BlockSink, Blocks, Palette};
use vistaforge_core::terrain::{presets, Quality};
use vistaforge_core::trees::render::render;
use vistaforge_core::trees::render::{gallery, View};
use vistaforge_core::trees::{Species, TreeRequest, Trees};
use vistaforge_core::world::Area;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let catalog = flag(&args, "--catalog").context("--catalog <catalog.json> is required")?;
    let out = PathBuf::from(flag(&args, "--out").unwrap_or_else(|| "tree-renders".into()));
    let seed = flag(&args, "--seed")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1);
    let mut view = View::default();
    if let Some(yaw) = flag(&args, "--yaw") {
        view.yaw = yaw.parse()?;
    }
    if let Some(scale) = flag(&args, "--scale") {
        view.scale = scale.parse()?;
    }
    let species = match flag(&args, "--species") {
        Some(list) => list
            .split(',')
            .map(|s| serde_json::from_value(serde_json::Value::String(s.trim().into())))
            .collect::<Result<Vec<Species>, _>>()
            .context("--species takes a list like oak,birch,spruce,pine,willow,dead,bush")?,
        None => Species::ALL.to_vec(),
    };
    let catalog = Catalog::load(Path::new(&catalog))?;
    if let Some(saves) = flag(&args, "--world") {
        let dir = gallery_world(&catalog, &species, seed, Path::new(&saves))?;
        println!("{}", dir.display());
    }
    std::fs::create_dir_all(&out)?;
    if let Some(scene) = flag(&args, "--scene") {
        let n: Vec<i32> = scene
            .split(',')
            .map(|v| v.trim().parse())
            .collect::<Result<_, _>>()
            .context("--scene takes x,z,size")?;
        let &[x, z, size] = n.as_slice() else {
            anyhow::bail!("--scene takes x,z,size");
        };
        let preset = flag(&args, "--preset").unwrap_or_else(|| "plains".into());
        let img = scene_view(
            &catalog,
            &preset,
            seed,
            Rect::new(x, z, x + size, z + size),
            &view,
        )?;
        let path = out.join(format!("scene-{preset}-{x}-{z}.png"));
        std::fs::write(&path, img.png()?)?;
        println!("{} ({}×{})", path.display(), img.width, img.height);
    }
    for (species, img) in gallery(&catalog, &species, seed, &view)? {
        let path = out.join(format!("trees-{species}.png"));
        std::fs::write(&path, img.png()?)?;
        println!("{} ({}×{})", path.display(), img.width, img.height);
    }
    Ok(())
}

/// Sizes planted in each row of the gallery world, west to east.
const SIZES: [f32; 6] = [0.05, 0.2, 0.4, 0.6, 0.8, 1.0];
/// Blocks between trees in the gallery world.
const SPACING: i32 = 32;

/// Writes `<saves>/Vistaforge Trees`: one row of trees per species (north to south, in the
/// order given), growing from a sapling in the west to a giant in the east, on gently uneven
/// grass with Conquest grass layers. The player starts at the north-west corner.
fn gallery_world(
    catalog: &Catalog,
    species: &[Species],
    seed: u64,
    saves: &Path,
) -> Result<PathBuf> {
    let palette = Palette::new();
    let blocks = Blocks::new(catalog, &palette);
    let trees = Trees::new(&blocks)?;
    let grass = blocks.id("minecraft:grass_block", &[("snowy", "false")])?;
    let dirt = blocks.id("minecraft:dirt", &[])?;
    let layers: Vec<BlockId> = (1..=7)
        .map(|l| {
            blocks.id(
                "conquest:grass_block_layer",
                &[("layers", &l.to_string()), ("waterlogged", "false")],
            )
        })
        .collect::<Result<_>>()?;

    // Whole chunks, with a margin of SPACING / 2 round the trees.
    let width = (SIZES.len() as i32 * SPACING + 15) / 16 * 16;
    let depth = (species.len() as i32 * SPACING + 15) / 16 * 16;
    let rect = Rect::new(0, 0, width, depth);
    let mut area = Area::new(rect, &palette);
    let ground = |x: i32, z: i32| {
        64.0 + 0.7 * ((x as f32 * 0.21).sin() + (z as f32 * 0.17).cos())
            + 0.4 * ((x + z) as f32 * 0.09).sin()
    };
    for (x, z) in rect.columns() {
        let g = ground(x, z);
        let top = g.floor() as i32;
        area.fill_column(x, z, 56, top, dirt);
        area.set(x, top, z, grass);
        let l = ((g - g.floor()) * 8.0) as usize;
        if l > 0 {
            area.set(x, top + 1, z, layers[l - 1]);
        }
    }
    for (row, &kind) in species.iter().enumerate() {
        for (col, &size) in SIZES.iter().enumerate() {
            let (x, z) = (
                col as i32 * SPACING + SPACING / 2,
                row as i32 * SPACING + SPACING / 2,
            );
            // The trunk starts in the layer cell when there is one, as `ColumnInfo::ground`.
            let g = ground(x, z);
            let y = g.floor() as i32 + 1;
            let req = TreeRequest {
                species: kind,
                x,
                y,
                z,
                size,
                seed: seed
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add((row * 16 + col) as u64),
            };
            trees.build(&req, &mut area);
        }
    }
    validate_palette(&palette, catalog)?;

    let name = "Vistaforge Trees";
    let world_dir = saves.join(name);
    if world_dir.exists() {
        std::fs::remove_dir_all(&world_dir)?;
    }
    let region_dir = world_dir.join("region");
    std::fs::create_dir_all(&region_dir)?;
    let mut region = RegionWriter::new();
    let mut scratch = Vec::new();
    for cz in 0..depth / 16 {
        for cx in 0..width / 16 {
            let tag = chunk_tag(
                cx,
                cz,
                &area,
                &palette,
                &|_, _| "minecraft:plains",
                &mut scratch,
            );
            region.add(cx, cz, &compress_chunk(&tag)?)?;
        }
    }
    region.write(&region_dir, 0, 0)?;
    let spawn_y = area.top(2, 2).unwrap_or(64) + 1;
    write_level_dat(
        &LevelSettings {
            name: name.into(),
            seed: seed as i64,
            spawn: (2, spawn_y, 2),
            border_center: (width as f64 / 2.0, depth as f64 / 2.0),
            border_size: width.max(depth) as f64,
            creative: true,
        },
        &world_dir,
    )?;
    Ok(world_dir)
}

/// A square of a generated map seen from above at an angle, as the generator builds it.
fn scene_view(
    catalog: &Catalog,
    preset: &str,
    seed: u64,
    rect: Rect,
    view: &View,
) -> Result<vistaforge_core::trees::render::Image> {
    let preset = presets::find(preset).with_context(|| format!("no preset {preset}"))?;
    let size = ((rect.x1.max(rect.z1) + 511) / 512 * 512) as u32;
    let settings = preset.settings("Tree scene", size, seed);
    let generator = Generator::new(&settings, catalog, Quality::Full)?;
    let piece = generator.generate(rect);
    let tops: Vec<i32> = rect
        .columns()
        .filter_map(|(x, z)| piece.area.top(x, z))
        .collect();
    let low = tops.iter().copied().min().unwrap_or(60);
    let high = tops.iter().copied().max().unwrap_or(100);
    let view = View {
        pitch: 32.0,
        scale: view.scale.min(8.0),
        ..*view
    };
    Ok(render(&piece.area, catalog, rect, low - 6, high + 1, &view))
}
