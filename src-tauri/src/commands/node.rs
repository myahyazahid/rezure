//! Thin glue between the Switch page and `services::node`/`services::node_catalog`.

use tauri::AppHandle;
use tauri::State;

use crate::config::settings::{self, SettingsState};
use crate::services::node::{self, NodeVersionStatus};
use crate::services::node_catalog::{self, NodeRelease};
use crate::services::php_path::{self, Tool};
use crate::utils::error::AppError;

/// The Node.js versions currently on disk, and which one is active.
#[tauri::command]
pub fn list_node_versions() -> Vec<NodeVersionStatus> {
    node::list()
}

/// Switches the global active Node.js version.
///
/// Unlike `commands::php::set_active_php_version`, there's no running
/// service to restart. What changes is what a project terminal that hasn't
/// pinned its own version resolves `node`/`npm`/`npx` as, and, when "Use
/// Rezure's PHP everywhere" is on, the `current\node` link every other
/// terminal goes through (`services::php_path::Tool::Node`).
#[tauri::command]
pub async fn set_active_node_version(
    id: String,
    settings: State<'_, SettingsState>,
) -> Result<Vec<NodeVersionStatus>, AppError> {
    let versions = node::set_active(&id)?;

    // Best-effort, same reasoning as the PHP equivalent: the switch itself
    // already succeeded, so a settings-write failure shouldn't fail the
    // command, just leave the choice unpersisted for next launch. Scoped so
    // the lock is released before the await below.
    {
        let mut current = settings.0.lock().unwrap();
        current.active_node_version = Some(id);
        if let Err(err) = settings::save(&current) {
            log::warn!("failed to persist the active Node.js version: {err}");
        }
    }

    // Reads the registry and may create a junction, both through PowerShell.
    let _ = tokio::task::spawn_blocking(|| php_path::refresh_tool_link(Tool::Node)).await;

    Ok(versions)
}

/// The versions nodejs.org currently publishes (newest patch per LTS line,
/// plus the newest Current release). Hits the network on first use, then
/// serves a cached copy until `refresh` is set.
#[tauri::command]
pub async fn list_node_catalog(refresh: bool) -> Result<Vec<NodeRelease>, AppError> {
    node_catalog::list(refresh).await
}

/// Also points the global `node` link at the active version when it's on —
/// the first install is what gives it anything to point at.
#[tauri::command]
pub async fn install_node_version(app: AppHandle, version: String) -> Result<(), AppError> {
    node_catalog::install(&app, &version).await?;
    let _ = tokio::task::spawn_blocking(|| php_path::refresh_tool_link(Tool::Node)).await;
    Ok(())
}
