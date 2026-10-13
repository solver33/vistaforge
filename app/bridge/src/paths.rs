//! World names, and finding the player's Minecraft folders.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Serialize;

/// Names Windows reserves for devices; a folder can't be called this, with any extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Checks that `name` works as a world folder name on Windows. The UI repeats these rules
/// (`app/src/lib/names.ts`) to explain problems while typing.
pub fn check_world_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("the world needs a name".into());
    }
    if name.chars().count() > 80 {
        return Err("the world name is longer than 80 characters".into());
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || "<>:\"/\\|?*".contains(*c))
    {
        return Err(format!("the world name can't contain {c:?}"));
    }
    if name.ends_with('.') || name.ends_with(' ') || name.starts_with(' ') {
        return Err("the world name can't start with a space or end with a space or a dot".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("Windows doesn't allow a folder named {name}"));
    }
    Ok(())
}

pub fn world_exists(saves_dir: &Path, name: &str) -> bool {
    check_world_name(name).is_ok() && saves_dir.join(name).exists()
}

/// A Minecraft game folder found on this computer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Instance {
    /// "CurseForge", "Minecraft Launcher", …
    pub launcher: String,
    /// The instance or profile name; the folder name for the vanilla launcher.
    pub name: String,
    pub game_dir: PathBuf,
    pub saves: Option<PathBuf>,
    /// `vistaforge/catalog.json`, written by the exporter mod.
    pub catalog: Option<PathBuf>,
}

/// Where a launcher keeps its game folders.
pub struct Root {
    pub launcher: &'static str,
    pub dir: PathBuf,
    /// `false`: `dir` is a game folder. `true`: each folder inside is an instance, whose game
    /// folder is the instance folder itself or its `.minecraft` / `minecraft`.
    pub instances: bool,
}

/// The usual launcher folders on this computer.
pub fn roots() -> Vec<Root> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut roots = Vec::new();
    let mut add = |launcher, dir: Option<PathBuf>, sub: &str, instances| {
        if let Some(dir) = dir {
            roots.push(Root {
                launcher,
                dir: dir.join(sub),
                instances,
            });
        }
    };
    if cfg!(windows) {
        add(
            "CurseForge",
            env("USERPROFILE"),
            "curseforge/minecraft/Instances",
            true,
        );
        add("Minecraft Launcher", env("APPDATA"), ".minecraft", false);
        add(
            "Prism Launcher",
            env("APPDATA"),
            "PrismLauncher/instances",
            true,
        );
        add("Modrinth App", env("APPDATA"), "ModrinthApp/profiles", true);
        add(
            "Modrinth App",
            env("APPDATA"),
            "com.modrinth.theseus/profiles",
            true,
        );
        add("ATLauncher", env("APPDATA"), "ATLauncher/instances", true);
        add("MultiMC", env("USERPROFILE"), "MultiMC/instances", true);
    } else {
        add("Minecraft Launcher", env("HOME"), ".minecraft", false);
        add(
            "Prism Launcher",
            env("HOME"),
            ".local/share/PrismLauncher/instances",
            true,
        );
        add(
            "CurseForge",
            env("HOME"),
            "curseforge/minecraft/Instances",
            true,
        );
    }
    roots
}

pub fn detect() -> Vec<Instance> {
    detect_in(&roots())
}

fn modified(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Looks like a Conquest Reforged pack, going by its name.
fn looks_like_conquest(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("conquest")
        || lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "cr")
}

/// Finds game folders with a `saves` folder or a Vistaforge catalog. Ones with a catalog come
/// first, then ones that look like Conquest packs, then the most recently used.
pub fn detect_in(roots: &[Root]) -> Vec<Instance> {
    let mut found = Vec::new();
    for root in roots {
        let candidates: Vec<(String, PathBuf)> = if root.instances {
            let Ok(entries) = std::fs::read_dir(&root.dir) else {
                continue;
            };
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let inst = e.path();
                    let game = [".minecraft", "minecraft"]
                        .iter()
                        .map(|s| inst.join(s))
                        .find(|g| g.is_dir())
                        .unwrap_or(inst);
                    (name, game)
                })
                .collect()
        } else {
            let name = root
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            vec![(name, root.dir.clone())]
        };
        for (name, game_dir) in candidates {
            let saves = Some(game_dir.join("saves")).filter(|p| p.is_dir());
            let catalog =
                Some(game_dir.join("vistaforge").join("catalog.json")).filter(|p| p.is_file());
            if saves.is_some() || catalog.is_some() {
                found.push(Instance {
                    launcher: root.launcher.to_string(),
                    name,
                    game_dir,
                    saves,
                    catalog,
                });
            }
        }
    }
    found.sort_by_key(|i| {
        let used = i
            .catalog
            .as_deref()
            .or(i.saves.as_deref())
            .map(modified)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        (
            i.catalog.is_none(),
            !looks_like_conquest(&i.name),
            std::cmp::Reverse(used),
        )
    });
    found
}

/// Opens `path` in the system file manager.
pub fn reveal(path: &Path) -> Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .with_context(|| format!("opening {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_names() {
        assert!(check_world_name("Conquest Valley 2").is_ok());
        assert!(check_world_name("Мир").is_ok());
        for bad in [
            "",
            "  ",
            "a/b",
            "a:b",
            "what?",
            "trailing.",
            " lead",
            "CON",
            "nul.txt",
            "x\ty",
        ] {
            assert!(check_world_name(bad).is_err(), "{bad:?}");
        }
        assert!(check_world_name(&"x".repeat(81)).is_err());
    }

    #[test]
    fn finds_instances_and_prefers_ones_with_a_catalog() {
        let base = std::env::temp_dir().join(format!("vistaforge-paths-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let cf = base.join("Instances");
        std::fs::create_dir_all(cf.join("Vanilla Fun/saves")).unwrap();
        std::fs::create_dir_all(cf.join("CR-Modpack-fabric-1.20.1-1.7.0/saves")).unwrap();
        std::fs::create_dir_all(cf.join("CR-Modpack-fabric-1.20.1-1.7.0/vistaforge")).unwrap();
        std::fs::write(
            cf.join("CR-Modpack-fabric-1.20.1-1.7.0/vistaforge/catalog.json"),
            "{}",
        )
        .unwrap();
        std::fs::create_dir_all(cf.join("Empty")).unwrap();
        let prism = base.join("prism");
        std::fs::create_dir_all(prism.join("Conquest/.minecraft/saves")).unwrap();
        let vanilla = base.join(".minecraft");
        std::fs::create_dir_all(vanilla.join("saves")).unwrap();

        let roots = [
            Root {
                launcher: "Minecraft Launcher",
                dir: vanilla.clone(),
                instances: false,
            },
            Root {
                launcher: "CurseForge",
                dir: cf.clone(),
                instances: true,
            },
            Root {
                launcher: "Prism Launcher",
                dir: prism.clone(),
                instances: true,
            },
            Root {
                launcher: "Missing",
                dir: base.join("nope"),
                instances: true,
            },
        ];
        let found = detect_in(&roots);
        let names: Vec<&str> = found.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert_eq!(names[0], "CR-Modpack-fabric-1.20.1-1.7.0");
        assert!(found[0].catalog.is_some());
        assert_eq!(names[1], "Conquest");
        assert_eq!(found[1].game_dir, prism.join("Conquest/.minecraft"));
        assert_eq!(
            found[1].saves,
            Some(prism.join("Conquest/.minecraft/saves"))
        );
        assert!(names.contains(&"Vanilla Fun") && names.contains(&".minecraft"));
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn conquest_names() {
        assert!(looks_like_conquest("CR-Modpack-fabric-1.20.1-1.7.0"));
        assert!(looks_like_conquest("My Conquest Reforged"));
        assert!(!looks_like_conquest("Create Above and Beyond"));
    }
}
