//! Config load / template generation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::monitors::MonitorInfo;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    /// How long (ms) to hold the chord down. Larger values are more reliably
    /// caught by polling key-detectors. Missing = use the built-in default.
    #[serde(default)]
    pub hold_ms: Option<u64>,

    /// Maps a monitor device name (e.g. `\\.\DISPLAY1`) to a shortcut string
    /// such as `"ctrl+shift+home"`. An empty value means "do nothing".
    pub shortcuts: BTreeMap<String, String>,
}

impl Config {
    pub fn load(path: &Path) -> Option<Config> {
        let text = std::fs::read_to_string(path).ok()?;
        // Tolerate a UTF-8 BOM that some editors prepend.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        serde_json::from_str(text).ok()
    }

    /// Load config, or write a template pre-filled with detected monitors.
    pub fn load_or_template(path: &Path, monitors: &[MonitorInfo]) -> Config {
        if let Some(cfg) = Config::load(path) {
            return cfg;
        }

        let mut shortcuts = BTreeMap::new();
        for m in monitors {
            shortcuts.insert(m.device.clone(), String::new());
        }
        let cfg = Config {
            hold_ms: Some(crate::keys::DEFAULT_HOLD_MS),
            shortcuts,
        };
        let _ = cfg.save(path);
        cfg
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(path, text)
    }
}

/// Fixed config path (`%LOCALAPPDATA%\ScreenSwitchRs\config.json`) so every
/// launch — console, elevated, or the installed Scheduled Task — shares the
/// same settings, alongside `log.txt`.
pub fn default_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    Path::new(&base).join("ScreenSwitchRs").join("config.json")
}
