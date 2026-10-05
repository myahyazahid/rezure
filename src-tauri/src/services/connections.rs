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
use super::mssql_localdb;
use super::odbc;
use super::postgres;
use super::secrets;
use super::tunnel;
use crate::config::connections::{
    self, Connection, ConnectionStore, NewConnection, ServerKind, TlsMode,
};
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
    /// Whether a client that can talk to this server is installed — a MySQL
    /// or MariaDB build for a MySQL-family server, the ODBC Driver (and, for
    /// Rezure's own instance, LocalDB itself) for SQL Server. Resolved per
    /// row so the switcher can grey one out and say why, rather than letting
    /// the selection fail later.
    pub client_available: bool,
    /// Whether a password is known — saved in Credential Manager, or
    /// entered earlier this session — or none is needed at all (Windows
    /// Authentication). Drives the "unlock" prompt.
    pub has_password: bool,
}

/// Whether `connection` signs in without a password Rezure has to supply:
/// Windows Authentication, or Rezure's own PostgreSQL, which trusts every
/// local connection.
pub fn needs_no_password(connection: &Connection) -> bool {
    match connection.kind {
        ServerKind::Sqlserver => connection.windows_auth,
        ServerKind::Postgres => connection.managed,
        ServerKind::Mysql => false,
    }
}

/// The `host` of every saved SQL Server connection that is reached directly.
///
/// For `mssql_discovery`, to stop offering an instance that's already there.
/// Tunnelled connections are left out: their host is the address *on the SSH
/// server* (nearly always `127.0.0.1`), which says nothing about this machine.
pub fn saved_sqlserver_hosts() -> Vec<String> {
    store()
        .connections
        .iter()
        .filter(|c| c.kind == ServerKind::Sqlserver && c.ssh.is_none())
        .map(|c| c.host.clone())
        .collect()
}

pub fn list() -> Vec<ConnectionStatus> {
    let store = store();
    let active_id = store.active_id.clone();
    // Asked once for the whole list: each answer is a call into the ODBC
    // driver manager or a folder scan, and they can't differ between rows.
    let odbc_installed = odbc::is_installed();
    let localdb_installed = mssql_localdb::is_installed();
    let psql_installed = postgres::client_bin_dir().is_ok();
    store
        .connections
        .iter()
        .map(|connection| ConnectionStatus {
            active: Some(&connection.id) == active_id.as_ref(),
            client_available: match connection.kind {
                ServerKind::Mysql => client_dir(connection.engine).is_ok(),
                ServerKind::Sqlserver => {
                    odbc_installed && (!connection.managed || localdb_installed)
                }
                ServerKind::Postgres => psql_installed,
            },
            has_password: needs_no_password(connection)
                || secrets::resolve(&connection.id).is_some(),
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
/// perfectly well — `mysql_native_password` natively, and MySQL 8's
/// `caching_sha2_password` through the plugin MariaDB ships beside it
/// (`Conn::plugin_dir` points the client at it). Refusing outright would
/// block a working setup on a machine that only ever installed one engine.
/// Where it still doesn't work the client says so itself, and that message
/// is more useful than a guess made here.
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
        kind,
        engine,
        windows_auth,
        trust_server_certificate,
        tls_mode,
        read_only,
        save_password,
        ssh,
    } = request;

    // Windows Authentication is a SQL Server idea; on a MySQL connection the
    // flag would only hide the user box for no reason.
    let windows_auth = windows_auth && kind == ServerKind::Sqlserver;
    let name = name.trim().to_string();
    let host = host.trim().to_string();
    let user = if windows_auth {
        String::new()
    } else {
        user.trim().to_string()
    };
    if name.is_empty() || host.is_empty() || (user.is_empty() && !windows_auth) {
        return Err(AppError::InvalidConnection(
            "a connection needs a name, a host and a user".to_string(),
        ));
    }
    // A named SQL Server instance (`host\SQLEXPRESS`) is found through SQL
    // Server Browser, so its port may be left empty. Everything else needs
    // one.
    if port == 0 && kind != ServerKind::Sqlserver {
        return Err(AppError::InvalidConnection(
            "a connection needs a port".to_string(),
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
    // A Windows-authenticated connection has no password to keep, and
    // filing one anyway would make it look like it does.
    let password = password.filter(|_| !windows_auth);
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
        kind,
        engine,
        windows_auth,
        trust_server_certificate,
        managed: false,
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
    if store.connections[index].managed {
        let owner = match store.connections[index].kind {
            ServerKind::Postgres => "PostgreSQL server",
            _ => "LocalDB instance",
        };
        return Err(AppError::InvalidConnection(format!(
            "\"{}\" is Rezure's own {owner} — it's listed for as long as that's installed",
            store.connections[index].name
        )));
    }

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

/// Adds the connection to Rezure's own LocalDB instance, once LocalDB is
/// installed and if it isn't there already.
///
/// A connection rather than a local profile: a profile is a datadir one
/// `mysqld` is pointed at, and LocalDB has no such thing. Seeded rather than
/// left to the user, because there is exactly one right way to reach it —
/// `(localdb)\Rezure`, Windows Authentication — and nothing to ask.
///
/// Called at startup and right after LocalDB is installed. Returns whether
/// it added one. Also creates the instance itself if it's missing — see
/// `mssql_localdb::ensure_created` for why that can't wait for Start.
pub fn ensure_localdb() -> bool {
    if !mssql_localdb::is_installed() {
        return false;
    }
    if let Err(err) = mssql_localdb::ensure_created() {
        log::warn!("could not create the LocalDB instance: {err}");
    }
    let mut store = store();
    if store
        .connections
        .iter()
        .any(|c| c.managed && c.kind == ServerKind::Sqlserver)
    {
        return false;
    }
    store.connections.push(Connection {
        id: uuid::Uuid::new_v4().to_string(),
        name: mssql_localdb::SERVICE_NAME.to_string(),
        host: mssql_localdb::SERVER_ADDRESS.to_string(),
        port: 0,
        user: String::new(),
        kind: ServerKind::Sqlserver,
        // Unused for SQL Server; any value reads back the same.
        engine: Engine::MariaDb,
        windows_auth: true,
        // Not needed: ODBC Driver 18 reaches LocalDB over its named pipe
        // with its default encryption and no certificate complaint (checked
        // by `mssql::tests::print_localdb_encryption_behaviour`).
        trust_server_certificate: false,
        managed: true,
        tls_mode: TlsMode::Preferred,
        read_only: false,
        save_password: false,
        ssh: None,
        last_used_at: None,
    });
    persist(&store);
    log::info!("added a connection for Rezure's LocalDB instance");
    true
}

/// Adds the connection to Rezure's own PostgreSQL service, once a version is
/// installed and if it isn't there already — the same reasoning as
/// [`ensure_localdb`]: there's one right way to reach it (`127.0.0.1:5432`,
/// `postgres`, no password) and nothing to ask.
///
/// Called at startup and right after a PostgreSQL install. Returns whether it
/// added one.
pub fn ensure_postgres() -> bool {
    if !postgres::is_installed() {
        return false;
    }
    let mut store = store();
    if store
        .connections
        .iter()
        .any(|c| c.managed && c.kind == ServerKind::Postgres)
    {
        return false;
    }
    store.connections.push(Connection {
        id: uuid::Uuid::new_v4().to_string(),
        name: postgres::SERVICE_NAME.to_string(),
        host: postgres::HOST.to_string(),
        port: postgres::PORT,
        user: postgres::SUPERUSER.to_string(),
        kind: ServerKind::Postgres,
        // Unused for PostgreSQL; any value reads back the same.
        engine: Engine::MariaDb,
        windows_auth: false,
        trust_server_certificate: false,
        managed: true,
        // The local server is bootstrapped without TLS; asking for it would
        // only cost a round trip before falling back.
        tls_mode: TlsMode::Disabled,
        read_only: false,
        save_password: false,
        ssh: None,
        last_used_at: None,
    });
    persist(&store);
    log::info!("added a connection for Rezure's PostgreSQL service");
    true
}
