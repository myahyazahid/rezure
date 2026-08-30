//! Thin glue between the Switch page and `services::php` /
//! `services::php_catalog`.

use std::path::PathBuf;

use tauri::AppHandle;

use crate::services::binaries;
use crate::services::php::{self, PhpVersionStatus};
use crate::services::php_catalog::{self, PhpRelease};
use crate::utils::error::AppError;

fn joined(e: tokio::task::JoinError) -> AppError {
    AppError::Io(format!("background task panicked: {e}"))
}

/// The PHP versions on disk — downloaded by Rezure or dropped into the
/// user's own `bin` folder, which are equally installed as far as this is
/// concerned.
#[tauri::command]
pub fn list_php_versions() -> Vec<PhpVersionStatus> {
    php::list()
}

#[tauri::command]
pub fn set_active_php_version(id: String) -> Result<Vec<PhpVersionStatus>, AppError> {
    php::set_active(&id)
}

/// The versions php.net currently publishes for Windows. Hits the network
/// on first use, then serves a cached copy until `refresh` is set.
#[tauri::command]
pub async fn list_php_catalog(refresh: bool) -> Result<Vec<PhpRelease>, AppError> {
    php_catalog::list(refresh).await
}

#[tauri::command]
pub async fn install_php_version(
    app: AppHandle,
    version: String,
) -> Result<Vec<PhpVersionStatus>, AppError> {
    php::install(&app, &version).await
}

/// Copies a PHP build the user downloaded themselves into the drop-in
/// folder, reading its real version out of the binary.
#[tauri::command]
pub async fn add_php_from_folder(path: String) -> Result<Vec<PhpVersionStatus>, AppError> {
    php::add_from_folder(PathBuf::from(path)).await
}

#[tauri::command]
pub async fn remove_php_version(version: String) -> Result<Vec<PhpVersionStatus>, AppError> {
    tokio::task::spawn_blocking(move || php::remove(&version))
        .await
        .map_err(joined)?
}

/// The drop-in folder's path, shown on the Switch page so it can be found
/// without digging through docs.
#[tauri::command]
pub fn php_drop_in_dir() -> Result<String, AppError> {
    Ok(binaries::user_bin_root()?.join("php").display().to_string())
}

/// Creates the drop-in folder if needed and opens it in Explorer.
#[tauri::command]
pub fn open_php_drop_in_dir() -> Result<(), AppError> {
    let dir = binaries::user_bin_root()?.join("php");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    tauri_plugin_opener::open_path(dir.display().to_string(), None::<&str>).map_err(|e| {
        AppError::OpenFailed {
            target: "the PHP folder".to_string(),
            reason: e.to_string(),
        }
    })
}
