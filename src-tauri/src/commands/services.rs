use tauri::{AppHandle, State};

use crate::services::ports::{self, PortHolder};
use crate::services::telemetry;
use crate::services::{ServiceInfo, ServiceManager, ServiceStatus};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_services(manager: State<'_, ServiceManager>) -> Vec<ServiceInfo> {
    manager.list()
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

/// Kills whatever is holding `port`, then reports what (if anything) still
/// is.
///
/// This is the "it says the port is taken, take it back" action. It refuses
/// protected system processes — port 80 belonging to `System` means IIS or
/// the Windows HTTP service, which has to be stopped as a service, not
/// killed. Starting the service afterwards stays a separate, explicit step:
/// freeing a port and claiming it are different decisions, and bundling
/// them would hide a failure of one behind the other.
#[tauri::command]
pub async fn free_port(port: u16) -> Result<Option<PortHolder>, AppError> {
    tokio::task::spawn_blocking(move || {
        ports::reclaim(port)?;
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
    if let Err(err) = &result {
        telemetry::record_error(&app, err, "service.restart", &id);
    }
    result
}
