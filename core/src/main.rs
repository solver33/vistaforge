use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::{Context, Result};

use vistaforge_core::catalog::Catalog;
use vistaforge_core::engine::{Generator, Preview};
use vistaforge_core::png;
use vistaforge_core::settings::{WorldHeight, WorldSettings};
use vistaforge_core::terrain::{presets, Quality};

const USAGE: &str = "\
Usage:
  vistaforge presets
  vistaforge generate --catalog <catalog.json> --out <saves folder> [map options] [--overwrite]
  vistaforge preview  --catalog <catalog.json> --png <file.png> [--res <pixels>] [map options]
  vistaforge testworld --catalog <catalog.json> --out <saves folder> [--name <name>] [--size <blocks>] [--seed <n>]

Map options:
  --settings <file.json>   settings saved by the app (a WorldSettings JSON)
  --preset <id>            start from a preset (default: plains)
  --name <world name>  --size <blocks>  --seed <n>
  --height standard|tall   world top at Y 319 (default) or Y 2031 (adds a data pack)

`generate` writes a Minecraft 1.20.1 world to <saves folder>/<world name>; point --out at the
game's `saves` folder or copy the world folder there.";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("presets") => {
            for p in presets::all() {
                println!("{:<20} {} — {}", p.id, p.name, p.description);
            }
            Ok(())
        }
        Some("generate") => generate(&args[1..], None),
        Some("testworld") => generate(&args[1..], Some(256)),
        Some("preview") => preview(&args[1..]),
        _ => {
            println!("{USAGE}");
            Ok(())
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn settings(args: &[String], default_size: Option<u32>) -> Result<WorldSettings> {
    let mut s = match flag(args, "--settings") {
        Some(path) => serde_json::from_str(
            &std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?,
        )
        .with_context(|| format!("parsing {path}"))?,
        None => {
            let id = flag(args, "--preset").unwrap_or_else(|| "plains".into());
            let preset = presets::find(&id)
                .with_context(|| format!("no preset {id}; see `vistaforge presets`"))?;
            preset.settings("Vistaforge Test", default_size.unwrap_or(1024), 20261013)
        }
    };
    if let Some(name) = flag(args, "--name") {
        s.name = name;
    }
    if let Some(size) = flag(args, "--size") {
        s.size = size.parse()?;
    } else if let Some(size) = default_size {
        s.size = size;
    }
    if let Some(seed) = flag(args, "--seed") {
        s.seed = seed.parse()?;
    }
    if let Some(height) = flag(args, "--height") {
        s.world_height = match height.as_str() {
            "standard" => WorldHeight::Standard,
            "tall" => WorldHeight::Tall,
            other => anyhow::bail!("--height must be standard or tall, not {other}"),
        };
    }
    s.validate().map_err(anyhow::Error::msg)?;
    Ok(s)
}

fn load_catalog(args: &[String]) -> Result<Catalog> {
    let path = PathBuf::from(flag(args, "--catalog").context("--catalog is required")?);
    let catalog = Catalog::load(&path)?;
    println!("Catalog: {} blocks", catalog.len());
    Ok(catalog)
}

fn generate(args: &[String], default_size: Option<u32>) -> Result<()> {
    let out = PathBuf::from(flag(args, "--out").context("--out is required")?);
    let settings = settings(args, default_size)?;
    let catalog = load_catalog(args)?;
    let started = Instant::now();
    let generator = Generator::new(&settings, &catalog, Quality::Full)?;
    println!(
        "Prepared {}×{} '{}' in {:.1}s",
        settings.size,
        settings.size,
        settings.preset,
        started.elapsed().as_secs_f64()
    );
    let last = std::sync::Mutex::new(0usize);
    let report = generator.export(
        &out,
        args.iter().any(|a| a == "--overwrite"),
        &|p| {
            let pct = p.chunks_done * 100 / p.chunks_total.max(1);
            let mut last = last.lock().unwrap();
            if pct >= *last + 10 {
                *last = pct;
                println!("  {pct}%");
            }
        },
        &AtomicBool::new(false),
    )?;
    println!(
        "Wrote {} ({} chunks, {} region files, {} block states, {} trees, spawn {:?}) in {:.1}s",
        report.world_dir.display(),
        report.chunks,
        report.regions,
        report.block_states,
        report.trees,
        report.spawn,
        report.seconds
    );
    Ok(())
}

fn preview(args: &[String]) -> Result<()> {
    let out = PathBuf::from(flag(args, "--png").context("--png is required")?);
    let res: u32 = flag(args, "--res")
        .map(|r| r.parse())
        .transpose()?
        .unwrap_or(512);
    let settings = settings(args, None)?;
    let catalog = load_catalog(args)?;
    let started = Instant::now();
    let generator = Generator::new(&settings, &catalog, Quality::Preview)?;
    let preview = generator.preview(res);
    println!(
        "Preview {}×{} in {:.2}s, {} trees",
        preview.res,
        preview.res,
        started.elapsed().as_secs_f64(),
        preview.trees.len()
    );
    std::fs::write(
        &out,
        png::encode_rgb(preview.res, preview.res, &shade(&preview))?,
    )?;
    println!("Wrote {}", out.display());
    Ok(())
}

/// Colours lit from the north-west, with trees as dark crowns.
fn shade(p: &Preview) -> Vec<[u8; 3]> {
    let n = p.res as usize;
    let h = |i: usize, j: usize| p.heights[j.min(n - 1) * n + i.min(n - 1)];
    let mut px: Vec<[u8; 3]> = (0..n * n)
        .map(|k| {
            let (i, j) = (k % n, k / n);
            let dx = h(i + 1, j) - h(i.saturating_sub(1), j);
            let dz = h(i, j + 1) - h(i, j.saturating_sub(1));
            let light = if p.water[k].is_some() {
                1.0
            } else {
                (1.0 - (dx + dz) / p.step * 0.35).clamp(0.55, 1.35)
            };
            p.colors[k].map(|c| (c as f32 * light).min(255.0) as u8)
        })
        .collect();
    for t in &p.trees {
        let r = (t.crown / p.step).max(1.0);
        let (ci, cj) = (t.x / p.step, t.z / p.step);
        let (i0, i1) = (
            (ci - r).floor().max(0.0) as usize,
            ((ci + r).ceil() as usize).min(n),
        );
        let (j0, j1) = (
            (cj - r).floor().max(0.0) as usize,
            ((cj + r).ceil() as usize).min(n),
        );
        for j in j0..j1 {
            for i in i0..i1 {
                let d = ((i as f32 + 0.5 - ci).powi(2) + (j as f32 + 0.5 - cj).powi(2)).sqrt();
                if d <= r {
                    let shade = 0.75 + 0.25 * (1.0 - d / r);
                    px[j * n + i] = [
                        (52.0 * shade) as u8,
                        (92.0 * shade) as u8,
                        (36.0 * shade) as u8,
                    ];
                }
            }
        }
    }
    px
}
