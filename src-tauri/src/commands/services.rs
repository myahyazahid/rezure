use tauri::{AppHandle, Manager, State};

use crate::config::settings::{Settings, SettingsState};
use crate::services::dev_servers;
use crate::services::ports::{self, PortHolder};
use crate::services::service_visibility::{self, ManagedService};
use crate::services::start_on_launch;
use crate::services::telemetry;
use crate::services::{ServiceInfo, ServiceManager, ServiceStatus};
use crate::utils::error::AppError;

/// The Services page's rows — every service not removed with Manage
/// services.
#[tauri::command]
pub fn list_services(
    manager: State<'_, ServiceManager>,
    settings: State<'_, SettingsState>,
) -> Vec<ServiceInfo> {
    service_visibility::visible(manager.list(), &service_visibility::hidden(&settings))
}

/// The Manage services list: every service, and whether it's on the page.
#[tauri::command]
pub fn list_managed_services(
    manager: State<'_, ServiceManager>,
    settings: State<'_, SettingsState>,
) -> Vec<ManagedService> {
    service_visibility::catalog(&manager.list(), &service_visibility::hidden(&settings))
}

/// Adds a service to the Services page or removes it, returning the updated
/// Manage services list. Removing a running service stops it, which for a
/// database can take a while — hence off the async runtime.
#[tauri::command]
pub async fn set_service_shown(
    id: String,
    shown: bool,
    app: AppHandle,
) -> Result<Vec<ManagedService>, AppError> {
    tokio::task::spawn_blocking(move || {
        let manager = app.state::<ServiceManager>();
        let settings = app.state::<SettingsState>();
        service_visibility::set_shown(&manager, &settings, &id, shown)?;
        Ok(service_visibility::catalog(
            &manager.list(),
            &service_visibility::hidden(&settings),
        ))
    })
    .await
    .map_err(|e| AppError::ProcessSpawnFailed {
        name: "manage services".to_string(),
        reason: format!("background task panicked: {e}"),
    })?
}

/// Marks a service to start, or not, as soon as Rezure opens, returning the
/// settings as they now stand — see `services::start_on_launch`.
#[tauri::command]
pub fn set_service_starts_on_launch(
    id: String,
    on: bool,
    manager: State<'_, ServiceManager>,
    settings: State<'_, SettingsState>,
) -> Result<Settings, AppError> {
    start_on_launch::set(&manager, &settings, &id, on)
}

// Spawning/killing a real process (and, for MariaDB's first run, waiting on
// `mariadb-install-db`) can briefly block — these are `async` and hand the
// actual work to `spawn_blocking` so they never tie up the async runtime.

/// Runs a blocking service action off the async runtime. A panicked task
/// comes back as the same `AppError` a failed action would.
async fn run_blocking(
    id: &str,
    action: impl FnOnce() -> Result<ServiceInfo, AppError> + Send + 'static,
) -> Result<ServiceInfo, AppError> {
    tokio::task::spawn_blocking(action)
        .await
        .map_err(|e| AppError::ProcessSpawnFailed {
            name: id.to_string(),
            reason: format!("background task panicked: {e}"),
        })?
}

#[tauri::command]
pub async fn start_service(
    id: String,
    manager: State<'_, ServiceManager>,
    app: AppHandle,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    let result = run_blocking(&id, move || service.start()).await;
    // A dev server's id and name are its project's — never reported.
    if dev_servers::is_dev_service(&id) {
        return result;
    }
    match &result {
        Ok(info) => telemetry::record(
            &app,
            "service.start",
            Some(&info.name),
            telemetry::stack_context(info),
        ),
        Err(err) => telemetry::record_error(&app, err, "service.start", &id),
    }
    result
}

#[tauri::command]
pub async fn stop_service(
    id: String,
    manager: State<'_, ServiceManager>,
    app: AppHandle,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    let result = run_blocking(&id, move || service.stop()).await;
    if dev_servers::is_dev_service(&id) {
        return result;
    }
    match &result {
        Ok(info) => telemetry::record(&app, "service.stop", Some(&info.name), None),
        Err(err) => telemetry::record_error(&app, err, "service.stop", &id),
    }
    result
}

/// Kills the service outright, skipping the clean shutdown `stop_service`
/// attempts.
///
/// Exists because that shutdown has a timeout, and a server that has hung
/// makes the user wait it out with no way to intervene. The frontend
/// confirms first for anything with state to lose — see `ServiceRow.vue`.
#[tauri::command]
pub async fn force_stop_service(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    run_blocking(&id, move || service.force_stop()).await
}

/// Who is holding the port a service wants, so a "port in use" failure can
/// name the culprit instead of leaving the user to find it with `netstat`.
///
/// Returns `None` when the port is free — worth checking, since the holder
/// may well have exited between the failed start and the user reading it.
#[tauri::command]
pub async fn port_holder(port: u16) -> Option<PortHolder> {
    tokio::task::spawn_blocking(move || ports::holder(port))
        .await
        .unwrap_or(None)
}

/// Stops whatever is holding `port`, then reports what (if anything) still
/// is.
///
/// This is the "it says the port is taken, take it back" action. A process
/// is killed; a Windows service is stopped through a UAC prompt, and with
/// `disable_autostart` set to start Manually. It refuses protected system
/// processes — port 80 belonging to `System` means IIS or the Windows HTTP
/// service, which can't be traced to one service from the port alone.
/// Starting the service afterwards stays a separate, explicit step: freeing
/// a port and claiming it are different decisions, and bundling them would
/// hide a failure of one behind the other.
#[tauri::command]
pub async fn free_port(
    port: u16,
    disable_autostart: Option<bool>,
) -> Result<Option<PortHolder>, AppError> {
    tokio::task::spawn_blocking(move || {
        ports::reclaim(port, disable_autostart.unwrap_or(false))?;
        Ok(ports::holder(port))
    })
    .await
    .map_err(|e| AppError::Io(format!("background task panicked: {e}")))?
}

/// Opens a running service's web UI (Mailpit's inbox) in the default
/// browser. The address comes from the service itself, never from the
/// frontend, so this can only ever open what a service reports serving.
#[tauri::command]
pub fn open_service_ui(id: String, manager: State<'_, ServiceManager>) -> Result<(), AppError> {
    let info = manager.find(&id)?.info();
    let url = info
        .web_url
        .ok_or_else(|| AppError::NoWebUi(info.name.clone()))?;
    if info.status != ServiceStatus::Running {
        return Err(AppError::ServiceNotRunning(info.name));
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| AppError::OpenFailed {
        target: url,
        reason: e.to_string(),
    })
}

#[tauri::command]
pub async fn restart_service(
    id: String,
    manager: State<'_, ServiceManager>,
    app: AppHandle,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    let result = run_blocking(&id, move || service.restart()).await;
    if dev_servers::is_dev_service(&id) {
        return result;
    }
    if let Err(err) = &result {
        telemetry::record_error(&app, err, "service.restart", &id);
    }
    result
}
