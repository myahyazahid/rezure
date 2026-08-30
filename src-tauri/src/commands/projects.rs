use tauri::State;

use crate::db::projects::ProjectInfo;
use crate::db::{self, DbState};
use crate::services::scaffold::ProjectTemplate;
use crate::services::{hosts, launcher, projects, scaffold, vhosts};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_projects(db_state: State<'_, DbState>) -> Result<Vec<ProjectInfo>, AppError> {
    let mut detected = projects::scan_projects()?;

    // Best-effort — a vhost-sync hiccup shouldn't stop the project list
    // itself from loading.
    if let Err(err) = vhosts::sync_vhosts() {
        log::warn!("failed to sync nginx vhosts: {err}");
    }

    // Best-effort, same reasoning: a SQLite hiccup shouldn't stop the list
    // from loading, just leave it without history for this call.
    let conn = db_state.0.lock().unwrap();
    for project in &detected {
        if let Err(err) = db::projects::upsert_seen(&conn, project) {
            log::warn!(
                "failed to record {} in the project database: {err}",
                project.id
            );
        }
    }
    match db::projects::fetch_history(&conn) {
        Ok(history) => {
            for project in &mut detected {
                if let Some((last_opened_at, open_count)) = history.get(&project.id) {
                    project.last_opened_at = *last_opened_at;
                    project.open_count = *open_count;
                }
            }
        }
        Err(err) => log::warn!("failed to load project history: {err}"),
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

/// Best-effort history write — the open itself already succeeded by the
/// time this runs, so a database hiccup here must not turn into a failed
/// command.
fn mark_opened(db_state: &State<'_, DbState>, id: &str) {
    let conn = db_state.0.lock().unwrap();
    if let Err(err) = db::projects::record_opened(&conn, id) {
        log::warn!("failed to record {id} as opened: {err}");
    }
}

/// Opens the project's site in the default browser. Takes the project's
/// id, not a URL — the domain is re-resolved from the scan on this side so
/// the frontend can't ask the OS to open something Rezure didn't detect.
#[tauri::command]
pub fn open_project_site(id: String, db_state: State<'_, DbState>) -> Result<(), AppError> {
    launcher::open_site(&id)?;
    mark_opened(&db_state, &id);
    Ok(())
}

/// Opens the project folder in Explorer.
#[tauri::command]
pub fn open_project_folder(id: String, db_state: State<'_, DbState>) -> Result<(), AppError> {
    launcher::open_folder(&id)?;
    mark_opened(&db_state, &id);
    Ok(())
}

/// Opens a terminal in the project folder.
#[tauri::command]
pub fn open_project_terminal(id: String, db_state: State<'_, DbState>) -> Result<(), AppError> {
    launcher::open_terminal(&id)?;
    mark_opened(&db_state, &id);
    Ok(())
}

/// What linking a folder would produce — name, stack, docroot and the
/// domain it would get — without registering anything. Also where a bad
/// path is refused, so the dialog can say why before the user commits.
#[tauri::command]
pub fn preview_project_link(path: String) -> Result<projects::LinkPreview, AppError> {
    projects::prepare_link(&path)
}

/// Registers a folder outside `www` as a project. Records the path only —
/// nothing inside the folder is read, written or moved.
#[tauri::command]
pub fn link_project(
    path: String,
    name: Option<String>,
    domain: Option<String>,
) -> Result<(), AppError> {
    projects::link(&path, name, domain)?;
    // The new project needs a vhost before it can serve; best-effort, since
    // a sync problem shouldn't undo a link the user just made.
    if let Err(err) = vhosts::sync_vhosts() {
        log::warn!("failed to sync nginx vhosts after linking: {err}");
    }
    Ok(())
}

/// Forgets a linked project. The folder and everything in it is left
/// exactly as it was — this only removes Rezure's pointer to it.
#[tauri::command]
pub fn unlink_project(id: String) -> Result<(), AppError> {
    projects::unlink(&id)?;
    if let Err(err) = vhosts::sync_vhosts() {
        log::warn!("failed to sync nginx vhosts after unlinking: {err}");
    }
    Ok(())
}
