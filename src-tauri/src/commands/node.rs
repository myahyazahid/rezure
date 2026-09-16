//! Thin glue between the Switch page and `services::node_catalog`.
//!
//! Catalog and install only — see the module doc on `services::node_catalog`
//! for why there's no "active version"/switch command here yet.

use tauri::AppHandle;

use crate::services::binaries::{self, InstalledVersionStatus};
use crate::services::node_catalog::{self, NodeRelease};
use crate::utils::error::AppError;

/// The Node.js versions currently on disk.
#[tauri::command]
pub fn list_node_versions() -> Vec<InstalledVersionStatus> {
    binaries::discover_status("node", "node.exe")
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
