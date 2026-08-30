//! Thin glue between the Databases page and `services::database` /
//! `services::db_clients`.
//!
//! Everything that touches MariaDB is blocking (it shells out to the
//! bundled client and waits), so the longer-running ones hop onto a
//! blocking task rather than stalling the UI thread while a dump runs.

use std::path::PathBuf;

use crate::services::database::{self, DatabaseInfo, ServerInfo};
use crate::services::db_clients::{self, DbClientInfo};
use crate::utils::error::AppError;

fn joined(e: tokio::task::JoinError) -> AppError {
    AppError::DatabaseQueryFailed(format!("background task panicked: {e}"))
}

#[tauri::command]
pub async fn list_databases() -> Result<Vec<DatabaseInfo>, AppError> {
    tokio::task::spawn_blocking(database::list_databases)
        .await
        .map_err(joined)?
}

#[tauri::command]
pub fn database_server_info() -> ServerInfo {
    database::server_info()
}

#[tauri::command]
pub async fn list_collations() -> Result<Vec<String>, AppError> {
    tokio::task::spawn_blocking(database::list_collations)
        .await
        .map_err(joined)?
}

#[tauri::command]
pub async fn create_database(name: String, collation: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || database::create_database(&name, &collation))
        .await
        .map_err(joined)?
}

#[tauri::command]
pub async fn drop_database(name: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || database::drop_database(&name))
        .await
        .map_err(joined)?
}

/// Dumps a database and returns the path of the `.sql` it wrote, so the UI
/// can tell the user where it landed instead of just claiming success.
#[tauri::command]
pub async fn export_database(name: String) -> Result<String, AppError> {
    let dump = tokio::task::spawn_blocking(move || database::export_database(&name))
        .await
        .map_err(joined)??;
    Ok(dump.display().to_string())
}

#[tauri::command]
pub async fn import_sql(name: String, file: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || database::import_sql(&name, &PathBuf::from(file)))
        .await
        .map_err(joined)?
}

/// Reveals the exports folder in Explorer.
#[tauri::command]
pub fn open_dumps_folder() -> Result<(), AppError> {
    let dir = database::dumps_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    tauri_plugin_opener::open_path(dir.display().to_string(), None::<&str>).map_err(|e| {
        AppError::OpenFailed {
            target: "the exports folder".to_string(),
            reason: e.to_string(),
        }
    })
}

/// The SQL clients installed on this machine — what the "Open" menu offers.
#[tauri::command]
pub fn list_db_clients() -> Vec<DbClientInfo> {
    db_clients::detect()
}

#[tauri::command]
pub fn open_in_db_client(client: String, database: String) -> Result<(), AppError> {
    db_clients::open(&client, &database)
}
