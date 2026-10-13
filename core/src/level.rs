//! Writes `level.dat` for a Minecraft 1.20.1 world whose generator is an empty (void) superflat,
//! so everything outside the generated square stays empty.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::anvil::DATA_VERSION;
use crate::nbt::{self, Compound, Tag};

pub struct LevelSettings {
    pub name: String,
    pub seed: i64,
    pub spawn: (i32, i32, i32),
    /// World border centre (x, z) and side length in blocks.
    pub border_center: (f64, f64),
    pub border_size: f64,
    pub creative: bool,
}

fn dimension(kind: &str, generator: Compound) -> Compound {
    Compound::new()
        .with("type", kind)
        .with("generator", generator)
}

fn world_gen_settings(seed: i64) -> Compound {
    let void_flat = Compound::new().with("type", "minecraft:flat").with(
        "settings",
        Compound::new()
            .with("biome", "minecraft:plains")
            .with("features", false)
            .with("lakes", false)
            .with("layers", Tag::List(vec![]))
            .with("structure_overrides", Tag::List(vec![])),
    );
    let nether = Compound::new()
        .with("type", "minecraft:noise")
        .with("settings", "minecraft:nether")
        .with(
            "biome_source",
            Compound::new()
                .with("type", "minecraft:multi_noise")
                .with("preset", "minecraft:nether"),
        );
    let end = Compound::new()
        .with("type", "minecraft:noise")
        .with("settings", "minecraft:end")
        .with(
            "biome_source",
            Compound::new().with("type", "minecraft:the_end"),
        );
    Compound::new()
        .with("seed", seed)
        .with("generate_features", false)
        .with("bonus_chest", false)
        .with(
            "dimensions",
            Compound::new()
                .with(
                    "minecraft:overworld",
                    dimension("minecraft:overworld", void_flat),
                )
                .with(
                    "minecraft:the_nether",
                    dimension("minecraft:the_nether", nether),
                )
                .with("minecraft:the_end", dimension("minecraft:the_end", end)),
        )
}

fn game_rules() -> Compound {
    let mut rules = Compound::new();
    for (k, v) in [
        // Spawn exactly at the spawn point instead of somewhere within 10 blocks.
        ("spawnRadius", "0"),
        // Keep the test world calm and lit so blocks and lighting are easy to inspect.
        ("doDaylightCycle", "false"),
        ("doWeatherCycle", "false"),
        ("doMobSpawning", "false"),
        ("doFireTick", "false"),
        ("randomTickSpeed", "0"),
    ] {
        rules.put(k, v);
    }
    rules
}

pub fn level_tag(s: &LevelSettings) -> Compound {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let data = Compound::new()
        .with("DataVersion", DATA_VERSION)
        .with("version", 19133i32)
        .with(
            "Version",
            Compound::new()
                .with("Id", DATA_VERSION)
                .with("Name", "1.20.1")
                .with("Series", "main")
                .with("Snapshot", false),
        )
        .with("LevelName", s.name.as_str())
        .with("GameType", if s.creative { 1i32 } else { 0i32 })
        .with("hardcore", false)
        .with("Difficulty", 0i8)
        .with("DifficultyLocked", false)
        .with("allowCommands", true)
        .with("initialized", true)
        .with("LastPlayed", now_ms)
        .with("Time", 0i64)
        .with("DayTime", 6000i64)
        .with("SpawnX", s.spawn.0)
        .with("SpawnY", s.spawn.1)
        .with("SpawnZ", s.spawn.2)
        .with("SpawnAngle", 0f32)
        .with("raining", false)
        .with("rainTime", 0i32)
        .with("thundering", false)
        .with("thunderTime", 0i32)
        .with("clearWeatherTime", 0i32)
        .with("BorderCenterX", s.border_center.0)
        .with("BorderCenterZ", s.border_center.1)
        .with("BorderSize", s.border_size)
        .with("BorderSizeLerpTarget", s.border_size)
        .with("BorderSizeLerpTime", 0i64)
        .with("BorderSafeZone", 5f64)
        .with("BorderWarningBlocks", 5f64)
        .with("BorderWarningTime", 15f64)
        .with("BorderDamagePerBlock", 0.2f64)
        .with("GameRules", game_rules())
        .with(
            "DataPacks",
            Compound::new()
                .with("Enabled", Tag::strings(&["vanilla"]))
                .with("Disabled", Tag::List(vec![])),
        )
        .with("enabled_features", Tag::strings(&["minecraft:vanilla"]))
        .with("WorldGenSettings", world_gen_settings(s.seed));
    Compound::new().with("Data", data)
}

pub fn write_level_dat(s: &LevelSettings, world_dir: &Path) -> Result<()> {
    fs::create_dir_all(world_dir)?;
    let raw = nbt::to_bytes("", &level_tag(s));
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&raw)?;
    fs::write(world_dir.join("level.dat"), gz.finish()?)?;
    Ok(())
}
