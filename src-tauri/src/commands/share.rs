use tauri::AppHandle;

use crate::services::share;
use crate::utils::error::AppError;

/// Starts sharing a project publicly via a Cloudflare Quick Tunnel, or
/// returns the URL of one already running for it.
#[tauri::command]
pub async fn share_project(id: String, app: AppHandle) -> Result<String, AppError> {
    let exe = share::ensure_installed(&app).await?;
    share::start(&exe, &id).await
}

#[tauri::command]
pub fn stop_sharing(id: String) {
    share::close(&id);
}

/// The URL of a share already running for `id`, without starting one — so
/// the Projects page can restore its state after a reload instead of
/// showing "not shared" for a tunnel that's actually still up.
#[tauri::command]
pub fn sharing_status(id: String) -> Option<String> {
    share::existing_url(&id)
}
