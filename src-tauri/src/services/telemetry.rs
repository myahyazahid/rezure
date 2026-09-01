//! Single entry point for recording telemetry locally, and for later sending
//! it — see `api-documentation/telemetry-api.md` (sibling repo) for the
//! payload shapes this mirrors.
//!
//! `TelemetryClient::record_event`/`record_heartbeat` no-op immediately when
//! usage sharing is off: opt-out must stop *recording*, not just sending.
//! `send_pending` re-checks the same flag before sending anything queued
//! earlier — opt-out must stop *sending* too, not just new recording.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;
use tauri::Manager;
use uuid::Uuid;

use crate::config::api;
use crate::config::settings::SettingsState;
use crate::db;
use crate::db::DbState;
use crate::utils::error::AppError;

/// Rows older than this (once sent) are dropped on each send cycle — a
/// bounded local queue, not an audit log.
const RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;
const SEND_BATCH_SIZE: i64 = 20;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Serialize)]
struct EventPayload<'a> {
    device_id: &'a str,
    event_id: String,
    event_type: &'a str,
    event_name: Option<&'a str>,
    app_version: &'a str,
    occurred_at: String,
}

#[derive(Serialize)]
struct HeartbeatPayload<'a> {
    device_id: &'a str,
    session_id: &'a str,
    app_version: &'a str,
    os: Option<&'a str>,
    os_version: Option<&'a str>,
    occurred_at: String,
    ended_at: Option<&'a str>,
}

/// One UUID generated at startup, kept in memory only for the life of the
/// process — sent on every heartbeat so the backend can group them into one
/// session.
pub struct SessionIdState(pub String);

pub struct TelemetryClient;

impl TelemetryClient {
    /// Queues a discrete, one-off action (a service starting, an error being
    /// hit, ...) — not for anything periodic, that's `record_heartbeat`.
    pub fn record_event(
        conn: &Connection,
        share_usage_data: bool,
        device_id: &str,
        event_type: &str,
        event_name: Option<&str>,
        app_version: &str,
    ) -> Result<(), AppError> {
        if !share_usage_data {
            return Ok(());
        }
        let event_id = Uuid::new_v4().to_string();
        let payload = EventPayload {
            device_id,
            event_id: event_id.clone(),
            event_type,
            event_name,
            app_version,
            occurred_at: now_rfc3339(),
        };
        let payload_json = serde_json::to_string(&payload)
            .map_err(|e| AppError::Database(format!("could not serialize event: {e}")))?;
        db::telemetry::insert_pending(conn, &event_id, &payload_json, "event", now())
    }

    /// Queues one "the app is open, on this device" ping for `session_id`,
    /// which stays stable for the life of one app launch. Pass `ended_at`
    /// (RFC 3339) only on the final heartbeat before the app quits.
    #[allow(clippy::too_many_arguments)]
    pub fn record_heartbeat(
        conn: &Connection,
        share_usage_data: bool,
        device_id: &str,
        session_id: &str,
        app_version: &str,
        os: Option<&str>,
        os_version: Option<&str>,
        ended_at: Option<&str>,
    ) -> Result<(), AppError> {
        if !share_usage_data {
            return Ok(());
        }
        let payload = HeartbeatPayload {
            device_id,
            session_id,
            app_version,
            os,
            os_version,
            occurred_at: now_rfc3339(),
            ended_at,
        };
        let payload_json = serde_json::to_string(&payload)
            .map_err(|e| AppError::Database(format!("could not serialize heartbeat: {e}")))?;
        // Heartbeats dedupe server-side on (device_id, session_id), not on
        // this row's id — a local, unique-per-row id is all `pending_events`
        // itself needs.
        let id = Uuid::new_v4().to_string();
        db::telemetry::insert_pending(conn, &id, &payload_json, "heartbeat", now())
    }
}

/// Sends whatever's queued in `pending_events`, one request per row (the real
/// backend has no bulk endpoint — see the module doc on `db::telemetry`).
/// Never panics and never returns an error: every failure is logged and left
/// for the next scheduled call, which is this loop's entire retry strategy.
pub async fn send_pending(app: &tauri::AppHandle) {
    let Some(settings_state) = app.try_state::<SettingsState>() else {
        return;
    };
    let share_usage_data = settings_state.0.lock().unwrap().share_usage_data;
    if !share_usage_data {
        return;
    }

    let Some(db_state) = app.try_state::<DbState>() else {
        return;
    };

    let batch = {
        let conn = db_state.0.lock().unwrap();
        match db::telemetry::fetch_unsent(&conn, SEND_BATCH_SIZE) {
            Ok(rows) => rows,
            Err(err) => {
                log::warn!("could not read pending telemetry: {err}");
                return;
            }
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            log::warn!("could not set up the telemetry client: {err}");
            return;
        }
    };
    let base_url = api::base_url();

    for row in batch {
        let value: serde_json::Value = match serde_json::from_str(&row.payload) {
            Ok(value) => value,
            Err(err) => {
                log::warn!(
                    "dropping unparseable pending telemetry row {}: {err}",
                    row.id
                );
                continue;
            }
        };
        let path = if row.kind == "heartbeat" {
            "telemetry/heartbeat"
        } else {
            "telemetry/event"
        };
        let url = format!("{base_url}/api/v1/{path}");

        match client.post(&url).json(&value).send().await {
            Ok(response) if response.status().is_success() => {
                let conn = db_state.0.lock().unwrap();
                if let Err(err) = db::telemetry::mark_sent(&conn, &row.id, now()) {
                    log::warn!("could not mark telemetry row {} as sent: {err}", row.id);
                }
            }
            Ok(response) if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                log::warn!("telemetry rate-limited — resuming next cycle");
                break;
            }
            Ok(response) => {
                log::warn!("telemetry send to {url} failed: {}", response.status());
            }
            Err(err) => {
                log::warn!("telemetry send to {url} failed: {err}");
            }
        }
    }

    let conn = db_state.0.lock().unwrap();
    if let Err(err) = db::telemetry::delete_sent_before(&conn, now() - RETENTION_SECONDS) {
        log::warn!("could not clean up sent telemetry: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_migrations_for_test;

    #[test]
    fn opted_out_records_nothing() {
        let conn = init_migrations_for_test();
        TelemetryClient::record_event(&conn, false, "device-1", "service.start", None, "1.0.0")
            .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn opted_in_queues_one_event() {
        let conn = init_migrations_for_test();
        TelemetryClient::record_event(
            &conn,
            true,
            "device-1",
            "service.start",
            Some("nginx"),
            "1.0.0",
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn opted_in_queues_one_heartbeat_with_occurred_at() {
        let conn = init_migrations_for_test();
        TelemetryClient::record_heartbeat(
            &conn,
            true,
            "device-1",
            "session-1",
            "1.0.0",
            Some("Windows 11"),
            Some("23H2"),
            None,
        )
        .unwrap();

        let payload: String = conn
            .query_row("SELECT payload FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert!(payload.contains("occurred_at"));
        assert!(payload.contains("session-1"));
    }
}
