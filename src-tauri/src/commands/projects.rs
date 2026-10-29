use tauri::{AppHandle, State};

use crate::commands::dev_servers::start_and_wait;
use crate::config;
use crate::db::projects::ProjectInfo;
use crate::db::{self, DbState};
use crate::services::dev_servers::{self, DevSpec};
use crate::services::frameworks::{self, FrameworkCatalog, ServeProfile};
use crate::services::scaffold::ProjectTemplate;
use crate::services::{
    hosts, launcher, projects, scaffold, telemetry, vhosts, ServiceManager, ServiceStatus,
};
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
/// actually changed something, makes a running nginx pick it up. Also
/// reconciles the pooled PHP services (`ServiceManager::sync_php_pool`)
/// against whatever versions the current projects pin — unconditionally,
/// since that's cheap and idempotent even when nothing else changed.
///
/// Reads each project's PHP version override from SQLite first — a
/// filesystem scan alone can't know it (see `db::projects::fetch_php_versions`),
/// and skipping this step would make every pin silently a no-op: every vhost
/// would keep resolving to the global default port regardless of what it's
/// pinned to.
///
/// `vhosts::sync_vhosts` is handed `ServiceManager::sync_php_pool` itself as
/// its port resolver, rather than this function computing ports separately —
/// `ServiceManager` is the only thing that knows which port an
/// already-registered pooled service is really bound to, so routing every
/// vhost write through it is what keeps a pinned project's port from ever
/// drifting out from under its own running `php-cgi` (see
/// `php_pool::assign_ports`'s doc comment for the bug this fixes).
///
/// The `changed` check on the *vhost* reload is what lets this be safe to
/// call from `list_projects`, which runs on every visit to the Projects
/// page: without it, simply opening that page would cycle nginx's workers.
///
/// Also reconciles the dev servers (`ServiceManager::sync_dev_servers`)
/// against the same scan, with each project's Node.js pin from SQLite, and
/// returns their specs — so a caller about to start one starts it as it is
/// now, not as it was when the app opened.
///
/// Best-effort throughout — a vhost hiccup should leave a warning in the log,
/// not fail the command the user actually asked for.
pub(crate) fn sync_vhosts_and_reload(
    manager: &ServiceManager,
    db_state: &DbState,
    context: &str,
) -> Vec<DevSpec> {
    let (php_versions, node_versions) = {
        let conn = db_state.0.lock().unwrap();
        let php = db::projects::fetch_php_versions(&conn).unwrap_or_else(|err| {
            log::warn!("failed to load PHP version overrides {context}: {err}");
            Default::default()
        });
        let node = db::projects::fetch_node_versions(&conn).unwrap_or_else(|err| {
            log::warn!("failed to load Node.js version overrides {context}: {err}");
            Default::default()
        });
        (php, node)
    };

    match vhosts::sync_vhosts(&php_versions, |wanted| manager.sync_php_pool(wanted)) {
        Ok(sync) => {
            if sync.changed {
                reload_nginx_if_running(manager);
            }
            let specs = dev_servers::specs(
                &sync.projects,
                &frameworks::registry(),
                &config::dev_servers::load(),
                &node_versions,
            );
            manager.sync_dev_servers(&specs);
            specs
        }
        Err(err) => {
            log::warn!("failed to sync nginx vhosts {context}: {err}");
            Vec::new()
        }
    }
}

#[tauri::command]
pub fn list_projects(
    db_state: State<'_, DbState>,
    manager: State<'_, ServiceManager>,
) -> Result<Vec<ProjectInfo>, AppError> {
    // A folder dropped into `www` by hand shows up in this scan and nowhere
    // else, so this is the only place its vhost gets written — and without
    // the reload nginx would keep 404ing it until its next restart. Before
    // the scan below, so a `proxy` project's port picked by this sync is in
    // the list already.
    sync_vhosts_and_reload(&manager, &db_state, "while listing projects");
    let mut detected = projects::scan_projects()?;

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
    match db::projects::fetch_php_versions(&conn) {
        Ok(versions) => {
            for project in &mut detected {
                if let Some(version) = versions.get(&project.id) {
                    project.php_version = version.clone();
                }
            }
        }
        Err(err) => log::warn!("failed to load project PHP version overrides: {err}"),
    }
    match db::projects::fetch_node_versions(&conn) {
        Ok(versions) => {
            for project in &mut detected {
                if let Some(version) = versions.get(&project.id) {
                    project.node_version = version.clone();
                }
            }
        }
        Err(err) => log::warn!("failed to load project Node.js version overrides: {err}"),
    }

    Ok(detected)
}

/// Pins (or, with `version: null`, clears back to the global default) the
/// PHP version this one project is served by — see `services::php_pool` for
/// what that makes possible. Re-syncs the vhosts (and, through it, the PHP
/// pool) immediately, so the change is live without the user needing to
/// revisit the Projects page first.
#[tauri::command]
pub fn set_project_php_version(
    id: String,
    version: Option<String>,
    db_state: State<'_, DbState>,
    manager: State<'_, ServiceManager>,
) -> Result<(), AppError> {
    let conn = db_state.0.lock().unwrap();
    db::projects::set_php_version(&conn, &id, version.as_deref())?;
    drop(conn);
    sync_vhosts_and_reload(&manager, &db_state, "after setting a project's PHP version");
    Ok(())
}

/// Pins (or, with `version: null`, clears back to the global default) the
/// Node.js version this one project's terminal and dev server resolve.
///
/// The resync is for the dev server alone (nothing in nginx depends on
/// Node): its spec carries the version, so a stopped one is rebuilt with the
/// new one at once, and a running one at its next stop.
#[tauri::command]
pub fn set_project_node_version(
    id: String,
    version: Option<String>,
    db_state: State<'_, DbState>,
    manager: State<'_, ServiceManager>,
) -> Result<(), AppError> {
    let conn = db_state.0.lock().unwrap();
    db::projects::set_node_version(&conn, &id, version.as_deref())?;
    drop(conn);
    sync_vhosts_and_reload(
        &manager,
        &db_state,
        "after setting a project's Node.js version",
    );
    Ok(())
}

/// This project's PHP and Node.js version overrides, for
/// `launcher::open_terminal`. Best-effort: a SQLite hiccup degrades to "no
/// pin" (the global active version), not a terminal that fails to open.
fn terminal_pins(db_state: &State<'_, DbState>, id: &str) -> (Option<String>, Option<String>) {
    let conn = db_state.0.lock().unwrap();
    let php = db::projects::php_version_for(&conn, id).unwrap_or_else(|err| {
        log::warn!("failed to load {id}'s PHP version override: {err}");
        None
    });
    let node = db::projects::node_version_for(&conn, id).unwrap_or_else(|err| {
        log::warn!("failed to load {id}'s Node.js version override: {err}");
        None
    });
    (php, node)
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
pub async fn sync_hosts(
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<bool, AppError> {
    sync_vhosts_and_reload(&manager, &db_state, "while syncing hosts");

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
    db_state: State<'_, DbState>,
) -> Result<(), AppError> {
    scaffold::create_project(&name, &template).await?;
    // The new project needs a vhost before it can serve.
    sync_vhosts_and_reload(&manager, &db_state, "after creating a project");
    Ok(())
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
///
/// A `proxy` project with Open-starts-it on (the default) has its dev server
/// started first, and the browser is only opened once that answers — a tab
/// opened sooner would land on the "not running" page. If it fails to come
/// up, the browser isn't opened at all and the error says why.
#[tauri::command]
pub async fn open_project_site(
    id: String,
    db_state: State<'_, DbState>,
    manager: State<'_, ServiceManager>,
) -> Result<(), AppError> {
    let project = projects::find(&id)?;
    if project.serve == ServeProfile::Proxy {
        let specs = sync_vhosts_and_reload(&manager, &db_state, "before opening a project");
        if dev_servers::starts_on_open(project.serve, &id, &specs) {
            let service = manager.find(&dev_servers::service_id(&id))?;
            start_and_wait(service).await?;
        }
    }
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

/// Opens a terminal in the project folder, with this project's effective PHP
/// and Node.js versions (its own pins, or the global active ones) put first
/// on the new terminal's `PATH` — see `services::launcher::open_terminal`.
#[tauri::command]
pub fn open_project_terminal(id: String, db_state: State<'_, DbState>) -> Result<(), AppError> {
    let (php_pin, node_pin) = terminal_pins(&db_state, &id);
    launcher::open_terminal(&id, php_pin.as_deref(), node_pin.as_deref())?;
    mark_opened(&db_state, &id);
    Ok(())
}

/// What linking a folder would produce — name, framework, docroot and the
/// domain it would get — without registering anything. Also where a bad
/// path is refused, so the dialog can say why before the user commits.
///
/// `framework` (an id) or `docroot` (a subfolder, "Custom") previews the
/// user's own choice instead of detection; at most one of them.
#[tauri::command]
pub fn preview_project_link(
    path: String,
    framework: Option<String>,
    docroot: Option<String>,
) -> Result<projects::LinkPreview, AppError> {
    let choice = projects::choice(framework, docroot)?;
    projects::prepare_link(&path, choice.as_ref())
}

/// Registers a folder outside `www` as a project. Records the path only —
/// nothing inside the folder is read, written or moved. `framework` and
/// `docroot` as in [`preview_project_link`].
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn link_project(
    path: String,
    name: Option<String>,
    domain: Option<String>,
    framework: Option<String>,
    docroot: Option<String>,
    app: AppHandle,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<(), AppError> {
    let choice = projects::choice(framework, docroot)?;
    let outcome = projects::link(&path, name, domain, choice)?;
    // The newly linked project needs a vhost before it can serve.
    sync_vhosts_and_reload(&manager, &db_state, "after linking");
    telemetry::record_framework(
        &app,
        "project.linked",
        &outcome.detected_framework,
        outcome.choice.as_ref(),
    );
    Ok(())
}

/// Forgets a linked project. The folder and everything in it is left
/// exactly as it was — this only removes Rezure's pointer to it.
#[tauri::command]
pub fn unlink_project(
    id: String,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<(), AppError> {
    projects::unlink(&id)?;
    sync_vhosts_and_reload(&manager, &db_state, "after unlinking");
    Ok(())
}

/// Deletes a project from `www`: its dev server is stopped first (a running
/// one holds files open, and the move to the Recycle Bin would fail on
/// them), the folder goes to the Recycle Bin, its history and version pins
/// are forgotten, and its vhost is dropped. Linked projects are refused —
/// those are only ever unlinked.
#[tauri::command]
pub async fn delete_project(
    id: String,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<(), AppError> {
    if let Ok(dev) = manager.find(&dev_servers::service_id(&id)) {
        if dev.info().status != ServiceStatus::Stopped {
            tokio::task::spawn_blocking(move || dev.stop())
                .await
                .map_err(|e| {
                    AppError::ProjectDeleteFailed(format!("background task panicked: {e}"))
                })??;
        }
    }

    let target = id.clone();
    tokio::task::spawn_blocking(move || projects::delete(&target))
        .await
        .map_err(|e| AppError::ProjectDeleteFailed(format!("background task panicked: {e}")))??;

    {
        let conn = db_state.0.lock().unwrap();
        if let Err(err) = db::projects::forget(&conn, &id) {
            log::warn!("deleted {id} but couldn't forget its history: {err}");
        }
    }
    sync_vhosts_and_reload(&manager, &db_state, "after deleting a project");
    Ok(())
}

/// Every framework the pickers can offer, plus anything wrong with the
/// definitions or with a project's saved choice.
#[tauri::command]
pub fn list_frameworks() -> Result<FrameworkCatalog, AppError> {
    projects::framework_catalog()
}

/// Re-reads the user's own `etc\frameworks.json` — the only way an edit to
/// it takes effect without restarting — and re-serves every project, since
/// any of them may now be detected differently.
#[tauri::command]
pub fn reload_frameworks(
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<FrameworkCatalog, AppError> {
    frameworks::reload();
    sync_vhosts_and_reload(&manager, &db_state, "after reloading frameworks");
    projects::framework_catalog()
}

/// Serves a project as `framework` (an id) or from `docroot` (a subfolder,
/// "Custom") — or, with neither, goes back to what detection says. Live
/// immediately: the vhosts are rewritten and a running nginx reloaded.
#[tauri::command]
pub fn set_project_framework(
    id: String,
    framework: Option<String>,
    docroot: Option<String>,
    app: AppHandle,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<(), AppError> {
    let choice = projects::choice(framework, docroot)?;
    let change = projects::set_framework(&id, choice)?;
    sync_vhosts_and_reload(&manager, &db_state, "after changing a project's framework");
    telemetry::record_framework(
        &app,
        "project.framework_changed",
        &change.detected_framework,
        change.choice.as_ref(),
    );
    Ok(())
}
