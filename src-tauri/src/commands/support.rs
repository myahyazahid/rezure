//! Thin glue between the Support page and `services::support`.

use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::config::device::DeviceIdState;
use crate::services::support::{self, AttachmentInfo, TicketHistoryItem};
use crate::utils::error::AppError;

#[tauri::command]
pub fn inspect_attachment(path: String) -> Result<AttachmentInfo, AppError> {
    support::inspect_attachment(&path)
}

/// Only the fields the frontend can't derive itself — `device_id` comes from
/// `DeviceIdState`, and `app_version`/`os_version` are read here rather than
/// trusted from the caller.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTicketPayload {
    pub client_ticket_id: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub attachment_paths: Vec<String>,
    pub include_system_info: bool,
    pub log_text: Option<String>,
}

#[tauri::command]
pub async fn submit_ticket(
    payload: SubmitTicketPayload,
    app: AppHandle,
    device: State<'_, DeviceIdState>,
) -> Result<(), AppError> {
    let (app_version, os_version) = if payload.include_system_info {
        (
            Some(app.package_info().version.to_string()),
            sysinfo::System::long_os_version(),
        )
    } else {
        (None, None)
    };

    support::submit_ticket(
        &device.0,
        &payload.client_ticket_id,
        &payload.category,
        &payload.title,
        &payload.description,
        app_version.as_deref(),
        os_version.as_deref(),
        &payload.attachment_paths,
        payload.log_text.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn fetch_ticket_history(
    device: State<'_, DeviceIdState>,
) -> Result<Vec<TicketHistoryItem>, AppError> {
    support::fetch_ticket_history(&device.0).await
}
