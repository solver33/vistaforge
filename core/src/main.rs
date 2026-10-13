use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};

use vistaforge_core::catalog::Catalog;
use vistaforge_core::level::{write_level_dat, LevelSettings};
use vistaforge_core::{anvil, testworld, validate};

const USAGE: &str = "\
Usage:
  vistaforge testworld --catalog <catalog.json> --out <folder> [--name <world name>] [--size <blocks>] [--seed <n>]

Writes a Minecraft 1.20.1 test world to <folder>/<world name>. Copy that folder into the
game's `saves` folder to open it.";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("testworld") => test_world(&args[1..]),
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

fn test_world(args: &[String]) -> Result<()> {
    let catalog_path = PathBuf::from(flag(args, "--catalog").context("--catalog is required")?);
    let out = PathBuf::from(flag(args, "--out").context("--out is required")?);
    let name = flag(args, "--name").unwrap_or_else(|| "Vistaforge Test".into());
    let size: i32 = flag(args, "--size")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(256);
    let seed: u64 = flag(args, "--seed")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(20261013);
    if size < 32 || size % 16 != 0 || size > 4096 {
        bail!("--size must be a multiple of 16 between 32 and 4096");
    }

    let started = Instant::now();
    let catalog = Catalog::load(&catalog_path)?;
    println!("Catalog: {} blocks", catalog.len());

    let test = testworld::generate(&catalog, &testworld::TestWorldOptions { size, seed })?;
    let states = validate(&test.world, &catalog)?;
    println!(
        "Generated {size}x{size}: {} chunks, {states} distinct block states, {} trees, {} plants",
        test.world.chunks.len(),
        test.trees,
        test.plants
    );

    let dir = out.join(&name);
    if dir.exists() {
        bail!(
            "{} already exists; pick another --name or remove it",
            dir.display()
        );
    }
    let regions = anvil::write_regions(&test.world, &dir)?;
    let half = size as f64 / 2.0;
    write_level_dat(
        &LevelSettings {
            name: name.clone(),
            seed: seed as i64,
            spawn: test.spawn,
            border_center: (half, half),
            border_size: size as f64,
            creative: true,
        },
        &dir,
    )?;
    println!(
        "Wrote {} ({regions} region file(s), spawn {:?}) in {:.1}s",
        dir.display(),
        test.spawn,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
