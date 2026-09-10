//! The live state behind remote connections: which ones exist, which one
//! the Databases page is reading, and which client binary can speak to it.
//!
//! Mirrors `services::db_profiles`' shape — a process-wide
//! `OnceLock<Mutex<_>>` persisted on every mutation — so the two halves of
//! the switcher behave the same way.
//!
//! The one thing this module does *not* do is touch the local server.
//! Selecting a remote connection leaves `mysqld` running on whatever
//! profile it was already serving: there is nothing to stop, nothing to
//! roll back, and switching back is instant. That asymmetry with
//! `db_profiles::set_active` is deliberate, not an oversight.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use serde::Serialize;

use super::db_engine::{self, Engine, SERVER_EXE};
use super::db_profiles;
use super::secrets;
use super::tunnel;
use crate::config::connections::{self, Connection, ConnectionStore, NewConnection};
use crate::utils::error::AppError;

fn store_cell() -> &'static Mutex<ConnectionStore> {
    static STORE: OnceLock<Mutex<ConnectionStore>> = OnceLock::new();
    STORE.get_or_init(|| {
        Mutex::new(connections::load().unwrap_or_else(|err| {
            // A store that won't parse is reported once and then treated as
            // empty *in memory only* — nothing here overwrites the file, so
            // the user still has it to fix.
            log::error!("could not read connections.json: {err}");
            ConnectionStore::default()
        }))
    })
}

/// A poisoned lock means a previous caller panicked mid-mutation. The store
/// is a plain data structure with no invariant spanning two fields, so the
/// contents are still coherent — recovering beats taking the app down.
fn store() -> MutexGuard<'static, ConnectionStore> {
    store_cell().lock().unwrap_or_else(|e| e.into_inner())
}

fn persist(store: &ConnectionStore) {
    if let Err(err) = connections::save(store) {
        log::warn!("could not persist connections: {err}");
    }
}

/// The remote connection currently being read, or `None` when the Databases
/// page is looking at the local server.
pub fn active() -> Option<Connection> {
    store().active().cloned()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    #[serde(flatten)]
    pub connection: Connection,
    pub active: bool,
    /// Whether a client binary that can talk to this server is installed.
    /// Resolved per row so the switcher can grey one out and say why,
    /// rather than letting the selection fail later.
    pub client_available: bool,
    /// Whether a password is known — saved in Credential Manager, or
    /// entered earlier this session. Drives the "unlock" prompt.
    pub has_password: bool,
}

pub fn list() -> Vec<ConnectionStatus> {
    let store = store();
    let active_id = store.active_id.clone();
    store
        .connections
        .iter()
        .map(|connection| ConnectionStatus {
            active: Some(&connection.id) == active_id.as_ref(),
            client_available: client_dir(connection.engine).is_ok(),
            has_password: secrets::resolve(&connection.id).is_some(),
            connection: connection.clone(),
        })
        .collect()
}

/// A client binary that can be pointed at a remote server.
///
/// Carries the engine and version it came from, not just the folder,
/// because the flags differ between them: TLS is `--ssl-mode` on MySQL and
/// `--ssl` on MariaDB, and two of `mysqldump`'s remote-safety options exist
/// only on MySQL 8. Guessing either produces an "unknown option" failure
/// with nothing pointing at the cause.
#[derive(Debug, Clone)]
pub struct ClientBuild {
    pub dir: PathBuf,
    pub engine: Engine,
    pub version: String,
}

impl ClientBuild {
    /// Whether this is a MySQL 8-or-newer client, which is where
    /// `--skip-column-statistics` and `--set-gtid-purged` appear.
    pub fn is_mysql_8_or_newer(&self) -> bool {
        self.engine == Engine::MySql
            && self
                .version
                .split('.')
                .next()
                .and_then(|major| major.parse::<u32>().ok())
                .is_some_and(|major| major >= 8)
    }
}

/// The client binaries to use for a server running `engine`.
///
/// Prefers a build of the same engine, then falls back to the other one.
/// The fallback exists because a MariaDB client speaks to a MySQL server
/// perfectly well for `mysql_native_password` accounts, which is most of
/// them — refusing outright would block a working setup on a machine that
/// only ever installed one engine. Where it *doesn't* work
/// (`caching_sha2_password`) the client says so itself, and that message is
/// more useful than a guess made here.
pub fn client_dir(engine: Engine) -> Result<ClientBuild, AppError> {
    let other = match engine {
        Engine::MySql => Engine::MariaDb,
        Engine::MariaDb => Engine::MySql,
    };

    for candidate in [engine, other] {
        for runtime in db_profiles::installed_binaries(candidate) {
            let Some(dir) = runtime.exe.parent() else {
                continue;
            };
            if dir.join(db_engine::CLIENT_EXE).is_file() {
                return Ok(ClientBuild {
                    dir: dir.to_path_buf(),
                    engine: candidate,
                    version: runtime.version,
                });
            }
        }
    }

    Err(AppError::EngineBinaryMissing {
        engine: engine.label().to_string(),
        version: format!("any version providing {SERVER_EXE}"),
    })
}

/// Registers a connection and, when asked, files its password.
///
/// The password is saved *before* the connection is persisted so a
/// Credential Manager failure can't leave a saved connection whose password
/// silently went nowhere.
pub fn add(request: NewConnection, password: Option<String>) -> Result<Connection, AppError> {
    let NewConnection {
        name,
        host,
        port,
        user,
        engine,
        tls_mode,
        read_only,
        save_password,
        ssh,
    } = request;

    let name = name.trim().to_string();
    let host = host.trim().to_string();
    let user = user.trim().to_string();
    if name.is_empty() || host.is_empty() || user.is_empty() {
        return Err(AppError::InvalidConnection(
            "a connection needs a name, a host and a user".to_string(),
        ));
    }

    let mut store = store();
    let ssh_host = ssh.as_ref().map(|s| s.host.clone());
    if let Some(existing) = store.endpoint_taken_by(&host, port, &user, ssh_host.as_deref(), None) {
        return Err(AppError::ConnectionAlreadyExists {
            endpoint: format!("{user}@{host}:{port}"),
            name: existing.name.clone(),
        });
    }

    let id = uuid::Uuid::new_v4().to_string();
    match (&password, save_password) {
        (Some(password), true) => secrets::save(&id, password)?,
        (Some(password), false) => secrets::remember_for_session(&id, password),
        (None, _) => {}
    }

    let connection = Connection {
        id,
        name,
        host,
        port,
        user,
        engine,
        tls_mode,
        read_only,
        save_password: save_password && password.is_some(),
        ssh,
        last_used_at: None,
    };
    store.connections.push(connection.clone());
    persist(&store);
    Ok(connection)
}

/// Replaces the password for an existing connection — the "unlock" path for
/// one saved without its password, and the way to correct a wrong one.
pub fn set_password(id: &str, password: &str, save: bool) -> Result<(), AppError> {
    let mut store = store();
    let index = store
        .connections
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| AppError::ConnectionNotFound(id.to_string()))?;

    if save {
        secrets::save(id, password)?;
    } else {
        // Drop any previously saved copy: the user has just said this
        // password shouldn't be persisted, and leaving a stale one in
        // Credential Manager would keep being picked up ahead of it.
        secrets::forget(id);
        secrets::remember_for_session(id, password);
    }
    store.connections[index].save_password = save;
    persist(&store);
    Ok(())
}

pub fn remove(id: &str) -> Result<(), AppError> {
    let mut store = store();
    let index = store
        .connections
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| AppError::ConnectionNotFound(id.to_string()))?;

    store.connections.remove(index);
    // Selecting nothing means the local profile, which is always there —
    // so removing the active connection needs no replacement chosen.
    if store.active_id.as_deref() == Some(id) {
        store.active_id = None;
    }
    persist(&store);
    drop(store);

    // Both outlive the store entry otherwise: a credential nothing can ever
    // use again, and an `ssh.exe` still holding a forwarded port.
    secrets::forget(id);
    tunnel::close(id);
    Ok(())
}

/// Points the Databases page at a remote connection.
pub fn set_active(id: &str) -> Result<Connection, AppError> {
    let mut store = store();
    let index = store
        .connections
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| AppError::ConnectionNotFound(id.to_string()))?;

    store.connections[index].last_used_at = Some(connections::now_secs());
    store.active_id = Some(id.to_string());
    let connection = store.connections[index].clone();
    persist(&store);
    Ok(connection)
}

/// Points the Databases page back at the local server.
///
/// Called on its own, and also by a local profile switch — choosing a
/// datadir is the user saying they want to look at local data, and leaving
/// a remote connection selected would show them a list from another server
/// while the profile switcher underneath claims otherwise.
pub fn clear_active() {
    let mut store = store();
    if store.active_id.is_some() {
        store.active_id = None;
        persist(&store);
    }
}
