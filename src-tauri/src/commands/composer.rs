//! Thin glue between the Switch page and `services::composer` /
//! `services::composer_catalog`.

use tauri::AppHandle;

use crate::services::composer::{self, ComposerVersionStatus};
use crate::services::composer_catalog::{self, ComposerRelease};
use crate::services::php_path;
use crate::services::scaffold;
use crate::utils::error::AppError;

/// Whether at least one Composer version is installed — the label a project
/// template's requirements check and the Switch page both read.
#[tauri::command]
pub fn composer_installed() -> bool {
    scaffold::composer_installed()
}

/// Downloads the newest catalog version if nothing is installed yet.
/// Pre-existing entry point (predates per-version installs); scaffolding a
/// Laravel project does the same thing lazily on its own, so this exists
/// for a user who wants Composer ready ahead of time.
#[tauri::command]
pub async fn install_composer() -> Result<(), AppError> {
    scaffold::install_composer().await
}

/// The Composer versions currently on disk, and which one is active.
#[tauri::command]
pub fn list_composer_versions() -> Vec<ComposerVersionStatus> {
    composer::list()
}

/// Also re-points the global `composer` link when it's on, the same way a
/// PHP switch re-points `php`'s.
#[tauri::command]
pub async fn set_active_composer_version(
    id: String,
) -> Result<Vec<ComposerVersionStatus>, AppError> {
    let versions = composer::set_active(&id)?;
    // Reads the registry and may create a junction, both through PowerShell.
    let _ = tokio::task::spawn_blocking(php_path::refresh_composer_link).await;
    Ok(versions)
}

/// The versions Composer currently publishes as stable. Hits the network on
/// first use, then serves a cached copy until `refresh` is set.
#[tauri::command]
pub async fn list_composer_catalog(refresh: bool) -> Result<Vec<ComposerRelease>, AppError> {
    composer_catalog::list(refresh).await
}

#[tauri::command]
pub async fn install_composer_version(
    app: AppHandle,
    version: String,
) -> Result<Vec<ComposerVersionStatus>, AppError> {
    let versions = composer::install(&app, &version).await?;
    // The first install is what gives an already-enabled global link
    // something to point at.
    let _ = tokio::task::spawn_blocking(php_path::refresh_composer_link).await;
    Ok(versions)
}
