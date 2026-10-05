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
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::config::api;
use crate::config::device::DeviceIdState;
use crate::config::settings::SettingsState;
use crate::db;
use crate::db::DbState;
use crate::services::db_engine::Engine;
use crate::services::{db_profiles, mssql_localdb, php, postgres, ServiceInfo};
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
    /// The API's free-form extra context — left off the wire entirely when
    /// there's none, rather than sent as `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<&'a Value>,
    occurred_at: String,
}

#[derive(Serialize)]
struct HeartbeatPayload<'a> {
    device_id: &'a str,
    session_id: &'a str,
    app_version: &'a str,
    os: Option<&'a str>,
    os_version: Option<&'a str>,
    /// The Windows account name (`%USERNAME%`), so the dashboard can tell
    /// installs apart by a human name instead of a uuid — see `device_name`.
    device_name: Option<&'a str>,
    occurred_at: String,
    ended_at: Option<&'a str>,
}

/// The API's `device_name` limit — anything longer is a `422`, which
/// `send_pending` treats as a verdict on the row and drops, heartbeat and all.
const DEVICE_NAME_MAX_CHARS: usize = 64;

/// The Windows account name the app runs under — the `Yahya` in
/// `C:\Users\Yahya`. Read fresh on each heartbeat, so it's never stored
/// locally beyond the queued payload.
pub fn device_name() -> Option<String> {
    normalize_device_name(std::env::var("USERNAME").ok())
}

/// Blank becomes `None`; anything past the API's limit is cut rather than
/// sent and rejected.
fn normalize_device_name(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(DEVICE_NAME_MAX_CHARS).collect())
}

/// One UUID generated at startup, kept in memory only for the life of the
/// process — sent on every heartbeat so the backend can group them into one
/// session.
pub struct SessionIdState(pub String);

pub struct TelemetryClient;

impl TelemetryClient {
    /// Queues a discrete, one-off action (a service starting, an error being
    /// hit, ...) — not for anything periodic, that's `record_heartbeat`.
    /// `context` is the event's optional `payload` object.
    pub fn record_event(
        conn: &Connection,
        share_usage_data: bool,
        device_id: &str,
        event_type: &str,
        event_name: Option<&str>,
        context: Option<&Value>,
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
            payload: context,
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
        device_name: Option<&str>,
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
            device_name,
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

/// Queues an event from anywhere holding an `AppHandle`, looking up the
/// opt-in flag, device id and queue itself. Best-effort like every other
/// telemetry call: failures are logged, never returned, since recording an
/// action must never fail the action.
///
/// Takes the database lock, so it must not run on a thread that may already
/// hold it — see the crash sink in `services::process`, which calls this
/// from its own thread for that reason.
pub fn record(app: &AppHandle, event_type: &str, event_name: Option<&str>, context: Option<Value>) {
    let (Some(settings), Some(device), Some(db)) = (
        app.try_state::<SettingsState>(),
        app.try_state::<DeviceIdState>(),
        app.try_state::<DbState>(),
    ) else {
        return;
    };
    let share_usage_data = settings.0.lock().unwrap().share_usage_data;
    let app_version = app.package_info().version.to_string();
    let conn = db.0.lock().unwrap();
    if let Err(err) = TelemetryClient::record_event(
        &conn,
        share_usage_data,
        &device.0,
        event_type,
        event_name,
        context.as_ref(),
        &app_version,
    ) {
        log::warn!("could not record {event_type} event: {err}");
    }
}

/// Queues an `error.report` for a failed `action` on `service_id`. The
/// event's name is the error's kind (`AppError::code`) — nothing from its
/// message goes out, since that's where paths and project names live.
pub fn record_error(app: &AppHandle, err: &AppError, action: &str, service_id: &str) {
    record(
        app,
        "error.report",
        Some(err.code()),
        Some(error_context(action, service_id)),
    );
}

/// Queues an `error.report` for a service that died on its own, with no
/// `AppError` behind it — reported as `ServiceCrashed` next to the error
/// kinds `record_error` sends.
pub fn record_crash(app: &AppHandle, service_id: &str) {
    record(
        app,
        "error.report",
        Some("ServiceCrashed"),
        Some(error_context("service.run", service_id)),
    );
}

fn error_context(action: &str, service_id: &str) -> Value {
    json!({ "action": action, "service": service_id })
}

/// The versions in use when `started` came up — what a `service.start` event
/// carries so the dashboard can chart which stacks people run. A PHP service
/// reports its own version (a project's pinned one, say); anything else
/// reports the active PHP.
///
/// Every database Rezure can run is reported when it's installed, not only
/// the one being started: starting Nginx says which stack it starts *in*.
pub fn stack_context(started: &ServiceInfo) -> Option<Value> {
    let php_version = if started.id.starts_with("php") && !started.version.is_empty() {
        started.version.clone()
    } else {
        php::active_id()
    };
    stack_payload(&StackVersions {
        php: php_version,
        database: db_profiles::active().map(|profile| (profile.engine, profile.version)),
        postgres: postgres::active_id(),
        sqlserver: mssql_localdb::release_label(),
    })
}

/// What [`stack_payload`] reports. An empty or absent field means "not
/// installed", and its key is left out.
#[derive(Debug, Default)]
struct StackVersions {
    php: String,
    /// The MySQL-family server of the active database profile.
    database: Option<(Engine, String)>,
    /// The active PostgreSQL version, bare (`17.2`).
    postgres: String,
    /// Already worded for the dashboard (`SQL Server 2022 LocalDB`).
    sqlserver: Option<String>,
}

/// Versions only, in the shape `laravel-api` reads: `php_version` bare, and
/// each database under its own key with the engine named in the value
/// (`"MariaDB 11.8.9"`, `"PostgreSQL 17.2"`). `None` when nothing is known.
///
/// The dashboard's top-stack-combos chart reads `php_version` and
/// `mysql_version`/`mariadb_version`; the other keys are stored with the event
/// and ignored by it until the dashboard learns them (the API accepts any
/// `payload` object).
fn stack_payload(stack: &StackVersions) -> Option<Value> {
    let mut payload = serde_json::Map::new();
    if !stack.php.is_empty() {
        payload.insert("php_version".to_string(), json!(stack.php));
    }
    if let Some((engine, version)) = stack.database.as_ref().filter(|(_, v)| !v.is_empty()) {
        let key = match engine {
            Engine::MySql => "mysql_version",
            Engine::MariaDb => "mariadb_version",
        };
        payload.insert(
            key.to_string(),
            json!(format!("{} {version}", engine.label())),
        );
    }
    if !stack.postgres.is_empty() {
        payload.insert(
            "postgres_version".to_string(),
            json!(format!("PostgreSQL {}", stack.postgres)),
        );
    }
    if let Some(sqlserver) = stack.sqlserver.as_ref().filter(|v| !v.is_empty()) {
        payload.insert("sqlserver_version".to_string(), json!(sqlserver));
    }
    (!payload.is_empty()).then_some(Value::Object(payload))
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
                discard_row(&db_state, &row.id);
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
            Ok(response) if is_permanent_rejection(response.status()) => {
                log::warn!(
                    "telemetry send to {url} rejected ({}), dropping row {}",
                    response.status(),
                    row.id
                );
                discard_row(&db_state, &row.id);
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

/// A 4xx the same row would get again on every retry — a validation failure
/// (`422`), say. `408` and `429` are about timing, not the row, so those
/// stay queued; `5xx` and network errors do too.
fn is_permanent_rejection(status: reqwest::StatusCode) -> bool {
    status.is_client_error()
        && status != reqwest::StatusCode::REQUEST_TIMEOUT
        && status != reqwest::StatusCode::TOO_MANY_REQUESTS
}

fn discard_row(db_state: &DbState, id: &str) {
    let conn = db_state.0.lock().unwrap();
    if let Err(err) = db::telemetry::discard(&conn, id) {
        log::warn!("could not drop telemetry row {id}: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_migrations_for_test;

    fn queued_payload(conn: &Connection) -> Value {
        let payload: String = conn
            .query_row("SELECT payload FROM pending_events", [], |row| row.get(0))
            .unwrap();
        serde_json::from_str(&payload).unwrap()
    }

    #[test]
    fn opted_out_records_nothing() {
        let conn = init_migrations_for_test();
        TelemetryClient::record_event(
            &conn,
            false,
            "device-1",
            "service.start",
            None,
            None,
            "1.0.0",
        )
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
            None,
            "1.0.0",
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert!(queued_payload(&conn).get("payload").is_none());
    }

    #[test]
    fn an_events_context_goes_out_as_its_payload() {
        let conn = init_migrations_for_test();
        let context = error_context("service.start", "nginx");
        TelemetryClient::record_event(
            &conn,
            true,
            "device-1",
            "error.report",
            Some("PortInUse"),
            Some(&context),
            "1.0.0",
        )
        .unwrap();

        let queued = queued_payload(&conn);
        assert_eq!(queued["event_name"], "PortInUse");
        assert_eq!(
            queued["payload"],
            json!({ "action": "service.start", "service": "nginx" })
        );
    }

    fn stack(php: &str) -> StackVersions {
        StackVersions {
            php: php.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn stack_payload_names_the_database_engine_the_way_the_dashboard_reads_it() {
        assert_eq!(
            stack_payload(&StackVersions {
                database: Some((Engine::MariaDb, "11.8.9".to_string())),
                ..stack("8.3.33")
            }),
            Some(json!({ "php_version": "8.3.33", "mariadb_version": "MariaDB 11.8.9" }))
        );
        assert_eq!(
            stack_payload(&StackVersions {
                database: Some((Engine::MySql, "8.4.2".to_string())),
                ..stack("8.2.30")
            }),
            Some(json!({ "php_version": "8.2.30", "mysql_version": "MySQL 8.4.2" }))
        );
    }

    #[test]
    fn stack_payload_leaves_out_what_isnt_installed() {
        assert_eq!(
            stack_payload(&stack("8.3.33")),
            Some(json!({ "php_version": "8.3.33" }))
        );
        assert_eq!(
            stack_payload(&StackVersions {
                database: Some((Engine::MariaDb, String::new())),
                ..stack("")
            }),
            None
        );
        assert_eq!(stack_payload(&StackVersions::default()), None);
    }

    /// Alongside the existing keys, not instead of them: the dashboard's
    /// combos chart still finds `php_version` and `mariadb_version` exactly
    /// where it always did.
    #[test]
    fn postgres_and_sql_server_are_reported_beside_the_existing_keys() {
        assert_eq!(
            stack_payload(&StackVersions {
                database: Some((Engine::MariaDb, "11.8.9".to_string())),
                postgres: "17.2".to_string(),
                sqlserver: Some("SQL Server 2022 LocalDB".to_string()),
                ..stack("8.3.33")
            }),
            Some(json!({
                "php_version": "8.3.33",
                "mariadb_version": "MariaDB 11.8.9",
                "postgres_version": "PostgreSQL 17.2",
                "sqlserver_version": "SQL Server 2022 LocalDB",
            }))
        );
    }

    /// A machine with only PostgreSQL still reports it — the dashboard just
    /// has no database key it knows how to chart for it yet.
    #[test]
    fn postgres_alone_is_still_reported() {
        assert_eq!(
            stack_payload(&StackVersions {
                postgres: "18.1".to_string(),
                ..stack("8.3.33")
            }),
            Some(json!({ "php_version": "8.3.33", "postgres_version": "PostgreSQL 18.1" }))
        );
        // Nothing installed means no key, not an empty one.
        assert_eq!(
            stack_payload(&StackVersions {
                sqlserver: Some(String::new()),
                ..stack("8.3.33")
            }),
            Some(json!({ "php_version": "8.3.33" }))
        );
    }

    /// Prints the stack this machine would report. Run with
    /// `cargo test --lib prints_this_machines_stack -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads this machine's installed runtimes"]
    fn prints_this_machines_stack() {
        let stack = StackVersions {
            php: php::active_id(),
            database: db_profiles::active().map(|p| (p.engine, p.version)),
            postgres: postgres::active_id(),
            sqlserver: mssql_localdb::release_label(),
        };
        println!("{stack:?}");
        println!("{:#}", stack_payload(&stack).unwrap_or(Value::Null));
    }

    #[test]
    fn only_a_rejection_of_the_row_itself_drops_it() {
        use reqwest::StatusCode;

        assert!(is_permanent_rejection(StatusCode::UNPROCESSABLE_ENTITY));
        assert!(is_permanent_rejection(StatusCode::NOT_FOUND));
        assert!(!is_permanent_rejection(StatusCode::TOO_MANY_REQUESTS));
        assert!(!is_permanent_rejection(StatusCode::REQUEST_TIMEOUT));
        assert!(!is_permanent_rejection(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!is_permanent_rejection(StatusCode::ACCEPTED));
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
            Some("Yahya"),
            None,
        )
        .unwrap();

        let payload: String = conn
            .query_row("SELECT payload FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert!(payload.contains("occurred_at"));
        assert!(payload.contains("session-1"));
        assert_eq!(queued_payload(&conn)["device_name"], "Yahya");
    }

    #[test]
    fn opted_out_queues_no_heartbeat_and_so_no_device_name() {
        let conn = init_migrations_for_test();
        TelemetryClient::record_heartbeat(
            &conn,
            false,
            "device-1",
            "session-1",
            "1.0.0",
            None,
            None,
            Some("Yahya"),
            None,
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn device_name_is_trimmed_blank_is_dropped_and_long_is_cut_to_the_api_limit() {
        assert_eq!(
            normalize_device_name(Some(" Yahya ".to_string())),
            Some("Yahya".to_string())
        );
        assert_eq!(normalize_device_name(Some("   ".to_string())), None);
        assert_eq!(normalize_device_name(None), None);
        assert_eq!(
            normalize_device_name(Some("é".repeat(70))).map(|name| name.chars().count()),
            Some(DEVICE_NAME_MAX_CHARS)
        );
    }
}
