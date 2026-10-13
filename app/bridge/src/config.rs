//! What the app remembers between runs: the chosen folders and the last map settings.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use vistaforge_core::settings::WorldSettings;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub catalog: Option<PathBuf>,
    pub saves_dir: Option<PathBuf>,
    /// The map being edited when the app closed.
    pub settings: Option<WorldSettings>,
    /// View preferences, owned by the UI.
    pub ui: serde_json::Value,
}

/// The saved config, or the default when there is none or it can't be read.
pub fn load(path: &Path) -> AppConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, config: &AppConfig) -> Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(config)?)
}

/// Writes through a temporary file, so a crash never leaves half a file behind.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_files() {
        let dir = std::env::temp_dir().join(format!("vistaforge-config-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("nested/config.json");
        assert_eq!(load(&path), AppConfig::default());
        let config = AppConfig {
            catalog: Some("C:/game/vistaforge/catalog.json".into()),
            saves_dir: Some("C:/game/saves".into()),
            settings: Some(WorldSettings::default()),
            ui: serde_json::json!({"view": "split"}),
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path), config);
        std::fs::write(&path, "{broken").unwrap();
        assert_eq!(load(&path), AppConfig::default());
        // Unknown and missing fields are fine.
        std::fs::write(&path, r#"{"saves_dir": "/x", "later": 1}"#).unwrap();
        assert_eq!(load(&path).saves_dir, Some("/x".into()));
        std::fs::remove_dir_all(dir).ok();
    }
}
