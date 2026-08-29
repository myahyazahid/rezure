use crate::db::projects::ProjectInfo;
use crate::services::{hosts, projects, vhosts};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_projects() -> Result<Vec<ProjectInfo>, AppError> {
    let detected = projects::scan_projects()?;

    // Best-effort — a vhost-sync hiccup shouldn't stop the project list
    // itself from loading.
    if let Err(err) = vhosts::sync_vhosts() {
        log::warn!("failed to sync nginx vhosts: {err}");
    }

    Ok(detected)
}

/// Writes every detected project's domain into the OS hosts file, prompting
/// the user for admin rights via a real Windows UAC dialog. Never called
/// automatically — only from an explicit action the user takes, since it's
/// a system file and the elevation prompt shouldn't show up as a surprise
/// side effect of just opening the Projects page.
///
/// Returns `true` if the hosts file changed, `false` if it was already up
/// to date (no elevation prompt is shown in that case at all).
#[tauri::command]
pub async fn sync_hosts() -> Result<bool, AppError> {
    tokio::task::spawn_blocking(hosts::sync_hosts_entries)
        .await
        .map_err(|e| AppError::HostsUpdateFailed(format!("background task panicked: {e}")))?
}
