use tauri::{AppHandle, State};

use crate::config::device::DeviceIdState;
use crate::config::settings::SettingsState;
use crate::db::DbState;
use crate::services::ports::{self, PortHolder};
use crate::services::telemetry::TelemetryClient;
use crate::services::{ServiceInfo, ServiceManager};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_services(manager: State<'_, ServiceManager>) -> Vec<ServiceInfo> {
    manager.list()
}

/// Queues a `service.start`/`service.stop` event — best-effort, the same as
/// every other telemetry call site: a failure here must never fail the
/// service action itself, so it's only ever logged.
fn record_service_event(
    app: &AppHandle,
    db: &DbState,
    settings: &SettingsState,
    device: &DeviceIdState,
    event_type: &str,
    service_name: &str,
) {
    let share_usage_data = settings.0.lock().unwrap().share_usage_data;
    let app_version = app.package_info().version.to_string();
    let conn = db.0.lock().unwrap();
    if let Err(err) = TelemetryClient::record_event(
        &conn,
        share_usage_data,
        &device.0,
        event_type,
        Some(service_name),
        &app_version,
    ) {
        log::warn!("could not record {event_type} event: {err}");
    }
}

// Spawning/killing a real process (and, for MariaDB's first run, waiting on
// `mariadb-install-db`) can briefly block — these are `async` and hand the
// actual work to `spawn_blocking` so they never tie up the async runtime.

#[tauri::command]
pub async fn start_service(
    id: String,
    manager: State<'_, ServiceManager>,
    app: AppHandle,
    db: State<'_, DbState>,
    settings: State<'_, SettingsState>,
    device: State<'_, DeviceIdState>,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    let info = tokio::task::spawn_blocking(move || service.start())
        .await
        .map_err(|e| AppError::ProcessSpawnFailed {
            name: id,
            reason: format!("background task panicked: {e}"),
        })??;
    record_service_event(&app, &db, &settings, &device, "service.start", &info.name);
    Ok(info)
}

#[tauri::command]
pub async fn stop_service(
    id: String,
    manager: State<'_, ServiceManager>,
    app: AppHandle,
    db: State<'_, DbState>,
    settings: State<'_, SettingsState>,
    device: State<'_, DeviceIdState>,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    let info = tokio::task::spawn_blocking(move || service.stop())
        .await
        .map_err(|e| AppError::ProcessSpawnFailed {
            name: id,
            reason: format!("background task panicked: {e}"),
        })??;
    record_service_event(&app, &db, &settings, &device, "service.stop", &info.name);
    Ok(info)
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
    tokio::task::spawn_blocking(move || service.force_stop())
        .await
        .map_err(|e| AppError::ProcessSpawnFailed {
            name: id,
            reason: format!("background task panicked: {e}"),
        })?
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

#[tauri::command]
pub async fn restart_service(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    tokio::task::spawn_blocking(move || service.restart())
        .await
        .map_err(|e| AppError::ProcessSpawnFailed {
            name: id,
            reason: format!("background task panicked: {e}"),
        })?
}
