use tauri::State;

use crate::db::projects::ProjectInfo;
use crate::db::{self, DbState};
use crate::services::scaffold::ProjectTemplate;
use crate::services::{hosts, launcher, projects, scaffold, vhosts, ServiceManager, ServiceStatus};
use crate::utils::error::AppError;

/// Makes a project just created, linked or unlinked live immediately
/// instead of waiting for nginx's next full restart. Skipped silently when
/// nginx isn't running — there's nothing to reload, and the vhost files
/// on disk are already correct for whenever it does start.
fn reload_nginx_if_running(manager: &ServiceManager) {
    let Ok(nginx) = manager.find("nginx") else {
        return;
    };
    if nginx.info().status != ServiceStatus::Running {
        return;
    }
    if let Err(err) = vhosts::reload() {
        log::warn!("failed to reload nginx after a project change: {err}");
    }
}

/// Brings the vhost files up to date with what's on disk and, when that
/// actually changed something, makes a running nginx pick it up.
///
/// The `changed` check is what lets this be safe to call from
/// `list_projects`, which runs on every visit to the Projects page: without
/// it, simply opening that page would cycle nginx's workers.
///
/// Best-effort throughout — a vhost hiccup should leave a warning in the log,
/// not fail the command the user actually asked for.
fn sync_vhosts_and_reload(manager: &ServiceManager, context: &str) {
    match vhosts::sync_vhosts() {
        Ok(sync) => {
            if sync.changed {
                reload_nginx_if_running(manager);
            }
        }
        Err(err) => log::warn!("failed to sync nginx vhosts {context}: {err}"),
    }
}

#[tauri::command]
pub fn list_projects(
    db_state: State<'_, DbState>,
    manager: State<'_, ServiceManager>,
) -> Result<Vec<ProjectInfo>, AppError> {
    let mut detected = projects::scan_projects()?;

    // A folder dropped into `www` by hand shows up in this scan and nowhere
    // else, so this is the only place its vhost gets written — and without
    // the reload nginx would keep 404ing it until its next restart.
    sync_vhosts_and_reload(&manager, "while listing projects");

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
///
/// Syncs the vhosts too. A domain in the hosts file only resolves to
/// something once nginx has a matching `server` block, so doing one without
/// the other leaves the user on a connection-refused page having done
/// everything the UI asked of them.
#[tauri::command]
pub async fn sync_hosts(manager: State<'_, ServiceManager>) -> Result<bool, AppError> {
    sync_vhosts_and_reload(&manager, "while syncing hosts");

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
pub async fn create_project(
    name: String,
    template: String,
    manager: State<'_, ServiceManager>,
) -> Result<(), AppError> {
    scaffold::create_project(&name, &template).await?;
    // The new project needs a vhost before it can serve.
    sync_vhosts_and_reload(&manager, "after creating a project");
    Ok(())
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
    manager: State<'_, ServiceManager>,
) -> Result<(), AppError> {
    projects::link(&path, name, domain)?;
    // The newly linked project needs a vhost before it can serve.
    sync_vhosts_and_reload(&manager, "after linking");
    Ok(())
}

/// Forgets a linked project. The folder and everything in it is left
/// exactly as it was — this only removes Rezure's pointer to it.
#[tauri::command]
pub fn unlink_project(id: String, manager: State<'_, ServiceManager>) -> Result<(), AppError> {
    projects::unlink(&id)?;
    sync_vhosts_and_reload(&manager, "after unlinking");
    Ok(())
}
