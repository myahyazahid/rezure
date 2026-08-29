use tauri::State;

use crate::services::{ServiceInfo, ServiceManager};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_services(manager: State<'_, ServiceManager>) -> Vec<ServiceInfo> {
    manager.list()
}

#[tauri::command]
pub fn start_service(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<ServiceInfo, AppError> {
    manager.find(&id)?.start()
}

#[tauri::command]
pub fn stop_service(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<ServiceInfo, AppError> {
    manager.find(&id)?.stop()
}

#[tauri::command]
pub fn restart_service(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<ServiceInfo, AppError> {
    manager.find(&id)?.restart()
}
