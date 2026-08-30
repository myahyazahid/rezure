use crate::db::projects::ProjectInfo;
use crate::services::scaffold::ProjectTemplate;
use crate::services::{hosts, launcher, projects, scaffold, vhosts};
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

#[tauri::command]
pub fn list_project_templates() -> Vec<ProjectTemplate> {
    scaffold::TEMPLATES.to_vec()
}

/// The folder new projects are created under — shown in the "New project"
/// dialog so the path preview matches reality.
#[tauri::command]
pub fn www_root() -> Result<String, AppError> {
    projects::www_root().map(|p| p.display().to_string())
}

/// Creates a new project under `www_root()` from a template. Can take a
/// while (Laravel resolves and downloads its Composer dependencies over
/// the network) — the frontend shows a pending state for the duration.
#[tauri::command]
pub async fn create_project(name: String, template: String) -> Result<(), AppError> {
    scaffold::create_project(&name, &template).await
}

#[tauri::command]
pub fn composer_installed() -> bool {
    scaffold::composer_installed()
}

#[tauri::command]
pub async fn install_composer() -> Result<(), AppError> {
    scaffold::install_composer().await
}

/// Opens the project's site in the default browser. Takes the project's
/// id, not a URL — the domain is re-resolved from the scan on this side so
/// the frontend can't ask the OS to open something Rezure didn't detect.
#[tauri::command]
pub fn open_project_site(id: String) -> Result<(), AppError> {
    launcher::open_site(&id)
}

/// Opens the project folder in Explorer.
#[tauri::command]
pub fn open_project_folder(id: String) -> Result<(), AppError> {
    launcher::open_folder(&id)
}

/// Opens a terminal in the project folder.
#[tauri::command]
pub fn open_project_terminal(id: String) -> Result<(), AppError> {
    launcher::open_terminal(&id)
}
