//! Thin glue between the Settings page and `config::settings`.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::config::settings::{self, Settings, SettingsState};
use crate::services::{binaries, database, projects};
use crate::utils::error::AppError;

#[tauri::command]
pub fn get_settings(state: State<'_, SettingsState>) -> Settings {
    state.0.lock().unwrap().clone()
}

/// Only the fields present in `patch` are changed — everything else in the
/// current settings is left as-is.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub default_port: Option<u16>,
    pub share_usage_data: Option<bool>,
}

#[tauri::command]
pub fn update_settings(
    patch: SettingsPatch,
    state: State<'_, SettingsState>,
) -> Result<Settings, AppError> {
    let mut current = state.0.lock().unwrap();
    if let Some(port) = patch.default_port {
        current.default_port = port;
    }
    if let Some(share) = patch.share_usage_data {
        current.share_usage_data = share;
    }
    settings::save(&current)?;
    Ok(current.clone())
}

/// Read-only — where Rezure's own state lives on disk. Shown on the
/// Settings page so the user can find it without digging through docs;
/// there's nothing to edit here since the filesystem itself is the
/// registry for installed binaries (see `services::binaries`'s doc
/// comment) rather than a list of paths Rezure could point elsewhere.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePaths {
    pub www_root: String,
    pub binaries_dir: String,
    pub drop_in_dir: String,
    pub dumps_dir: String,
}

#[tauri::command]
pub fn storage_paths() -> Result<StoragePaths, AppError> {
    Ok(StoragePaths {
        www_root: projects::www_root()?.display().to_string(),
        binaries_dir: binaries::install_root()?.display().to_string(),
        drop_in_dir: binaries::user_bin_root()?.display().to_string(),
        dumps_dir: database::dumps_dir()?.display().to_string(),
    })
}
