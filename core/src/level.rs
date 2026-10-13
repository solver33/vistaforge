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
use crate::settings::WorldHeight;
use crate::world::MIN_Y;

/// The world data pack that raises the overworld's height limit in tall worlds.
pub const HEIGHT_PACK: &str = "vistaforge-height";

pub struct LevelSettings {
    pub name: String,
    pub seed: i64,
    pub spawn: (i32, i32, i32),
    /// World border centre (x, z) and side length in blocks.
    pub border_center: (f64, f64),
    pub border_size: f64,
    pub creative: bool,
    /// A tall world gets [`HEIGHT_PACK`], which the game needs to keep blocks above Y 319.
    pub height: WorldHeight,
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
                .with(
                    "Enabled",
                    if s.height != WorldHeight::Standard {
                        Tag::strings(&["vanilla", &format!("file/{HEIGHT_PACK}")])
                    } else {
                        Tag::strings(&["vanilla"])
                    },
                )
                .with("Disabled", Tag::List(vec![])),
        )
        .with("enabled_features", Tag::strings(&["minecraft:vanilla"]))
        .with("WorldGenSettings", world_gen_settings(s.seed));
    Compound::new().with("Data", data)
}

/// Vanilla 1.20.1's overworld dimension type with `height` and `logical_height` replaced.
fn overworld_type(height: WorldHeight) -> String {
    let height = height.height();
    format!(
        r##"{{
  "ambient_light": 0.0,
  "bed_works": true,
  "coordinate_scale": 1.0,
  "effects": "minecraft:overworld",
  "has_ceiling": false,
  "has_raids": true,
  "has_skylight": true,
  "height": {height},
  "infiniburn": "#minecraft:infiniburn_overworld",
  "logical_height": {height},
  "min_y": {MIN_Y},
  "monster_spawn_block_light_limit": 0,
  "monster_spawn_light_level": {{
    "type": "minecraft:uniform",
    "value": {{
      "max_inclusive": 7,
      "min_inclusive": 0
    }}
  }},
  "natural": true,
  "piglin_safe": false,
  "respawn_anchor_works": false,
  "ultrawarm": false
}}
"##
    )
}

/// Writes `datapacks/vistaforge-height`, which overrides the overworld's dimension type so the
/// game keeps blocks up to the top of a tall world. Since 1.19 a dimension type can't be written
/// inline in `level.dat`; overriding `minecraft:overworld` keeps the world's own entry valid.
fn write_height_pack(world_dir: &Path, height: WorldHeight) -> Result<()> {
    let pack = world_dir.join("datapacks").join(HEIGHT_PACK);
    let types = pack.join("data/minecraft/dimension_type");
    fs::create_dir_all(&types)?;
    fs::write(
        pack.join("pack.mcmeta"),
        format!(
            "{{\n  \"pack\": {{\n    \"pack_format\": 15,\n    \"description\": \"Vistaforge: overworld from Y {MIN_Y} to {}\"\n  }}\n}}\n",
            height.max_y()
        ),
    )?;
    fs::write(types.join("overworld.json"), overworld_type(height))?;
    Ok(())
}

pub fn write_level_dat(s: &LevelSettings, world_dir: &Path) -> Result<()> {
    fs::create_dir_all(world_dir)?;
    if s.height != WorldHeight::Standard {
        write_height_pack(world_dir, s.height)?;
    }
    let raw = nbt::to_bytes("", &level_tag(s));
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&raw)?;
    fs::write(world_dir.join("level.dat"), gz.finish()?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(height: WorldHeight) -> LevelSettings {
        LevelSettings {
            name: "Test".into(),
            seed: 1,
            spawn: (0, 70, 0),
            border_center: (0.0, 0.0),
            border_size: 256.0,
            creative: true,
            height,
        }
    }

    fn enabled_packs(s: &LevelSettings) -> Vec<String> {
        let root = level_tag(s);
        let Some(Tag::Compound(data)) = root.get("Data") else {
            panic!()
        };
        let Some(Tag::Compound(packs)) = data.get("DataPacks") else {
            panic!()
        };
        let Some(Tag::List(enabled)) = packs.get("Enabled") else {
            panic!()
        };
        enabled
            .iter()
            .map(|t| match t {
                Tag::String(s) => s.clone(),
                _ => panic!(),
            })
            .collect()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vistaforge-level-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn standard_worlds_use_vanilla_data_only() {
        let s = settings(WorldHeight::Standard);
        assert_eq!(enabled_packs(&s), ["vanilla"]);
        let dir = temp_dir("standard");
        write_level_dat(&s, &dir).unwrap();
        assert!(dir.join("level.dat").is_file());
        assert!(!dir.join("datapacks").exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn tall_worlds_carry_the_height_pack() {
        let s = settings(WorldHeight::Tall);
        assert_eq!(enabled_packs(&s), ["vanilla", "file/vistaforge-height"]);
        let dir = temp_dir("tall");
        write_level_dat(&s, &dir).unwrap();
        let pack = dir.join("datapacks").join(HEIGHT_PACK);
        let meta: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(pack.join("pack.mcmeta")).unwrap()).unwrap();
        assert_eq!(meta["pack"]["pack_format"], 15);
        let path = pack.join("data/minecraft/dimension_type/overworld.json");
        let t: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(t["min_y"], -64);
        assert_eq!(t["height"], 2096);
        assert_eq!(t["logical_height"], 2096);
        assert_eq!(t["effects"], "minecraft:overworld");
        assert_eq!(t["monster_spawn_light_level"]["value"]["max_inclusive"], 7);
        fs::remove_dir_all(dir).ok();
    }
}
