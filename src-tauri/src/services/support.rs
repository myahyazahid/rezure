//! Support ticket submission and history — calls `rezure-dashboard`'s
//! `POST /support/tickets` and `GET /support/tickets`. See
//! `api-documentation/telemetry-api.md` (sibling repo) for the full contract.
//!
//! Unlike telemetry (queued locally, sent in the background), a ticket is a
//! direct, user-initiated action: it's sent immediately and the UI shows a
//! real-time result, so there's no local queue here.

use std::path::Path;
use std::time::Duration;

use reqwest::multipart;
use serde::{Deserialize, Serialize};

use crate::config::api;
use crate::utils::error::AppError;

const MAX_ATTACHMENTS: usize = 5;
const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "txt", "log", "zip"];
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketHistoryItem {
    pub category: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Stats and validates one file before it's added to a ticket draft — the
/// closest equivalent to client-side validation Tauri's process-separated
/// model allows: the frontend only ever gets a path from the file picker, so
/// Rust has to be the one to check it.
pub fn inspect_attachment(path: &str) -> Result<AttachmentInfo, AppError> {
    let path_ref = Path::new(path);
    let ext = extension_of(path_ref);
    if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
        return Err(AppError::AttachmentRejected {
            path: path.to_string(),
            reason: format!(
                "unsupported file type \"{ext}\" — allowed: {}",
                ALLOWED_EXTENSIONS.join(", ")
            ),
        });
    }

    let metadata = std::fs::metadata(path_ref).map_err(|e| AppError::AttachmentRejected {
        path: path.to_string(),
        reason: format!("could not read the file: {e}"),
    })?;
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        return Err(AppError::AttachmentRejected {
            path: path.to_string(),
            reason: "file is larger than the 10MB limit".to_string(),
        });
    }

    let name = path_ref
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string();

    Ok(AttachmentInfo {
        name,
        size_bytes: metadata.len(),
    })
}

fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| AppError::TicketSubmitFailed(format!("could not set up the request: {e}")))
}

async fn error_message_from_response(response: reqwest::Response) -> String {
    let status = response.status();
    match response.text().await {
        Ok(body) if !body.is_empty() => {
            #[derive(Deserialize)]
            struct ErrorBody {
                message: String,
            }
            match serde_json::from_str::<ErrorBody>(&body) {
                Ok(parsed) => parsed.message,
                Err(_) => format!("server returned {status}"),
            }
        }
        _ => format!("server returned {status}"),
    }
}

/// Submits a bug report / feature request / general feedback ticket, with up
/// to 5 optional file attachments. `attachment_paths` must already have
/// passed `inspect_attachment` — this re-validates anyway (cheap, and this is
/// the boundary that actually matters before spending a network call).
///
/// `client_ticket_id` is the idempotency key: the caller must pass the same
/// value on every retry of the same logical submission, never a fresh one.
#[allow(clippy::too_many_arguments)]
pub async fn submit_ticket(
    device_id: &str,
    client_ticket_id: &str,
    category: &str,
    title: &str,
    description: &str,
    app_version: Option<&str>,
    os_version: Option<&str>,
    attachment_paths: &[String],
    log_text: Option<&str>,
) -> Result<(), AppError> {
    if attachment_paths.len() > MAX_ATTACHMENTS {
        return Err(AppError::AttachmentRejected {
            path: String::new(),
            reason: format!("at most {MAX_ATTACHMENTS} attachments are allowed"),
        });
    }

    let mut form = multipart::Form::new()
        .text("device_id", device_id.to_string())
        .text("client_ticket_id", client_ticket_id.to_string())
        .text("category", category.to_string())
        .text("title", title.to_string())
        .text("description", description.to_string());

    if let Some(version) = app_version {
        form = form.text("app_version", version.to_string());
    }
    if let Some(version) = os_version {
        form = form.text("os_version", version.to_string());
    }

    for path in attachment_paths {
        inspect_attachment(path)?;
        let path_ref = Path::new(path);
        let bytes = tokio::fs::read(path_ref)
            .await
            .map_err(|e| AppError::AttachmentRejected {
                path: path.clone(),
                reason: format!("could not read the file: {e}"),
            })?;
        let name = path_ref
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();
        let mime = mime_for_extension(&extension_of(path_ref));
        let part = multipart::Part::bytes(bytes)
            .file_name(name)
            .mime_str(mime)
            .map_err(|e| AppError::TicketSubmitFailed(format!("invalid attachment: {e}")))?;
        form = form.part("attachments[]", part);
    }

    if let Some(log_text) = log_text {
        let part = multipart::Part::bytes(log_text.as_bytes().to_vec())
            .file_name("latest-log.txt")
            .mime_str("text/plain")
            .map_err(|e| AppError::TicketSubmitFailed(format!("invalid log attachment: {e}")))?;
        form = form.part("attachments[]", part);
    }

    let url = format!("{}/api/v1/support/tickets", api::base_url());
    let response = client()?
        .post(url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| AppError::TicketSubmitFailed(e.to_string()))?;

    if response.status().is_success() {
        return Ok(());
    }

    let retry_after = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let status = response.status();
    let message = error_message_from_response(response).await;

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let suffix = retry_after
            .map(|s| format!(" — try again in {s}s"))
            .unwrap_or_default();
        return Err(AppError::TicketSubmitFailed(format!(
            "too many attempts{suffix}"
        )));
    }

    Err(AppError::TicketSubmitFailed(message))
}

/// Per-device ticket history — up to 50 tickets, newest first.
pub async fn fetch_ticket_history(device_id: &str) -> Result<Vec<TicketHistoryItem>, AppError> {
    let url = format!("{}/api/v1/support/tickets", api::base_url());
    let response = client()?
        .get(url)
        .query(&[("device_id", device_id)])
        .send()
        .await
        .map_err(|e| AppError::TicketHistoryFailed(e.to_string()))?;

    if !response.status().is_success() {
        let message = error_message_from_response(response).await;
        return Err(AppError::TicketHistoryFailed(message));
    }

    response
        .json::<Vec<TicketHistoryItem>>()
        .await
        .map_err(|e| AppError::TicketHistoryFailed(format!("unexpected response: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_disallowed_extension() {
        let path = std::env::temp_dir().join("rezure-test-attachment.exe");
        std::fs::write(&path, b"data").unwrap();

        let result = inspect_attachment(path.to_str().unwrap());
        assert!(result.is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn accepts_an_allowed_small_file() {
        let path = std::env::temp_dir().join("rezure-test-attachment.txt");
        std::fs::write(&path, b"hello").unwrap();

        let info = inspect_attachment(path.to_str().unwrap()).unwrap();
        assert_eq!(info.size_bytes, 5);
        std::fs::remove_file(&path).unwrap();
    }
}
