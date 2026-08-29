use tauri::State;

use crate::services::php::{PhpVersion, PhpVersionManager};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_php_versions(manager: State<'_, PhpVersionManager>) -> Vec<PhpVersion> {
    manager.list()
}

#[tauri::command]
pub fn set_active_php_version(
    id: String,
    manager: State<'_, PhpVersionManager>,
) -> Result<Vec<PhpVersion>, AppError> {
    manager.set_active(&id)
}
