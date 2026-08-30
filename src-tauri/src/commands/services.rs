use tauri::State;

use crate::services::{ServiceInfo, ServiceManager};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_services(manager: State<'_, ServiceManager>) -> Vec<ServiceInfo> {
    manager.list()
}

// Spawning/killing a real process (and, for MariaDB's first run, waiting on
// `mariadb-install-db`) can briefly block — these are `async` and hand the
// actual work to `spawn_blocking` so they never tie up the async runtime.

#[tauri::command]
pub async fn start_service(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    tokio::task::spawn_blocking(move || service.start())
        .await
        .map_err(|e| AppError::ProcessSpawnFailed {
            name: id,
            reason: format!("background task panicked: {e}"),
        })?
}

#[tauri::command]
pub async fn stop_service(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<ServiceInfo, AppError> {
    let service = manager.find(&id)?;
    tokio::task::spawn_blocking(move || service.stop())
        .await
        .map_err(|e| AppError::ProcessSpawnFailed {
            name: id,
            reason: format!("background task panicked: {e}"),
        })?
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
