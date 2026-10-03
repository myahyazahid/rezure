//! Thin glue between the Switch page and `services::node`/`services::node_catalog`.

use tauri::AppHandle;
use tauri::State;

use crate::config::settings::{self, SettingsState};
use crate::services::node::{self, NodeVersionStatus};
use crate::services::node_catalog::{self, NodeRelease};
use crate::utils::error::AppError;

/// The Node.js versions currently on disk, and which one is active.
#[tauri::command]
pub fn list_node_versions() -> Vec<NodeVersionStatus> {
    node::list()
}

/// Switches the global active Node.js version.
///
/// Unlike `commands::php::set_active_php_version`, there's no running
/// service to restart and (for now) no system-wide PATH link to re-point —
/// see `services::node`'s module doc for why. The switch only changes what
/// `services::launcher::open_terminal` resolves `node`/`npm`/`npx` as the
/// next time a terminal is opened for a project that hasn't pinned its own
/// version.
#[tauri::command]
pub async fn set_active_node_version(
    id: String,
    settings: State<'_, SettingsState>,
) -> Result<Vec<NodeVersionStatus>, AppError> {
    let versions = node::set_active(&id)?;

    // Best-effort, same reasoning as the PHP equivalent: the switch itself
    // already succeeded, so a settings-write failure shouldn't fail the
    // command, just leave the choice unpersisted for next launch.
    let mut current = settings.0.lock().unwrap();
    current.active_node_version = Some(id);
    if let Err(err) = settings::save(&current) {
        log::warn!("failed to persist the active Node.js version: {err}");
    }

    Ok(versions)
}

/// The versions nodejs.org currently publishes (newest patch per LTS line,
/// plus the newest Current release). Hits the network on first use, then
/// serves a cached copy until `refresh` is set.
#[tauri::command]
pub async fn list_node_catalog(refresh: bool) -> Result<Vec<NodeRelease>, AppError> {
    node_catalog::list(refresh).await
}

#[tauri::command]
pub async fn install_node_version(app: AppHandle, version: String) -> Result<(), AppError> {
    node_catalog::install(&app, &version).await
}
