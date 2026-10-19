//! The game's own block models and textures, read from the player's install: the enabled
//! resource packs, the mod jars and the Minecraft jar, in the order the game stacks them. The
//! app ships none of these files.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::Serialize;
use zip::ZipArchive;

/// Where a file comes from.
enum Source {
    Zip(Mutex<ZipArchive<BufReader<File>>>),
    Dir(PathBuf),
}

/// One file of the stack: the source that has it, and its name there.
struct Entry {
    source: usize,
    name: String,
}

/// The files under `assets/` of every source, the highest-priority copy of each path.
pub struct Assets {
    sources: Vec<Source>,
    index: HashMap<String, Entry>,
    pub found: Found,
}

/// What was found, for the UI.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Found {
    pub game_dir: Option<PathBuf>,
    pub minecraft_jar: Option<PathBuf>,
    pub mods: usize,
    pub packs: usize,
}

/// The files of a game folder that hold assets, lowest priority first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    pub minecraft_jar: Option<PathBuf>,
    pub mods: Vec<PathBuf>,
    /// Enabled resource packs from `options.txt`, lowest priority first.
    pub packs: Vec<PathBuf>,
    /// Names of every enabled pack, to switch on packs built into mod jars.
    pub enabled: Vec<String>,
}

fn wanted(name: &str) -> bool {
    name.starts_with("assets/")
        && (name.ends_with(".json") || name.ends_with(".png") || name.ends_with(".mcmeta"))
}

impl Assets {
    /// The asset stack of a game folder (the one holding `mods`, `resourcepacks` and
    /// `options.txt`) for Minecraft `version`.
    pub fn open(game_dir: &Path, version: &str) -> Result<Self> {
        let stack = stack(game_dir, version);
        let mut assets = Self::from_stack(&stack)?;
        assets.found.game_dir = Some(game_dir.to_path_buf());
        Ok(assets)
    }

    pub fn from_stack(stack: &Stack) -> Result<Self> {
        let mut assets = Self {
            sources: Vec::new(),
            index: HashMap::new(),
            found: Found {
                game_dir: None,
                minecraft_jar: stack.minecraft_jar.clone(),
                mods: 0,
                packs: 0,
            },
        };
        let enabled: HashSet<String> = stack.enabled.iter().map(|s| s.to_lowercase()).collect();
        // Later sources override earlier ones, as in the game.
        if let Some(jar) = &stack.minecraft_jar {
            assets.add(jar, &enabled)?;
        }
        for jar in &stack.mods {
            // A broken jar shouldn't stop the others from being read.
            if assets.add(jar, &enabled).is_ok() {
                assets.found.mods += 1;
            }
        }
        for pack in &stack.packs {
            if assets.add(pack, &enabled).is_ok() {
                assets.found.packs += 1;
            }
        }
        Ok(assets)
    }

    fn add(&mut self, path: &Path, enabled: &HashSet<String>) -> Result<()> {
        let source = self.sources.len();
        let mut names = Vec::new();
        if path.is_dir() {
            walk(path, path, &mut names);
            self.sources.push(Source::Dir(path.to_path_buf()));
        } else {
            let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
            let zip = ZipArchive::new(BufReader::new(file))
                .with_context(|| format!("reading {}", path.display()))?;
            names.extend(zip.file_names().map(String::from));
            self.sources.push(Source::Zip(Mutex::new(zip)));
        }
        // The source's own assets first, then packs built into a mod jar that the player has
        // switched on (`resourcepacks/<name>/assets/…`), which go over them.
        let mut built_in = Vec::new();
        for name in names {
            if wanted(&name) {
                self.index.insert(name.clone(), Entry { source, name });
            } else if let Some(rest) = name.strip_prefix("resourcepacks/") {
                if let Some((pack, inner)) = rest.split_once('/') {
                    if wanted(inner) && enabled.iter().any(|e| e.contains(&pack.to_lowercase())) {
                        built_in.push((inner.to_string(), name.clone()));
                    }
                }
            }
        }
        for (key, name) in built_in {
            self.index.insert(key, Entry { source, name });
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn has(&self, path: &str) -> bool {
        self.index.contains_key(path)
    }

    /// The bytes of `assets/…/file`, from the source that wins.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let entry = self.index.get(path)?;
        match &self.sources[entry.source] {
            Source::Dir(dir) => std::fs::read(dir.join(&entry.name)).ok(),
            Source::Zip(zip) => {
                let mut zip = zip.lock().unwrap();
                let mut file = zip.by_name(&entry.name).ok()?;
                let mut out = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut out).ok()?;
                Some(out)
            }
        }
    }

    pub fn json(&self, path: &str) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.read(path)?).ok()
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            let name = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push(name);
        }
    }
}

/// The files holding assets in a game folder.
pub fn stack(game_dir: &Path, version: &str) -> Stack {
    let mut mods: Vec<PathBuf> = std::fs::read_dir(game_dir.join("mods"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")))
                .collect()
        })
        .unwrap_or_default();
    mods.sort();
    let enabled = enabled_packs(game_dir);
    let packs = enabled
        .iter()
        .filter_map(|p| p.strip_prefix("file/"))
        .map(|name| game_dir.join("resourcepacks").join(name))
        .filter(|p| p.exists())
        .collect();
    Stack {
        minecraft_jar: minecraft_jar(game_dir, version),
        mods,
        packs,
        enabled,
    }
}

/// The `resourcePacks` list of `options.txt`, lowest priority first.
fn enabled_packs(game_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(game_dir.join("options.txt")) else {
        return Vec::new();
    };
    text.lines()
        .find_map(|l| l.strip_prefix("resourcePacks:"))
        .and_then(|list| serde_json::from_str::<Vec<String>>(list.trim()).ok())
        .unwrap_or_default()
}

/// The Minecraft client jar, wherever the launcher keeps it: near the game folder for
/// CurseForge, Prism, MultiMC and the Modrinth App, or in the vanilla launcher's folder.
pub fn minecraft_jar(game_dir: &Path, version: &str) -> Option<PathBuf> {
    let file = format!("{version}.jar");
    let client = format!("minecraft-{version}-client.jar");
    let tails = [
        vec!["versions", version, &file],
        vec!["Install", "versions", version, &file],
        vec!["install", "versions", version, &file],
        vec!["meta", "versions", version, &file],
        vec!["minecraft", "versions", version, &file],
        vec!["libraries", "com", "mojang", "minecraft", version, &client],
    ];
    let mut bases: Vec<PathBuf> = game_dir
        .ancestors()
        .take(6)
        .map(Path::to_path_buf)
        .collect();
    for var in ["APPDATA", "HOME"] {
        if let Some(home) = std::env::var_os(var) {
            bases.push(PathBuf::from(home).join(".minecraft"));
        }
    }
    bases.iter().find_map(|base| {
        tails.iter().find_map(|tail| {
            let path = tail.iter().fold(base.clone(), |p, part| p.join(part));
            path.is_file().then_some(path)
        })
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a zip with the given files.
    pub fn zip_file(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vistaforge-assets-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stacks_like_the_game() {
        let base = temp_dir("stack");
        // A CurseForge layout: the Minecraft jar two folders up from the instance.
        let jar_dir = base.join("minecraft/Install/versions/1.20.1");
        std::fs::create_dir_all(&jar_dir).unwrap();
        zip_file(
            &jar_dir.join("1.20.1.jar"),
            &[
                ("assets/minecraft/textures/block/stone.png", b"vanilla"),
                ("assets/minecraft/models/block/cube.json", b"{}"),
                ("net/minecraft/Foo.class", b""),
            ],
        );
        let game = base.join("minecraft/Instances/CR Pack");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::create_dir_all(
            game.join("resourcepacks/Folder Pack/assets/minecraft/textures/block"),
        )
        .unwrap();
        std::fs::write(
            game.join("resourcepacks/Folder Pack/assets/minecraft/textures/block/dirt.png"),
            b"pack",
        )
        .unwrap();
        zip_file(
            &game.join("mods/conquest.jar"),
            &[
                ("assets/minecraft/textures/block/stone.png", b"conquest"),
                ("assets/minecraft/textures/block/dirt.png", b"conquest"),
                (
                    "resourcepacks/extra/assets/minecraft/textures/block/sand.png",
                    b"built-in",
                ),
                (
                    "resourcepacks/off/assets/minecraft/textures/block/gravel.png",
                    b"off",
                ),
            ],
        );
        std::fs::write(game.join("mods/old.jar.disabled"), b"").unwrap();
        std::fs::write(
            game.join("options.txt"),
            "lang:en_us\nresourcePacks:[\"vanilla\",\"fabric\",\"conquest:extra\",\"file/Folder Pack\"]\n",
        )
        .unwrap();

        let stack = stack(&game, "1.20.1");
        assert_eq!(stack.minecraft_jar, Some(jar_dir.join("1.20.1.jar")));
        assert_eq!(stack.mods, vec![game.join("mods/conquest.jar")]);
        assert_eq!(stack.packs, vec![game.join("resourcepacks/Folder Pack")]);

        let assets = Assets::open(&game, "1.20.1").unwrap();
        let read = |p: &str| assets.read(p).map(|b| String::from_utf8(b).unwrap());
        assert_eq!(
            read("assets/minecraft/textures/block/stone.png").unwrap(),
            "conquest"
        );
        assert_eq!(
            read("assets/minecraft/textures/block/dirt.png").unwrap(),
            "pack"
        );
        assert_eq!(
            read("assets/minecraft/textures/block/sand.png").unwrap(),
            "built-in"
        );
        assert!(read("assets/minecraft/textures/block/gravel.png").is_none());
        assert!(assets.has("assets/minecraft/models/block/cube.json"));
        assert!(!assets.has("net/minecraft/Foo.class"));
        assert_eq!(assets.found.mods, 1);
        assert_eq!(assets.found.packs, 1);
        std::fs::remove_dir_all(base).ok();
    }
}
