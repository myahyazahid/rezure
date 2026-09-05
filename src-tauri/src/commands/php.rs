//! Thin glue between the Switch page and `services::php` /
//! `services::php_catalog`.

use std::path::PathBuf;

use tauri::AppHandle;

use serde::Serialize;
use tauri::State;

use crate::config::settings::{self, SettingsState};
use crate::services::binaries;
use crate::services::doctor::{self, ProjectDiagnosis};
use crate::services::php::{self, PhpVersionStatus};
use crate::services::php_catalog::{self, PhpRelease};
use crate::services::php_ext::{self, ExtensionStatus};
use crate::services::php_ini;
use crate::services::php_path::{self, PhpPathStatus};
use crate::services::{ServiceManager, ServiceStatus};
use crate::utils::error::AppError;

/// The PHP service's id in [`ServiceManager`] — see `ProcessService::php`.
const PHP_SERVICE: &str = "php";

fn joined(e: tokio::task::JoinError) -> AppError {
    AppError::Io(format!("background task panicked: {e}"))
}

/// What a version switch did, beyond changing the active version.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpSwitchResult {
    pub versions: Vec<PhpVersionStatus>,
    /// True when the running PHP service was restarted onto the new version.
    /// False when it wasn't running — there was nothing to reload.
    pub restarted: bool,
    /// Set when the restart itself failed. Reported *beside* the result
    /// rather than as a failed command: the switch already happened, so
    /// returning an error would leave the UI showing the old version as
    /// active while the backend had already moved on. The user needs to see
    /// both — the version changed, and PHP is now down.
    pub restart_error: Option<String>,
}

/// The PHP versions on disk — downloaded by Rezure or dropped into the
/// user's own `bin` folder, which are equally installed as far as this is
/// concerned.
#[tauri::command]
pub fn list_php_versions() -> Vec<PhpVersionStatus> {
    php::list()
}

/// Switches the active PHP version and reloads the service so the change
/// takes effect immediately.
///
/// `services::process` resolves the PHP binary at spawn time, so a running
/// `php-cgi` keeps serving the old version until it restarts. Doing that
/// here means the Switch page's promise ("pick the version each new vhost
/// should use") is true the moment it's clicked, instead of quietly
/// requiring a manual restart nothing in the UI asks for.
///
/// Only PHP is restarted: nginx reaches it over `127.0.0.1:9000` per
/// request and reconnects on its own once the new process has rebound the
/// port, so bouncing nginx too would drop live requests for nothing.
#[tauri::command]
pub async fn set_active_php_version(
    id: String,
    manager: State<'_, ServiceManager>,
    settings: State<'_, SettingsState>,
) -> Result<PhpSwitchResult, AppError> {
    let versions = php::set_active(&id)?;

    // Persisted so the choice survives a restart — best-effort: the switch
    // itself already succeeded, so a settings-write failure shouldn't turn
    // into a failed command, just an unpersisted choice for this session.
    {
        let mut current = settings.0.lock().unwrap();
        current.active_php_version = Some(id.clone());
        if let Err(err) = settings::save(&current) {
            log::warn!("failed to persist the active PHP version: {err}");
        }
    }

    // When the global PATH link is on, it has to follow the switch — that's
    // the whole reason it exists. Best-effort: a link problem must not make
    // the switch itself look like it failed, and `status()` reports the
    // link as out of sync if this didn't take.
    if php_path::status().map(|s| s.on_path).unwrap_or(false) {
        if let Err(err) = php_path::sync() {
            log::warn!("failed to re-point the PHP PATH link: {err}");
        }
    }

    let service = manager.find(PHP_SERVICE)?;
    if service.info().status != ServiceStatus::Running {
        return Ok(PhpSwitchResult {
            versions,
            restarted: false,
            restart_error: None,
        });
    }

    let restart = tokio::task::spawn_blocking(move || service.restart())
        .await
        .map_err(joined)?;

    Ok(PhpSwitchResult {
        versions,
        restarted: restart.is_ok(),
        restart_error: restart.err().map(|e| e.to_string()),
    })
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

/// Which PECL extensions Rezure can add to this PHP version, and which are
/// already there.
#[tauri::command]
pub async fn php_extensions(php_version: String) -> Result<Vec<ExtensionStatus>, AppError> {
    tokio::task::spawn_blocking(move || php_ext::status_for(&php_version))
        .await
        .map_err(joined)?
}

/// Downloads a PECL extension, verifies it against its pinned checksum, and
/// installs it into that PHP version.
///
/// Returns the refreshed list rather than a bare `Ok`, so the UI can't end up
/// showing a stale "not installed" next to an extension that now is.
#[tauri::command]
pub async fn install_php_extension(
    app: AppHandle,
    id: String,
    php_version: String,
) -> Result<Vec<ExtensionStatus>, AppError> {
    php_ext::install(&app, &id, &php_version).await?;
    php_ext::status_for(&php_version)
}

/// Reads a project's `ext-*` requirements back against the active PHP.
///
/// Lives with the PHP commands rather than the project ones because that is
/// what it answers a question about: the project side is just the
/// `composer.json` it reads. Spawns `php -m`, so it goes through
/// `spawn_blocking` like every other command here that shells out.
#[tauri::command]
pub async fn diagnose_project(id: String) -> Result<ProjectDiagnosis, AppError> {
    tokio::task::spawn_blocking(move || doctor::diagnose_project(&id))
        .await
        .map_err(joined)?
}

/// The folder a user's own PHP settings go in, shown on the Switch page.
///
/// Read without creating anything: this is a label on a page that loads on
/// its own, and the folder is created by the first PHP process anyway.
#[tauri::command]
pub fn php_config_dir() -> Result<String, AppError> {
    Ok(php_ini::conf_d()?.display().to_string())
}

/// Creates the settings folder if needed and opens it in Explorer.
#[tauri::command]
pub fn open_php_config_dir() -> Result<(), AppError> {
    let dir = php_ini::ensure_conf_d()?;
    tauri_plugin_opener::open_path(dir.display().to_string(), None::<&str>).map_err(|e| {
        AppError::OpenFailed {
            target: "the PHP settings folder".to_string(),
            reason: e.to_string(),
        }
    })
}

/// Whether Rezure's PHP is on the user's PATH, where the link points, and
/// which other PHP installs enabling it would override.
#[tauri::command]
pub async fn php_path_status() -> Result<PhpPathStatus, AppError> {
    tokio::task::spawn_blocking(php_path::status)
        .await
        .map_err(joined)?
}

/// Puts Rezure's PHP first on the user's PATH.
///
/// The one action in Rezure that changes something outside the app, so it
/// only ever runs from an explicit toggle — never as a side effect.
#[tauri::command]
pub async fn enable_php_path() -> Result<PhpPathStatus, AppError> {
    tokio::task::spawn_blocking(php_path::enable)
        .await
        .map_err(joined)?
}

#[tauri::command]
pub async fn disable_php_path() -> Result<PhpPathStatus, AppError> {
    tokio::task::spawn_blocking(php_path::disable)
        .await
        .map_err(joined)?
}
