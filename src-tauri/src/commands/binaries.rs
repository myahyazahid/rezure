use tauri::AppHandle;

use crate::services::binaries::{self, BinaryStatus};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_binaries() -> Vec<BinaryStatus> {
    binaries::list_status()
}

#[tauri::command]
pub async fn install_binary(app: AppHandle, id: String) -> Result<BinaryStatus, AppError> {
    binaries::install(&app, &id).await
}
