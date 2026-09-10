//! Thin glue between the Databases page's connection switcher and
//! `services::connections` / `services::database`.
//!
//! Anything that reaches a remote host blocks for as long as the network
//! takes — up to `CONNECT_TIMEOUT_SECS` for a host that never answers — so
//! those commands hop onto a blocking task rather than freezing the UI
//! thread on a server that may not exist.

use crate::config::connections::{NewConnection, SshAuth, SshTunnel, TlsMode};
use crate::services::connections::{self, ConnectionStatus};
use crate::services::database;
use crate::services::db_engine::Engine;
use crate::services::secrets;
use crate::utils::error::AppError;

fn joined(e: tokio::task::JoinError) -> AppError {
    AppError::Database(format!("background task panicked: {e}"))
}

#[tauri::command]
pub fn list_db_connections() -> Vec<ConnectionStatus> {
    connections::list()
}

/// The add-connection form, as one payload — the frontend sends it under a
/// single `request` key, matching `add_db_profile`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRequest {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Absent for a passwordless server; never stored in `connections.json`
    /// either way — see `services::secrets`.
    pub password: Option<String>,
    pub engine: Engine,
    pub tls_mode: Option<TlsMode>,
    pub read_only: Option<bool>,
    pub save_password: Option<bool>,
    /// Present when the database is only reachable through an SSH forward.
    /// `host`/`port` above are then resolved on the SSH server.
    pub ssh: Option<SshTunnel>,
    /// Only sent when the tunnel authenticates with a password. Stored
    /// beside the database password, under its own Credential Manager key.
    pub ssh_password: Option<String>,
}

/// Connects to a server that isn't saved yet and reports its version.
///
/// Runs against the form's current values rather than a stored connection,
/// which is the point: it answers "will this work" before the user commits
/// to it, and every remote failure mode surfaces here first.
#[tauri::command]
pub async fn test_db_connection(request: ConnectionRequest) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || {
        database::probe(database::Probe {
            host: &request.host,
            port: request.port,
            user: &request.user,
            password: request.password.as_deref(),
            engine: request.engine,
            tls: request.tls_mode.unwrap_or_default(),
            ssh: request.ssh.as_ref(),
            ssh_password: request.ssh_password.as_deref(),
        })
    })
    .await
    .map_err(joined)?
}

/// Saves a connection. Writes are refused by default — see
/// `config::connections::Connection::read_only`.
#[tauri::command]
pub fn add_db_connection(request: ConnectionRequest) -> Result<Vec<ConnectionStatus>, AppError> {
    let needs_ssh_password = matches!(
        request.ssh.as_ref().map(|ssh| &ssh.auth),
        Some(SshAuth::Password)
    );
    let ssh_password = request.ssh_password.clone();

    let connection = connections::add(
        NewConnection {
            name: request.name,
            host: request.host,
            port: request.port,
            user: request.user,
            engine: request.engine,
            tls_mode: request.tls_mode.unwrap_or_default(),
            read_only: request.read_only.unwrap_or(true),
            save_password: request.save_password.unwrap_or(false),
            ssh: request.ssh,
        },
        request.password,
    )?;

    // Filed only after the connection exists, because the id it is keyed by
    // is generated there. A Credential Manager failure at this point leaves
    // a saved connection whose tunnel will ask for its password again —
    // recoverable, unlike a password filed under an id nothing refers to.
    if needs_ssh_password {
        if let Some(password) = ssh_password.filter(|p| !p.is_empty()) {
            secrets::save(&secrets::ssh_id(&connection.id), &password)?;
        }
    }
    Ok(connections::list())
}

#[tauri::command]
pub fn remove_db_connection(id: String) -> Result<Vec<ConnectionStatus>, AppError> {
    connections::remove(&id)?;
    Ok(connections::list())
}

/// Supplies (or corrects) the password for a saved connection — the
/// "unlock" path for one whose password was never persisted.
#[tauri::command]
pub fn set_db_connection_password(
    id: String,
    password: String,
    save: bool,
) -> Result<Vec<ConnectionStatus>, AppError> {
    connections::set_password(&id, &password, save)?;
    Ok(connections::list())
}

/// What selecting a target left the page looking at.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetResult {
    pub connections: Vec<ConnectionStatus>,
    /// The name of what's now being read, for the switcher's notice.
    pub label: String,
    pub remote: bool,
}

/// Points the Databases page at a remote connection.
///
/// Unlike a local profile switch there is nothing to stop, start or roll
/// back: the local server keeps running on whatever datadir it was already
/// serving, so this can't fail halfway and leave the user with no database.
#[tauri::command]
pub fn use_db_connection(id: String) -> Result<TargetResult, AppError> {
    let connection = connections::set_active(&id)?;
    Ok(TargetResult {
        connections: connections::list(),
        label: connection.name,
        remote: true,
    })
}

/// Points the Databases page back at the local server.
#[tauri::command]
pub fn use_local_db_profile() -> TargetResult {
    connections::clear_active();
    TargetResult {
        connections: connections::list(),
        label: crate::services::db_profiles::active()
            .map(|profile| profile.name)
            .unwrap_or_else(|| "the local server".to_string()),
        remote: false,
    }
}
