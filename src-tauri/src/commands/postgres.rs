//! Thin glue between the Switch page and `services::postgres` /
//! `services::postgres_catalog`, and between the Databases page's Users
//! dialog and `services::postgres_roles`.

use tauri::{AppHandle, Manager, State};

use crate::config::settings::{self, SettingsState};
use crate::services::postgres::{self, PostgresVersionStatus};
use crate::services::postgres_catalog::{self, PostgresRelease};
use crate::services::postgres_roles::{self, ConnectionSnippet, NewRole, PostgresRole};
use crate::services::{connections, ServiceManager};
use crate::utils::error::AppError;

/// The PostgreSQL versions currently on disk, and which one is active.
#[tauri::command]
pub fn list_postgres_versions() -> Vec<PostgresVersionStatus> {
    postgres::list()
}

/// Switches the active PostgreSQL version, restarting a running server on
/// it — see `postgres::switch`. Off the async runtime, since stopping a
/// database can take a while.
#[tauri::command]
pub async fn set_active_postgres_version(
    id: String,
    app: AppHandle,
    settings: State<'_, SettingsState>,
) -> Result<Vec<PostgresVersionStatus>, AppError> {
    let switch_id = id.clone();
    let result = tokio::task::spawn_blocking(move || {
        postgres::switch(&app.state::<ServiceManager>(), &switch_id)
    })
    .await
    .map_err(|e| AppError::ProcessSpawnFailed {
        name: postgres::SERVICE_NAME.to_string(),
        reason: format!("background task panicked: {e}"),
    })?;

    // Persisted whenever the switch itself happened — even if the restart
    // after it failed, the version that's active is the one asked for.
    // Best-effort, like the PHP and Node equivalents.
    if postgres::active_id() == id {
        let mut current = settings.0.lock().unwrap();
        current.active_postgres_version = Some(id);
        if let Err(err) = settings::save(&current) {
            log::warn!("failed to persist the active PostgreSQL version: {err}");
        }
    }

    result.map(|_| postgres::list())
}

/// The versions Rezure can install — a pinned list, so no network involved.
#[tauri::command]
pub fn list_postgres_catalog() -> Vec<PostgresRelease> {
    postgres_catalog::list()
}

#[tauri::command]
pub async fn install_postgres_version(app: AppHandle, version: String) -> Result<(), AppError> {
    postgres_catalog::install(&app, &version).await?;
    connections::ensure_postgres();
    Ok(())
}

fn joined(e: tokio::task::JoinError) -> AppError {
    AppError::DatabaseQueryFailed(format!("background task panicked: {e}"))
}

/// The users on Rezure's own PostgreSQL — see `services::postgres_roles`.
#[tauri::command]
pub async fn list_postgres_roles() -> Result<Vec<PostgresRole>, AppError> {
    tokio::task::spawn_blocking(postgres_roles::list)
        .await
        .map_err(joined)?
}

/// Creates a user (and optionally a database it owns), returning the list
/// as it now stands.
#[tauri::command]
pub async fn create_postgres_role(role: NewRole) -> Result<Vec<PostgresRole>, AppError> {
    tokio::task::spawn_blocking(move || {
        postgres_roles::create(&role)?;
        postgres_roles::list()
    })
    .await
    .map_err(joined)?
}

#[tauri::command]
pub async fn set_postgres_role_password(
    name: String,
    password: Option<String>,
) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || postgres_roles::set_password(&name, password.as_deref()))
        .await
        .map_err(joined)?
}

#[tauri::command]
pub async fn drop_postgres_role(name: String) -> Result<Vec<PostgresRole>, AppError> {
    tokio::task::spawn_blocking(move || {
        postgres_roles::drop(&name)?;
        postgres_roles::list()
    })
    .await
    .map_err(joined)?
}

/// The settings that point an app at `user` — `password` being the one the
/// user just typed, since Rezure doesn't keep it.
#[tauri::command]
pub fn postgres_connection_snippets(
    user: String,
    password: Option<String>,
    database: Option<String>,
) -> Vec<ConnectionSnippet> {
    postgres_roles::snippets(&user, password.as_deref(), database.as_deref())
}
