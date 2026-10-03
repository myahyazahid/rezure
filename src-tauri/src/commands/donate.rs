//! Thin glue between the Support Developer page and `services::donate`.

use crate::services::donate::{self, DonateConfig};
use crate::utils::error::AppError;

#[tauri::command]
pub async fn fetch_donate_config() -> DonateConfig {
    donate::fetch().await
}

/// Opens a donation link in the system's default browser. Restricted to
/// `http`/`https` — these URLs come from `rezure-dashboard`, not free-typed
/// user input, but the app should still never hand `tauri_plugin_opener` a
/// `file://` or custom-scheme URL just because a server response said to.
#[tauri::command]
pub fn open_external_link(url: String) -> Result<(), AppError> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(AppError::Io(format!(
            "refusing to open non-http(s) URL: {url}"
        )));
    }
    tauri_plugin_opener::open_url(url, None::<&str>)
        .map_err(|e| AppError::Io(format!("could not open the link: {e}")))
}
