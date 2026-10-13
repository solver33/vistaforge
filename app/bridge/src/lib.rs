//! Everything the desktop app asks of the core, as plain functions.
//!
//! The Tauri shell (`app/src-tauri`) and the development server (`vistaforge-devserver`) both
//! wrap [`Bridge`], so this logic builds and is tested without a webview. Names in the JSON that
//! goes to the UI are snake_case, like the settings file.

pub mod config;
pub mod encode;
pub mod paths;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use vistaforge_core::biome::Biome;
use vistaforge_core::catalog::Catalog;
use vistaforge_core::engine::{ExportReport, Generator, Progress};
use vistaforge_core::settings::{Preset, WorldSettings, MAX_SIZE, MIN_SIZE};
use vistaforge_core::terrain::{presets, Quality};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the UI needs to draw its controls; sent once at start-up.
#[derive(Clone, Debug, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub presets: Vec<Preset>,
    pub biomes: Vec<BiomeInfo>,
    pub min_size: u32,
    pub max_size: u32,
    pub defaults: WorldSettings,
}

#[derive(Clone, Debug, Serialize)]
pub struct BiomeInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// Made by the terrain (beaches, rivers, lakes) rather than chosen in the biome mix.
    pub feature: bool,
}

pub fn app_info() -> AppInfo {
    AppInfo {
        version: VERSION,
        presets: presets::all(),
        biomes: Biome::ALL
            .iter()
            .map(|&b| BiomeInfo {
                id: b.id(),
                name: b.name(),
                feature: b.is_feature(),
            })
            .collect(),
        min_size: MIN_SIZE,
        max_size: MAX_SIZE,
        defaults: WorldSettings::default(),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogInfo {
    pub path: PathBuf,
    pub blocks: usize,
    /// Mod versions recorded by the exporter, e.g. `conquest` → `1.7.0`.
    pub mods: BTreeMap<String, Option<String>>,
    pub seconds: f64,
}

/// How an export ended when it did not fail.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ExportOutcome {
    Done { report: ExportReport },
    Cancelled,
}

pub struct Bridge {
    catalog: Mutex<Option<Arc<Catalog>>>,
    cancel: AtomicBool,
    exporting: AtomicBool,
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Clears a flag when dropped, so an early return or a panic can't leave it set.
struct Lowered<'a>(&'a AtomicBool);

impl Drop for Lowered<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Bridge {
    pub fn new() -> Self {
        Self {
            catalog: Mutex::new(None),
            cancel: AtomicBool::new(false),
            exporting: AtomicBool::new(false),
        }
    }

    /// Loads the catalog the exporter mod wrote and checks that it has every block the
    /// generator uses, so a wrong file is reported now rather than at export.
    pub fn load_catalog(&self, path: &Path) -> Result<CatalogInfo> {
        let started = Instant::now();
        let catalog = Catalog::load(path)?;
        let probe = WorldSettings {
            size: MIN_SIZE,
            ..WorldSettings::default()
        };
        Generator::new(&probe, &catalog, Quality::Preview).context(
            "this catalog lacks blocks Vistaforge needs; export it from a Conquest Reforged \
             1.20.1 game with the Vistaforge exporter mod",
        )?;
        let info = CatalogInfo {
            path: path.to_path_buf(),
            blocks: catalog.len(),
            mods: catalog.mods.clone(),
            seconds: started.elapsed().as_secs_f64(),
        };
        *self.catalog.lock().unwrap() = Some(Arc::new(catalog));
        Ok(info)
    }

    fn catalog(&self) -> Result<Arc<Catalog>> {
        self.catalog
            .lock()
            .unwrap()
            .clone()
            .context("choose the block catalog first")
    }

    /// A low-resolution picture of the map, packed by [`encode::preview`].
    pub fn preview(&self, settings: &WorldSettings, res: u32) -> Result<Vec<u8>> {
        let catalog = self.catalog()?;
        let generator = Generator::new(settings, &catalog, Quality::Preview)?;
        Ok(encode::preview(&generator.preview(res)))
    }

    /// Writes the world to `<saves_dir>/<settings.name>`. `progress` is called at most about
    /// twenty times a second, and always for the last chunk. A cancelled or failed export
    /// leaves no half-written world behind.
    pub fn export(
        &self,
        settings: &WorldSettings,
        saves_dir: &Path,
        overwrite: bool,
        progress: &(dyn Fn(Progress) + Sync),
    ) -> Result<ExportOutcome> {
        paths::check_world_name(&settings.name).map_err(anyhow::Error::msg)?;
        if !saves_dir.is_dir() {
            bail!("the saves folder {} does not exist", saves_dir.display());
        }
        let world = saves_dir.join(&settings.name);
        if world.exists() && !overwrite {
            bail!(
                "a world named \"{}\" already exists in {}",
                settings.name,
                saves_dir.display()
            );
        }
        if self.exporting.swap(true, Ordering::SeqCst) {
            bail!("an export is already running");
        }
        let _running = Lowered(&self.exporting);
        self.cancel.store(false, Ordering::SeqCst);

        let catalog = self.catalog()?;
        let generator = Generator::new(settings, &catalog, Quality::Full)?;
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let throttled = |p: Progress| {
            let mut last = last.lock().unwrap();
            if p.chunks_done >= p.chunks_total || last.elapsed() >= Duration::from_millis(50) {
                *last = Instant::now();
                progress(p);
            }
        };
        match generator.export(saves_dir, overwrite, &throttled, &self.cancel) {
            Ok(report) => Ok(ExportOutcome::Done { report }),
            Err(e) => {
                // From here on the folder is ours: it was missing, or the engine removed the
                // old world because the user chose to replace it.
                if world.exists() {
                    std::fs::remove_dir_all(&world).ok();
                }
                if self.cancel.load(Ordering::SeqCst) {
                    Ok(ExportOutcome::Cancelled)
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn cancel_export(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_exporting(&self) -> bool {
        self.exporting.load(Ordering::SeqCst)
    }
}

pub fn read_settings(path: &Path) -> Result<WorldSettings> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a settings file", path.display()))
}

pub fn write_settings(path: &Path, settings: &WorldSettings) -> Result<()> {
    config::write_atomic(path, &serde_json::to_vec_pretty(settings)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn catalog_path() -> Option<PathBuf> {
        let path = std::env::var("VISTAFORGE_CATALOG")
            .unwrap_or_else(|_| "/mnt/project-files/vistaforge/catalog-cr-1.7.0.json".into());
        Path::new(&path).is_file().then(|| path.into())
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vistaforge-bridge-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn app_info_lists_presets_and_biomes() {
        let info = app_info();
        assert!(!info.presets.is_empty());
        assert_eq!(info.biomes.len(), Biome::ALL.len());
        assert!(info.biomes.iter().any(|b| b.id == "river" && b.feature));
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["defaults"]["sea_level"], 63);
        assert_eq!(json["min_size"], MIN_SIZE);
    }

    #[test]
    fn preview_needs_a_catalog() {
        let err = Bridge::new()
            .preview(&WorldSettings::default(), 64)
            .unwrap_err();
        assert!(err.to_string().contains("catalog"));
    }

    #[test]
    fn settings_files_round_trip() {
        let dir = temp_dir("settings");
        let path = dir.join("map.json");
        let settings = WorldSettings {
            name: "Round trip".into(),
            seed: 42,
            ..WorldSettings::default()
        };
        write_settings(&path, &settings).unwrap();
        assert_eq!(read_settings(&path).unwrap(), settings);
        std::fs::write(&path, "not json").unwrap();
        assert!(read_settings(&path).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn export_refuses_bad_names_and_existing_worlds() {
        let dir = temp_dir("refuse");
        std::fs::create_dir_all(dir.join("Taken")).unwrap();
        let bridge = Bridge::new();
        let bad = WorldSettings {
            name: "a/b".into(),
            ..WorldSettings::default()
        };
        assert!(bridge.export(&bad, &dir, false, &|_| {}).is_err());
        let taken = WorldSettings {
            name: "Taken".into(),
            ..WorldSettings::default()
        };
        let err = bridge.export(&taken, &dir, false, &|_| {}).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        // The existing world is untouched.
        assert!(dir.join("Taken").is_dir());
        assert!(!bridge.is_exporting());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn preview_and_export_with_the_real_catalog() {
        let Some(path) = catalog_path() else {
            eprintln!("skipped: no catalog (set VISTAFORGE_CATALOG)");
            return;
        };
        let bridge = Bridge::new();
        let info = bridge.load_catalog(&path).unwrap();
        assert!(info.blocks > 1000);

        let settings = WorldSettings {
            name: "Bridge Test".into(),
            size: 256,
            ..WorldSettings::default()
        };
        let bytes = bridge.preview(&settings, 64).unwrap();
        let decoded = encode::tests::decode(&bytes);
        assert_eq!(decoded.res, 64);
        assert_eq!(decoded.heights.len(), 64 * 64);

        let dir = temp_dir("export");
        let calls = Mutex::new(Vec::new());
        let outcome = bridge
            .export(&settings, &dir, false, &|p| calls.lock().unwrap().push(p))
            .unwrap();
        let ExportOutcome::Done { report } = outcome else {
            panic!("export did not finish");
        };
        assert_eq!(report.chunks, 256);
        assert!(dir.join("Bridge Test/level.dat").is_file());
        let calls = calls.into_inner().unwrap();
        assert_eq!(calls.last().unwrap().chunks_done, 256);

        // Cancelling while replacing a world leaves nothing half-written behind.
        let cancel_early = |_: Progress| bridge.cancel_export();
        let outcome = bridge.export(&settings, &dir, true, &cancel_early).unwrap();
        assert!(matches!(outcome, ExportOutcome::Cancelled));
        assert!(!dir.join("Bridge Test").exists());
        assert!(!bridge.is_exporting());
        std::fs::remove_dir_all(dir).ok();
    }
}
