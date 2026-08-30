//! User-facing settings, persisted as JSON — the single source of truth for
//! everything on the Settings page, plus the active PHP version (`services::php`
//! previously kept that in process-only memory).
//!
//! Loaded once at startup into [`SettingsState`] (managed Tauri state) per
//! `CLAUDE.md`'s "config is a single source of truth" rule — commands read
//! and write the in-memory copy, and every write is mirrored to disk.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::utils::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_port: u16,
    pub share_usage_data: bool,
    /// Mirrors `services::php`'s in-memory active version so it survives a
    /// restart. `None` until the user has switched at least once.
    pub active_php_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_port: 80,
            share_usage_data: false,
            active_php_version: None,
        }
    }
}

/// `%APPDATA%\Rezure\settings.json`.
pub fn path() -> Result<PathBuf, AppError> {
    let base = dirs::config_dir()
        .ok_or_else(|| AppError::Settings("could not resolve the config directory".to_string()))?;
    Ok(base.join("Rezure").join("settings.json"))
}

/// Reads settings from disk, falling back to defaults if the file is
/// missing or unreadable — corrupt settings must never block startup.
pub fn load() -> Settings {
    load_from(&match path() {
        Ok(path) => path,
        Err(err) => {
            log::warn!("could not resolve settings path, using defaults: {err}");
            return Settings::default();
        }
    })
}

fn load_from(path: &Path) -> Settings {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return Settings::default(),
    };
    match serde_json::from_str(&content) {
        Ok(settings) => settings,
        Err(err) => {
            log::warn!(
                "settings file at {} is unreadable, using defaults: {err}",
                path.display()
            );
            Settings::default()
        }
    }
}

pub fn save(settings: &Settings) -> Result<(), AppError> {
    save_to(&path()?, settings)
}

fn save_to(path: &Path, settings: &Settings) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Settings(format!("could not create {}: {e}", parent.display()))
        })?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| AppError::Settings(format!("could not serialize settings: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| AppError::Settings(format!("could not write {}: {e}", path.display())))
}

pub struct SettingsState(pub Mutex<Settings>);

impl SettingsState {
    pub fn new(settings: Settings) -> Self {
        Self(Mutex::new(settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rezure-test-settings-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        let settings = load_from(&path);
        assert_eq!(settings.default_port, 80);
        assert!(!settings.share_usage_data);
        assert_eq!(settings.active_php_version, None);
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults_instead_of_panicking() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        let settings = load_from(&path);
        assert_eq!(settings.default_port, 80);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = temp_path("roundtrip");
        let settings = Settings {
            default_port: 8080,
            share_usage_data: true,
            active_php_version: Some("8.3.33".to_string()),
        };
        save_to(&path, &settings).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.default_port, 8080);
        assert!(loaded.share_usage_data);
        assert_eq!(loaded.active_php_version.as_deref(), Some("8.3.33"));
        std::fs::remove_file(&path).unwrap();
    }
}
