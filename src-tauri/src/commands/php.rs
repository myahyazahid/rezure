use crate::services::php::{self, PhpVersionStatus};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_php_versions() -> Vec<PhpVersionStatus> {
    php::list()
}

#[tauri::command]
pub fn set_active_php_version(id: String) -> Result<Vec<PhpVersionStatus>, AppError> {
    php::set_active(&id)
}
