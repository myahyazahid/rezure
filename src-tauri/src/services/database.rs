//! Reads and manages the schemas on whichever database Rezure is currently
//! pointed at — the local server (see `services::db_profiles`) or a remote
//! connection (see `services::connections`).
//!
//! There's no MySQL driver crate in the dependency tree on purpose: every
//! server build Rezure runs ships its own client binaries (`mysql.exe`,
//! `mysqldump.exe`) next to it, so this module drives those instead of
//! adding a second, redundant way to speak the protocol. `--batch` makes
//! the client emit tab-separated rows with no box drawing, which is what
//! every read here parses.
//!
//! # One resolved connection, not a set of globals
//!
//! Everything below goes through [`Conn`], resolved once per operation by
//! [`active_conn`]. Host, port, user, credentials, TLS and *which client
//! binary to run* all come from there, because all six differ between a
//! local profile and a remote server — and a function that reads only some
//! of them from the active target is how a query ends up asking the right
//! server with the wrong client, or the wrong server entirely.
//!
//! Rezure's own datadir is bootstrapped without a root password — it binds
//! to 127.0.0.1 only, and asking a developer to invent a password for a
//! throwaway local server just moves the secret into a config file. A
//! remote connection always carries one; it is passed through a temporary
//! `--defaults-file` and never on the command line, where every process
//! listing on the machine could read it.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::connections;
use super::db_engine::{self, Engine};
use super::db_profiles;
use super::projects::scan_projects;
use super::secrets;
use super::tunnel;
use crate::config::connections::{Connection, SshTunnel, TlsMode};
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;
use crate::utils::paths;

pub const HOST: &str = "127.0.0.1";
pub const USER: &str = "root";
/// Used only when no profile is resolvable — the real port comes from
/// whichever profile is active.
pub const DEFAULT_PORT: u16 = 3306;

/// How long a client waits for a TCP connect before giving up.
///
/// The client's own default is minutes long. On localhost that never
/// mattered, because a refused connection fails instantly; a remote host
/// that is simply unreachable would otherwise hang the Databases page with
/// no way to tell whether anything is happening.
const CONNECT_TIMEOUT_SECS: u32 = 10;

/// The port the currently active profile's server listens on.
///
/// A function rather than a constant because the profile switcher lets each
/// profile carry its own port: reading a fixed 3306 here would point every
/// query on the Databases page at whatever happened to own that port,
/// which after a switch may not be Rezure's server at all.
pub fn port() -> u16 {
    db_profiles::active()
        .map(|profile| profile.port)
        .unwrap_or(DEFAULT_PORT)
}

/// Where a SQL client should be pointed to reach what Rezure is showing:
/// host, port and user of the active target.
///
/// Used by `db_clients` for the "Open" menu, so handing off to DBeaver or
/// TablePlus lands on the same server the page is listing rather than
/// always on localhost.
pub fn endpoint() -> (String, u16, String) {
    match connections::active() {
        // A tunnelled connection's own host and port only mean anything on
        // the SSH server. What a client on *this* machine can dial is the
        // forwarded local port — and only while the tunnel is up, which is
        // why this reports the live one rather than starting anything.
        Some(connection) => match tunnel::existing_port(&connection.id) {
            Some(local) => (HOST.to_string(), local, connection.user),
            None => (connection.host, connection.port, connection.user),
        },
        None => (HOST.to_string(), port(), USER.to_string()),
    }
}

/// Whether a console opened on the active target has to ask for a password.
///
/// Rezure's own server has none, so prompting there would be a step with no
/// answer. A remote one almost always does, and a console that connects
/// without offering to supply it just fails with "access denied".
pub fn endpoint_prompts_for_password() -> bool {
    connections::active()
        .map(|connection| secrets::resolve(&connection.id).is_some())
        .unwrap_or(false)
}

/// Schemas MariaDB itself owns — never listed, never droppable.
const SYSTEM_SCHEMAS: [&str; 4] = ["mysql", "information_schema", "performance_schema", "sys"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub name: String,
    pub collation: String,
    pub table_count: u64,
    /// Data + index bytes, as `information_schema` reports them. Zero for
    /// an empty schema, and approximate for InnoDB by nature — it's a
    /// storage estimate, not a byte count.
    pub size_bytes: u64,
    /// The project domain this database appears to belong to, matched by
    /// name — see [`used_by`]. Always `None` for a remote server, where
    /// local project folders say nothing about the schemas on it.
    pub used_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub has_password: bool,
    /// Ready to paste into a client that takes a connection string. Never
    /// carries the password, even for a remote connection that has one.
    pub dsn: String,
    /// Which target this is — a remote connection, or the local server.
    pub remote: bool,
    /// The name of the active connection or profile, for the UI to state
    /// beside the endpoint. Empty when no profile has been set up yet.
    pub label: String,
    /// Whether writes (create, drop, import) are refused for this target.
    pub read_only: bool,
}

pub fn server_info() -> ServerInfo {
    match connections::active() {
        Some(connection) => {
            let has_password = secrets::resolve(&connection.id).is_some();
            ServerInfo {
                dsn: format!(
                    "mysql://{}@{}:{}",
                    connection.user, connection.host, connection.port
                ),
                host: connection.host,
                port: connection.port,
                user: connection.user,
                has_password,
                remote: true,
                label: connection.name,
                read_only: connection.read_only,
            }
        }
        None => ServerInfo {
            host: HOST.to_string(),
            port: port(),
            user: USER.to_string(),
            has_password: false,
            dsn: format!("mysql://{USER}@{HOST}:{}", port()),
            remote: false,
            label: db_profiles::active()
                .map(|profile| profile.name)
                .unwrap_or_default(),
            read_only: false,
        },
    }
}

/// A temporary option file carrying a password, deleted when dropped.
///
/// The password cannot go on the command line: `-p<secret>` is visible in
/// Task Manager, `tasklist`, and any process-listing API, to every process
/// running as this user. An option file is what both clients document for
/// exactly this, and Windows' per-user temp directory is already ACL'd to
/// the user — so the file is readable by the same set of processes that
/// could read Credential Manager anyway, and only for as long as the
/// command runs.
struct TempDefaults {
    path: PathBuf,
}

impl TempDefaults {
    fn new(password: &str) -> Result<Self, AppError> {
        let path = std::env::temp_dir().join(format!(
            "rezure-{}-{}.cnf",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        // `[client]` is read by `mysql` and `mysqldump` alike, so one
        // section covers every binary this module runs.
        let contents = format!("[client]\npassword=\"{}\"\n", escape_option_value(password));
        std::fs::write(&path, contents).map_err(|e| {
            AppError::Io(format!(
                "could not write the temporary credentials file: {e}"
            ))
        })?;
        Ok(Self { path })
    }
}

impl Drop for TempDefaults {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Escapes a value for a double-quoted option-file entry.
///
/// Option files process backslash escapes inside quotes, so a password
/// containing `\` or `"` would otherwise be read as something other than
/// what the user typed — and fail authentication with no hint as to why.
fn escape_option_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Everything needed to run one client command against the active target.
pub struct Conn {
    host: String,
    port: u16,
    user: String,
    tls: TlsMode,
    /// The engine of the *client binary*, which is not always the engine of
    /// the server — see `connections::client_dir`.
    client_engine: Engine,
    /// True when this client is MySQL 8+, whose `mysqldump` takes two
    /// options no other build has.
    mysql_8_client: bool,
    bin: PathBuf,
    /// Held for the lifetime of the connection: dropping it deletes the
    /// file, so the password exists on disk only while a command runs.
    defaults: Option<TempDefaults>,
    /// The remote connection this points at, or `None` for the local
    /// server.
    remote: Option<Connection>,
}

impl Conn {
    /// The leading arguments every client invocation starts with.
    ///
    /// `--defaults-file` must come first — both clients refuse it anywhere
    /// else — so this is deliberately returned as one ordered block that
    /// callers append their own arguments to, never interleave with.
    fn args(&self) -> Vec<String> {
        self.base_args(true)
    }

    /// The same block for `mysqldump`, which is the one client here that
    /// doesn't register `--connect-timeout`: MariaDB's build rejects it
    /// outright (`unknown variable 'connect-timeout=10'`), which failed
    /// every export rather than only slow ones. A dump has no use for a
    /// short connect deadline anyway — a tunnelled connection has already
    /// been proven reachable by `tunnel::open`, and a direct one falls back
    /// to the OS's own TCP timeout.
    fn dump_args(&self) -> Vec<String> {
        self.base_args(false)
    }

    fn base_args(&self, connect_timeout: bool) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(defaults) = &self.defaults {
            args.push(format!("--defaults-file={}", defaults.path.display()));
        } else {
            // Without this the client silently reads `my.cnf` from the
            // machine's default locations, which for a user who also runs
            // XAMPP or Laragon can mean connecting somewhere else entirely.
            args.push("--no-defaults".to_string());
        }
        args.push("-h".to_string());
        args.push(self.host.clone());
        args.push("-P".to_string());
        args.push(self.port.to_string());
        args.push("-u".to_string());
        args.push(self.user.clone());
        if connect_timeout {
            args.push(format!("--connect-timeout={CONNECT_TIMEOUT_SECS}"));
        }
        args.extend(self.tls_args());
        args
    }

    /// TLS spelled the way this client understands it. MySQL took
    /// `--ssl-mode` in 5.7; MariaDB never adopted it and uses `--ssl` /
    /// `--skip-ssl`. Passing the wrong one aborts with "unknown option".
    fn tls_args(&self) -> Vec<String> {
        match (self.tls, self.client_engine) {
            // The client's own default already negotiates TLS when the
            // server offers it, so there's nothing to say.
            (TlsMode::Preferred, _) => Vec::new(),
            (TlsMode::Required, Engine::MySql) => vec!["--ssl-mode=REQUIRED".to_string()],
            (TlsMode::Required, Engine::MariaDb) => vec!["--ssl".to_string()],
            (TlsMode::Disabled, Engine::MySql) => vec!["--ssl-mode=DISABLED".to_string()],
            (TlsMode::Disabled, Engine::MariaDb) => vec!["--skip-ssl".to_string()],
        }
    }

    /// One of the client binaries that ships alongside the server. The
    /// names are engine-neutral — see `services::db_engine`.
    fn client(&self, name: &str) -> Result<PathBuf, AppError> {
        let exe = self.bin.join(name);
        if exe.is_file() {
            Ok(exe)
        } else {
            Err(AppError::BinaryNotInstalled(format!("the server's {name}")))
        }
    }

    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// Refuses a write against a target marked read-only, naming it, before
    /// anything reaches the server.
    fn check_writable(&self) -> Result<(), AppError> {
        match &self.remote {
            Some(connection) if connection.read_only => Err(AppError::ConnectionReadOnly {
                name: connection.name.clone(),
            }),
            _ => Ok(()),
        }
    }
}

/// The connection every operation below runs through.
///
/// A remote connection wins when one is selected; otherwise this is the
/// local server, exactly as before remote connections existed.
fn active_conn() -> Result<Conn, AppError> {
    match connections::active() {
        Some(connection) => remote_conn(connection),
        None => local_conn(),
    }
}

/// The local server, whose client binaries come from the build actually
/// serving right now.
///
/// Resolved through the active profile rather than the pinned MariaDB
/// manifest entry, so that after a switch to a MySQL profile the client
/// binaries used to query it come from that same MySQL build — mixing a
/// MariaDB client with a MySQL server is a source of confusing protocol
/// and authentication errors.
fn local_conn() -> Result<Conn, AppError> {
    let profile = db_profiles::active()
        .ok_or_else(|| AppError::BinaryNotInstalled("Database".to_string()))?;
    let exe = db_profiles::resolve_server_exe(&profile)?;
    let bin = exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Io("the database server has no parent directory".to_string()))?;

    Ok(Conn {
        host: HOST.to_string(),
        port: profile.port,
        user: USER.to_string(),
        // The local server is loopback-only and has no certificate; asking
        // for TLS here would fail a connection that is already private.
        tls: TlsMode::Preferred,
        client_engine: profile.engine,
        mysql_8_client: false,
        bin,
        defaults: None,
        remote: None,
    })
}

fn remote_conn(connection: Connection) -> Result<Conn, AppError> {
    let build = connections::client_dir(connection.engine)?;

    // With a tunnel the client talks to a local port and knows nothing
    // about SSH; without one it dials the host directly. Everything below
    // this point is identical either way.
    let (host, port) = match &connection.ssh {
        Some(_) => (HOST.to_string(), tunnel::ensure(&connection)?),
        None => (connection.host.clone(), connection.port),
    };
    let defaults = match secrets::resolve(&connection.id) {
        Some(password) => Some(TempDefaults::new(&password)?),
        // Not an error: a server may genuinely have no password, and one
        // that does will say so itself in a message worth showing.
        None => None,
    };

    Ok(Conn {
        host,
        port,
        user: connection.user.clone(),
        tls: connection.tls_mode,
        client_engine: build.engine,
        mysql_8_client: build.is_mysql_8_or_newer(),
        bin: build.dir,
        defaults,
        remote: Some(connection),
    })
}

/// Refuses anything that isn't a plain identifier.
///
/// A database name can't be bound as a parameter — it has to be
/// interpolated into the statement, which is the classic injection
/// thing — so the only defence is to refuse anything that isn't a plain
/// identifier in the first place. Deliberately stricter than MySQL's own
/// rules (which allow almost anything inside backticks): nothing a local
/// dev database legitimately needs is excluded, and no quoting or escaping
/// question can arise downstream.
fn validate_identifier(name: &str, kind: &str) -> Result<(), AppError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidDatabaseName {
            name: name.to_string(),
            kind: kind.to_string(),
        })
    }
}

/// The interactive console client for the active target, for handing a
/// developer a shell that's already connected — see
/// `db_clients::open_console`.
pub fn console_client() -> Result<PathBuf, AppError> {
    active_conn()?.client(db_engine::CLIENT_EXE)
}

/// Turns a failed client run into an error carrying the server's own
/// message — `ERROR 1049 (42000): Unknown database 'x'` is far more useful
/// to show than "command failed with exit code 1".
///
/// The one case that gets rewritten rather than passed through is a failed
/// connect: see [`connect_failure`].
fn client_error(conn: &Conn, stderr: &[u8], fallback: &str) -> AppError {
    let message = String::from_utf8_lossy(stderr);
    let detail = message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(fallback)
        .trim();

    if let Some(rewritten) = connect_failure(conn, detail) {
        return rewritten;
    }
    // Everything else keeps the server's own wording — it is the accurate
    // part — with a hint appended where that wording is known to send
    // people the wrong way.
    match hint_for(detail) {
        Some(hint) => AppError::DatabaseQueryFailed(format!("{detail} — {hint}")),
        None => AppError::DatabaseQueryFailed(detail.to_string()),
    }
}

/// Server errors whose own message is correct but hides what to actually do.
///
/// Both of these are dead ends people lose an evening to, and neither is
/// guessable from the text the server sends:
///
/// * **1698** — the account authenticates by *operating-system user*
///   (`auth_socket` on MySQL, `unix_socket` on MariaDB), which is the
///   default for `root@localhost` on every Debian and Ubuntu package. It
///   cannot be used over a password connection at all, no matter how right
///   the password is. Reading "Access denied" sends people off to reset a
///   password that was never going to be consulted.
/// * **caching_sha2_password** — MySQL 8's default plugin, which a MariaDB
///   client cannot speak. Rezure falls back to a MariaDB client when no
///   MySQL build is installed (see `connections::client_dir`), so this is
///   reachable with a perfectly correct username and password.
/// * **`utf8mb4_0900_*`** — MySQL 8's own default collation family, which
///   MariaDB has never implemented (it stopped at the `unicode_520` set).
///   `mysqldump` writes the source server's default collation into every
///   `CREATE TABLE`, so a dump taken from a MySQL 8 server — including one
///   of Rezure's own remote connections — fails on line one of the import
///   if the target is MariaDB, with no correct username, password or grant
///   that would have prevented it.
fn hint_for(detail: &str) -> Option<&'static str> {
    let lower = detail.to_lowercase();
    if detail.contains("1698") {
        return Some(
            "that account signs in by operating-system user (auth_socket / unix_socket), not by \
             password, so no password will ever be accepted for it. Create a database user that \
             has its own password and connect as that one instead",
        );
    }
    if lower.contains("caching_sha2_password") {
        return Some(
            "this account uses MySQL 8's authentication plugin, which the MariaDB client can't \
             speak. Install a MySQL build under C:\\rezure\\custom\\mysql\\ so Rezure has a \
             matching client, or give the account mysql_native_password on the server",
        );
    }
    if lower.contains("unknown collation") && lower.contains("0900") {
        return Some(
            "this dump came from a MySQL 8 server, and utf8mb4_0900_* is a MySQL-8-only \
             collation family MariaDB has never implemented. Import it against a MySQL build \
             instead (add one under C:\\rezure\\custom\\mysql\\), or open the .sql file and \
             replace utf8mb4_0900_ai_ci (and any other utf8mb4_0900_* entries) with \
             utf8mb4_unicode_ci before importing",
        );
    }
    None
}

/// Rewrites "couldn't connect" into which host, and which of the two very
/// different causes it was.
///
/// The client reports these as `ERROR 2002 (HY000): Can't connect to server
/// on 'host' (138)`, where the number is a C `errno` — not a MySQL code and
/// not a Winsock one, so nothing a user searches for finds it. The
/// distinction it hides is the whole diagnosis:
///
/// * **timed out** — the packets went nowhere. A firewall dropping them, or
///   a server bound to `127.0.0.1` so nothing outside the box can reach it.
/// * **refused** — the host answered "nothing is listening here", which
///   means the port is right but the server is down, or the port is wrong.
///
/// Telling someone to check their firewall when the server simply isn't
/// running wastes an afternoon, so the two are never merged.
fn connect_failure(conn: &Conn, detail: &str) -> Option<AppError> {
    let lower = detail.to_lowercase();
    if !lower.contains("can't connect") && !lower.contains("cannot connect") {
        return None;
    }

    // 138 is MSVC's `ETIMEDOUT`; 10060 is the Winsock spelling of the same
    // thing, which some builds report instead.
    let timed_out =
        detail.contains("(138)") || detail.contains("(10060)") || lower.contains("timed out");
    // 10061 is Winsock's ECONNREFUSED, 111 the POSIX one.
    let refused =
        detail.contains("(10061)") || detail.contains("(111)") || lower.contains("refused");

    let reason = if timed_out {
        "the connection timed out. Nothing answered, which usually means a firewall is dropping          the port, or the server is bound to 127.0.0.1 and only accepts local connections"
    } else if refused {
        "the connection was refused. The host is reachable but nothing is listening on that port          — check the server is running, and that this is the right port"
    } else {
        "the server couldn't be reached"
    };

    Some(AppError::ServerUnreachable {
        host: conn.host.clone(),
        port: conn.port,
        reason: reason.to_string(),
    })
}

/// Runs a `SELECT` and hands back its rows.
///
/// `--skip-column-names` drops the header, so a caller's row indexes line
/// up with its `SELECT` list and nothing has to be skipped.
fn query(sql: &str) -> Result<Vec<Vec<String>>, AppError> {
    query_on(&active_conn()?, sql)
}

fn query_on(conn: &Conn, sql: &str) -> Result<Vec<Vec<String>>, AppError> {
    let output = Command::new(conn.client(db_engine::CLIENT_EXE)?)
        .args(conn.args())
        .args(["--batch", "--skip-column-names", "-e", sql])
        .hidden()
        .output()
        .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(client_error(conn, &output.stderr, "the query failed"));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect())
}

/// Runs a statement that returns no rows, refusing it on a read-only
/// target.
fn execute(sql: &str) -> Result<(), AppError> {
    let conn = active_conn()?;
    conn.check_writable()?;
    query_on(&conn, sql).map(|_| ())
}

/// Connects to a server that isn't registered yet and reports its version.
///
/// The add-connection form's "Test" button: everything that can go wrong
/// about a remote server — unreachable host, wrong port, bad credentials,
/// a TLS requirement, a client that can't do its authentication plugin —
/// goes wrong here, once, before the connection is saved and before any of
/// it can be mistaken for a broken feature.
pub struct Probe<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub password: Option<&'a str>,
    pub engine: Engine,
    pub tls: TlsMode,
    pub ssh: Option<&'a SshTunnel>,
    /// The SSH password, when the tunnel authenticates with one. Separate
    /// from `password` above: the box you log into and the database on it
    /// are two different accounts.
    pub ssh_password: Option<&'a str>,
}

pub fn probe(request: Probe<'_>) -> Result<String, AppError> {
    let Probe {
        host,
        port,
        user,
        password,
        engine,
        tls,
        ssh,
        ssh_password,
    } = request;
    let build = connections::client_dir(engine)?;

    // Bound, not dropped: the tunnel has to stay up for the query below,
    // and go away as soon as this function returns — an unsaved connection
    // has no id to file a long-lived tunnel under.
    let tunnelled = match ssh {
        Some(ssh) => Some(tunnel::open(ssh, ssh_password, host.trim(), port)?),
        None => None,
    };
    let (host, port) = match &tunnelled {
        Some(tunnel) => (HOST.to_string(), tunnel.local_port),
        None => (host.trim().to_string(), port),
    };

    let conn = Conn {
        host,
        port,
        user: user.trim().to_string(),
        tls,
        client_engine: build.engine,
        mysql_8_client: build.is_mysql_8_or_newer(),
        bin: build.dir,
        defaults: match password {
            Some(password) if !password.is_empty() => Some(TempDefaults::new(password)?),
            _ => None,
        },
        remote: None,
    };

    let rows = query_on(&conn, "SELECT VERSION()")?;
    Ok(rows
        .into_iter()
        .flatten()
        .next()
        .unwrap_or_else(|| "connected".to_string()))
}

/// Matches a schema to a project by name, so the list can show which site
/// a database belongs to.
///
/// Underscores and hyphens are treated as the same character: a folder
/// can't be named `shop_api` and produce `shop-api.test`, while a schema
/// named `shop-api` needs backticks in every query a developer writes — so
/// in practice the two spellings drift apart for the same project, and
/// matching them up is the whole point of the column.
fn used_by(schema: &str, projects: &[(String, String)]) -> Option<String> {
    let normalize = |s: &str| s.to_lowercase().replace('-', "_");
    let schema = normalize(schema);
    projects
        .iter()
        .find(|(id, _)| normalize(id) == schema)
        .map(|(_, domain)| domain.clone())
}

pub fn list_databases() -> Result<Vec<DatabaseInfo>, AppError> {
    let conn = active_conn()?;

    // Local projects say nothing about the schemas on somebody else's
    // server: a name that happens to match is a coincidence, and claiming
    // a staging database is "used by" a folder on this machine would be a
    // false statement, not a helpful one.
    //
    // Best-effort even locally: a project-scan hiccup should cost the
    // "used by" column, not the whole database list.
    let projects: Vec<(String, String)> = if conn.is_remote() {
        Vec::new()
    } else {
        scan_projects()
            .map(|found| found.into_iter().map(|p| (p.id, p.domain)).collect())
            .unwrap_or_default()
    };

    let excluded = SYSTEM_SCHEMAS
        .map(|schema| format!("'{schema}'"))
        .join(", ");

    // LEFT JOIN, not an inner one: a schema with no tables yet still has to
    // appear in the list (with a count of 0) rather than vanish from it.
    let rows = query_on(
        &conn,
        &format!(
            "SELECT s.schema_name, s.default_collation_name, COUNT(t.table_name), \
             COALESCE(SUM(t.data_length + t.index_length), 0) \
             FROM information_schema.schemata s \
             LEFT JOIN information_schema.tables t ON t.table_schema = s.schema_name \
             WHERE s.schema_name NOT IN ({excluded}) \
             GROUP BY s.schema_name, s.default_collation_name \
             ORDER BY s.schema_name"
        ),
    )?;

    Ok(rows
        .into_iter()
        .filter(|row| row.len() >= 4)
        .map(|row| DatabaseInfo {
            used_by: used_by(&row[0], &projects),
            name: row[0].clone(),
            collation: row[1].clone(),
            table_count: row[2].parse().unwrap_or(0),
            size_bytes: row[3].parse().unwrap_or(0),
        })
        .collect())
}

/// The collations offered in the "New database" dialog, newest-friendly
/// first. Read from the server rather than hard-coded, so switching the
/// bundled MariaDB version — or pointing at somebody else's server — can't
/// leave a stale list behind.
pub fn list_collations() -> Result<Vec<String>, AppError> {
    let rows = query(
        "SELECT collation_name FROM information_schema.collations \
         WHERE character_set_name IN ('utf8mb4', 'utf8mb3') \
         ORDER BY character_set_name DESC, collation_name",
    )?;
    Ok(rows
        .into_iter()
        .filter_map(|row| row.into_iter().next())
        .collect())
}

pub fn create_database(name: &str, collation: &str) -> Result<(), AppError> {
    validate_identifier(name, "database")?;
    validate_identifier(collation, "collation")?;
    execute(&format!("CREATE DATABASE `{name}` COLLATE {collation}"))
}

pub fn drop_database(name: &str) -> Result<(), AppError> {
    validate_identifier(name, "database")?;
    if SYSTEM_SCHEMAS.contains(&name) {
        return Err(AppError::DatabaseQueryFailed(format!(
            "`{name}` is one of MariaDB's own schemas and can't be dropped"
        )));
    }
    execute(&format!("DROP DATABASE `{name}`"))
}

/// `C:\rezure\dumps` — where exports land.
///
/// A fixed, documented folder rather than a save dialog: an export is
/// usually one step of "dump it, then do something with the file", and a
/// predictable path is easier to reach from a terminal afterwards than a
/// location the user has to remember choosing.
pub fn dumps_dir() -> Result<PathBuf, AppError> {
    paths::dumps()
}

/// The dump options that only make sense against a server Rezure doesn't
/// own.
///
/// None of these are needed locally, and two of them don't exist on every
/// client, so they're added only where they earn it:
///
/// * `--single-transaction` — `mysqldump` otherwise takes `LOCK TABLES`
///   across the whole schema. On a private local server that costs
///   nothing; on a shared staging box it blocks every other developer for
///   the length of the dump.
/// * `--quick` — streams rows instead of buffering a whole table in
///   memory, which is the difference between dumping a large remote table
///   and running out of it.
/// * `--skip-column-statistics` — MySQL 8's client queries a table MariaDB
///   has never had, and fails the dump outright with
///   `Unknown table 'COLUMN_STATISTICS'` when pointed at one.
/// * `--set-gtid-purged=OFF` — managed providers (RDS, Aiven) report GTID
///   state the client writes into the dump as `SET @@GLOBAL.gtid_purged`,
///   which then fails to import anywhere the user doesn't have SUPER.
fn remote_dump_args(conn: &Conn) -> Vec<String> {
    let mut args = vec![
        "--single-transaction".to_string(),
        "--quick".to_string(),
        // Large rows (a `LONGBLOB`, a serialised cache table) exceed the
        // client's modest default and abort the dump partway.
        "--max-allowed-packet=512M".to_string(),
    ];
    if conn.mysql_8_client {
        args.push("--skip-column-statistics".to_string());
        args.push("--set-gtid-purged=OFF".to_string());
    }
    args
}

/// How often the destination file's size is checked while a dump is in
/// flight — frequent enough that a progress bar reads as live, not so
/// frequent that polling the filesystem competes with the dump itself for
/// disk time.
const EXPORT_POLL_INTERVAL: Duration = Duration::from_millis(400);

/// What the Databases page renders as a progress bar during
/// [`export_database`].
///
/// `estimated_total_bytes` is exactly that — an estimate, not a promise. A
/// `.sql` dump is text (`INSERT` statements, hex-escaped BLOBs) written from
/// the schema's raw `data_length + index_length`, and the two rarely match
/// byte for byte. The frontend is expected to cap the displayed percentage
/// below 100% until the export actually finishes, the same way a download
/// with a rough `Content-Length` would.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub name: String,
    pub bytes_written: u64,
    pub estimated_total_bytes: Option<u64>,
}

/// Event name the frontend subscribes to via `listen()` for export progress.
pub const EXPORT_PROGRESS_EVENT: &str = "database://export-progress";

fn emit_export_progress(app: &AppHandle, progress: &ExportProgress) {
    // Best-effort — a dropped event shouldn't abort an otherwise-fine
    // export, it just costs the progress bar one tick.
    if let Err(err) = app.emit(EXPORT_PROGRESS_EVENT, progress) {
        log::warn!("failed to emit export progress: {err}");
    }
}

/// A dump in flight, tracked so [`cancel_export`] has something to kill.
struct RunningExport {
    child: Child,
    /// Set by [`cancel_export`], so once the killed process actually exits
    /// [`watch_export`] can tell "the user stopped this" apart from "the
    /// client crashed" — the exit status alone can't distinguish them.
    cancelled: bool,
}

fn export_registry() -> &'static Mutex<HashMap<String, RunningExport>> {
    static EXPORTS: OnceLock<Mutex<HashMap<String, RunningExport>>> = OnceLock::new();
    EXPORTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn exports() -> MutexGuard<'static, HashMap<String, RunningExport>> {
    export_registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// Kills a dump still in progress, if `name` has one.
///
/// A race with the dump finishing on its own is expected and harmless: if
/// it has already exited there's nothing left to kill, and this just
/// reports that rather than treating it as an error.
pub fn cancel_export(name: &str) -> bool {
    match exports().get_mut(name) {
        Some(export) => {
            export.cancelled = true;
            let _ = export.child.kill();
            true
        }
        None => false,
    }
}

/// The schema's current size, in the same bytes [`list_databases`] reports.
///
/// Used only as the progress bar's denominator, so a failure here (a schema
/// that vanished between listing and exporting, a query hiccup) costs a
/// percentage, not the export — callers treat it as best-effort.
fn schema_size_bytes(conn: &Conn, name: &str) -> Result<u64, AppError> {
    let rows = query_on(
        conn,
        &format!(
            "SELECT COALESCE(SUM(data_length + index_length), 0) FROM \
             information_schema.tables WHERE table_schema = '{name}'"
        ),
    )?;
    rows.first()
        .and_then(|row| row.first())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| AppError::DatabaseQueryFailed("no size reported".to_string()))
}

/// Dumps `name` to a timestamped `.sql` file and returns its path.
///
/// A remote dump is prefixed with the connection's name: a `blog-*.sql`
/// pulled from staging and one taken locally are otherwise indistinguishable
/// in the dumps folder, and importing the wrong one is silent.
///
/// `app` is `None` only from the `#[ignore]`d integration test at the bottom
/// of this module, which has no running Tauri app to emit progress through.
/// Every real caller goes through `commands::database::export_database`,
/// which always has one.
pub fn export_database(app: Option<&AppHandle>, name: &str) -> Result<PathBuf, AppError> {
    validate_identifier(name, "database")?;
    let conn = active_conn()?;

    let dir = dumps_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    let prefix = conn
        .remote
        .as_ref()
        .map(|connection| format!("{}-", slugify(&connection.name)))
        .unwrap_or_default();
    let dest = dir.join(format!("{prefix}{name}-{}.sql", timestamp()));

    // Best-effort: a database whose size can't be re-read here still
    // exports, just without a percentage to show for it.
    let estimated_total_bytes = schema_size_bytes(&conn, name).ok();

    // `mariadb-dump` writes the dump to stdout, so this redirects it into
    // the file rather than passing a path the client would have to quote.
    let file = std::fs::File::create(&dest)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dest.display())))?;
    let mut command = Command::new(conn.client(db_engine::DUMP_EXE)?);
    command.args(conn.dump_args());
    if conn.is_remote() {
        command.args(remote_dump_args(&conn));
    }
    // Deliberately *not* `--databases name`: that flag makes `mysqldump`
    // embed its own `CREATE DATABASE`/`USE \`name\`` at the top of the
    // file, and a dump imported into a *different* database name — the
    // whole point of the "Import into" field — would run every statement
    // in it under that embedded `USE` instead, silently writing into the
    // original database (creating it first if it happens not to exist) and
    // leaving the one the user actually asked for empty. Passing `name`
    // bare dumps just that database's tables, with no `USE` to fight the
    // one `import_sql` already selects.
    let child = command
        .arg(name)
        .stdout(file)
        .stderr(Stdio::piped())
        .hidden()
        .spawn()
        .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;

    exports().insert(
        name.to_string(),
        RunningExport {
            child,
            cancelled: false,
        },
    );

    // The registry entry is this function's own, start to finish — removed
    // here regardless of how `watch_export` returned, so a failed or
    // cancelled dump can't leave a dead entry that shadows the next export
    // of the same database.
    let result = watch_export(app, &conn, name, &dest, estimated_total_bytes);
    exports().remove(name);
    result
}

/// Polls the running dump until it exits, emitting [`ExportProgress`] as the
/// destination file grows.
///
/// A file-size poll rather than instrumenting `ssh.exe`'s traffic: the
/// tunnel forwards raw, uncompressed bytes (see `services::tunnel`), so the
/// file this writes is already the most direct measurement of progress
/// there is — no extra plumbing, and it works identically for a local
/// export that has no tunnel at all.
fn watch_export(
    app: Option<&AppHandle>,
    conn: &Conn,
    name: &str,
    dest: &Path,
    estimated_total_bytes: Option<u64>,
) -> Result<PathBuf, AppError> {
    loop {
        std::thread::sleep(EXPORT_POLL_INTERVAL);

        if let Some(app) = app {
            let bytes_written = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            emit_export_progress(
                app,
                &ExportProgress {
                    name: name.to_string(),
                    bytes_written,
                    estimated_total_bytes,
                },
            );
        }

        let mut guard = exports();
        // Only `export_database` ever removes its own entry, and it does so
        // after this loop returns — so a missing one here points at a bug
        // in that bookkeeping, not a race worth handling quietly.
        let export = guard
            .get_mut(name)
            .expect("export_database's own registry entry is missing");
        let status = match export.child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => continue,
            Err(e) => return Err(AppError::DatabaseQueryFailed(e.to_string())),
        };
        let cancelled = export.cancelled;
        let mut stderr = Vec::new();
        if let Some(mut pipe) = export.child.stderr.take() {
            let _ = pipe.read_to_end(&mut stderr);
        }
        drop(guard);

        // Don't leave a half-written or empty .sql behind looking like a
        // successful export, whether it was cancelled or it failed on its
        // own.
        if cancelled {
            let _ = std::fs::remove_file(dest);
            return Err(AppError::ExportCancelled(name.to_string()));
        }
        if !status.success() {
            let _ = std::fs::remove_file(dest);
            return Err(client_error(conn, &stderr, "the export failed"));
        }
        return Ok(dest.to_path_buf());
    }
}

/// A connection name reduced to something safe to put in a filename.
///
/// Connection names are free text — "Staging (EU)" is a reasonable thing to
/// call one — and `:` or `/` in a Windows filename fails the export at the
/// point where the dump has already been taken.
fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').replace("--", "-");
    if slug.is_empty() {
        "remote".to_string()
    } else {
        slug
    }
}

/// Pipes a `.sql` file into `name`, creating the database if it isn't
/// there yet — importing a dump into a database you have to remember to
/// create first is a papercut with no upside.
///
/// Refused outright on a read-only connection: this is the one operation
/// here that can destroy data on a server Rezure doesn't own, and the
/// confirmation the UI asks for is a second gate, not this one.
pub fn import_sql(name: &str, file: &Path) -> Result<(), AppError> {
    validate_identifier(name, "database")?;
    if !file.is_file() {
        return Err(AppError::Io(format!("no such file: {}", file.display())));
    }
    let conn = active_conn()?;
    conn.check_writable()?;

    query_on(&conn, &format!("CREATE DATABASE IF NOT EXISTS `{name}`"))?;

    let input = std::fs::File::open(file)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", file.display())))?;
    let mut command = Command::new(conn.client(db_engine::CLIENT_EXE)?);
    command.args(conn.args());
    if conn.is_remote() {
        // The counterpart to the dump side: a statement holding one large
        // row is rejected by the server's own limit otherwise.
        command.arg("--max-allowed-packet=512M");
    }
    let output = command
        .arg(name)
        .stdin(input)
        .hidden()
        .output()
        .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(client_error(&conn, &output.stderr, "the import failed"));
    }
    Ok(())
}

/// `YYYYMMDD-HHMMSS` in UTC, for export filenames.
///
/// Hand-rolled from a Unix timestamp rather than pulling in a date crate
/// for one format string — it's only ever read as "which dump is newer".
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// Days-since-epoch to a calendar date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_for(tls: TlsMode, client_engine: Engine, remote: Option<Connection>) -> Conn {
        Conn {
            host: "db.example.com".to_string(),
            port: 3307,
            user: "app".to_string(),
            tls,
            client_engine,
            mysql_8_client: client_engine == Engine::MySql,
            bin: PathBuf::from(r"C:\rezure\bin\mysql\8.4.0\bin"),
            defaults: None,
            remote,
        }
    }

    fn remote_connection(read_only: bool) -> Connection {
        Connection {
            id: "c1".to_string(),
            name: "Staging (EU)".to_string(),
            host: "db.example.com".to_string(),
            port: 3307,
            user: "app".to_string(),
            engine: Engine::MySql,
            tls_mode: TlsMode::Required,
            read_only,
            save_password: false,
            ssh: None,
            last_used_at: None,
        }
    }

    #[test]
    fn plain_identifiers_are_accepted() {
        for name in ["blog", "shop_api", "client-cms", "db2"] {
            assert!(validate_identifier(name, "database").is_ok(), "{name}");
        }
    }

    /// The whole point of the check — an identifier can't be bound as a
    /// parameter, so anything that could end a statement and start another
    /// has to be refused before it reaches the client.
    #[test]
    fn identifiers_that_could_break_out_of_a_statement_are_refused() {
        for name in [
            "blog`; DROP DATABASE `mysql",
            "blog; SELECT 1",
            "blog'",
            "blog db",
            "",
            "café",
        ] {
            assert!(
                validate_identifier(name, "database").is_err(),
                "{name:?} must be refused"
            );
        }
    }

    #[test]
    fn an_over_long_identifier_is_refused() {
        assert!(validate_identifier(&"a".repeat(65), "database").is_err());
    }

    #[test]
    fn dropping_a_system_schema_is_refused_before_it_reaches_the_server() {
        for schema in SYSTEM_SCHEMAS {
            assert!(drop_database(schema).is_err(), "{schema}");
        }
    }

    #[test]
    fn used_by_matches_a_project_across_underscores_and_hyphens() {
        let projects = vec![
            ("shop-api".to_string(), "shop-api.test".to_string()),
            ("blog".to_string(), "blog.test".to_string()),
        ];
        assert_eq!(
            used_by("shop_api", &projects).as_deref(),
            Some("shop-api.test")
        );
        assert_eq!(used_by("blog", &projects).as_deref(), Some("blog.test"));
        assert_eq!(used_by("sandbox", &projects), None);
    }

    /// `--defaults-file` is only honoured as the first argument, and the
    /// password is only reachable through it — so this ordering is what
    /// keeps the secret off the command line.
    #[test]
    fn the_defaults_file_comes_first_when_there_is_a_password() {
        let mut conn = conn_for(TlsMode::Preferred, Engine::MySql, None);
        conn.defaults = Some(TempDefaults::new("s3cret").unwrap());
        let args = conn.args();
        assert!(args[0].starts_with("--defaults-file="), "{args:?}");
        assert!(
            !args.iter().any(|arg| arg.contains("s3cret")),
            "the password must never appear in the arguments: {args:?}"
        );
    }

    /// MariaDB's `mysqldump` rejects `--connect-timeout` with "unknown
    /// variable", which made every export fail — including local ones, where
    /// the option was never earning anything.
    #[test]
    fn the_dump_client_is_not_given_an_option_it_does_not_register() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        assert!(
            conn.args()
                .iter()
                .any(|arg| arg.contains("connect-timeout")),
            "the console client still gets its deadline: {:?}",
            conn.args()
        );
        assert!(
            !conn
                .dump_args()
                .iter()
                .any(|arg| arg.contains("connect-timeout")),
            "mysqldump must not be handed it: {:?}",
            conn.dump_args()
        );
    }

    #[test]
    fn a_target_without_a_password_still_refuses_the_machines_own_option_files() {
        let conn = conn_for(TlsMode::Preferred, Engine::MySql, None);
        assert_eq!(conn.args()[0], "--no-defaults");
    }

    #[test]
    fn tls_is_spelled_the_way_each_client_understands_it() {
        assert_eq!(
            conn_for(TlsMode::Required, Engine::MySql, None).tls_args(),
            vec!["--ssl-mode=REQUIRED"]
        );
        assert_eq!(
            conn_for(TlsMode::Required, Engine::MariaDb, None).tls_args(),
            vec!["--ssl"]
        );
        assert_eq!(
            conn_for(TlsMode::Disabled, Engine::MariaDb, None).tls_args(),
            vec!["--skip-ssl"]
        );
        // The client's own default already does this; saying it would only
        // risk an unknown option on an older build.
        assert!(conn_for(TlsMode::Preferred, Engine::MySql, None)
            .tls_args()
            .is_empty());
    }

    #[test]
    fn a_read_only_connection_refuses_writes_before_reaching_the_server() {
        let conn = conn_for(
            TlsMode::Required,
            Engine::MySql,
            Some(remote_connection(true)),
        );
        let err = conn.check_writable().expect_err("a write must be refused");
        assert!(err.to_string().contains("Staging (EU)"), "{err}");
    }

    #[test]
    fn a_writable_connection_allows_writes() {
        let conn = conn_for(
            TlsMode::Required,
            Engine::MySql,
            Some(remote_connection(false)),
        );
        assert!(conn.check_writable().is_ok());
    }

    #[test]
    fn the_local_target_is_never_read_only() {
        assert!(conn_for(TlsMode::Preferred, Engine::MariaDb, None)
            .check_writable()
            .is_ok());
    }

    #[test]
    fn remote_dumps_never_lock_the_whole_schema() {
        let conn = conn_for(
            TlsMode::Required,
            Engine::MariaDb,
            Some(remote_connection(true)),
        );
        let args = remote_dump_args(&conn);
        assert!(
            args.contains(&"--single-transaction".to_string()),
            "{args:?}"
        );
        // MariaDB's client has neither option; passing them aborts the dump.
        assert!(!args.iter().any(|a| a.contains("column-statistics")));
        assert!(!args.iter().any(|a| a.contains("gtid-purged")));
    }

    #[test]
    fn a_mysql_8_client_gets_the_two_options_only_it_has() {
        let conn = conn_for(
            TlsMode::Required,
            Engine::MySql,
            Some(remote_connection(true)),
        );
        let args = remote_dump_args(&conn);
        assert!(args.contains(&"--skip-column-statistics".to_string()));
        assert!(args.contains(&"--set-gtid-purged=OFF".to_string()));
    }

    /// The message the user actually hit: a VPS with 22, 80 and 443 open
    /// and 3306 dropped by the firewall.
    #[test]
    fn a_timed_out_connect_names_the_host_and_the_likely_cause() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        let err = connect_failure(
            &conn,
            "ERROR 2002 (HY000): Can't connect to server on '203.0.113.10' (138)",
        )
        .expect("a connect failure must be rewritten");
        let message = err.to_string();
        assert!(message.contains("db.example.com:3307"), "{message}");
        assert!(message.contains("firewall"), "{message}");
        assert!(
            !message.contains("138"),
            "the raw errno helps nobody: {message}"
        );
    }

    #[test]
    fn a_refused_connect_is_not_reported_as_a_firewall_problem() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        let message = connect_failure(
            &conn,
            "ERROR 2002 (HY000): Can't connect to server on 'localhost' (10061)",
        )
        .expect("a connect failure must be rewritten")
        .to_string();
        assert!(message.contains("nothing is listening"), "{message}");
        assert!(!message.contains("firewall"), "{message}");
    }

    /// Everything that isn't a connect failure has to reach the user
    /// unchanged — the server's own wording is the useful part.
    #[test]
    fn an_ordinary_server_error_is_passed_through_untouched() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        assert!(connect_failure(&conn, "ERROR 1049 (42000): Unknown database 'nope'").is_none());
        assert!(connect_failure(&conn, "ERROR 1045 (28000): Access denied for user").is_none());
    }

    /// Reached through a working tunnel: the connection is fine, the
    /// account simply can't be used with a password.
    #[test]
    fn socket_authentication_is_explained_rather_than_left_as_access_denied() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        let message = client_error(
            &conn,
            b"ERROR 1698 (28000): Access denied for user 'root'@'localhost'",
            "the query failed",
        )
        .to_string();
        // The server's own words survive — they are the accurate part.
        assert!(message.contains("ERROR 1698"), "{message}");
        assert!(message.contains("operating-system user"), "{message}");
    }

    #[test]
    fn a_plugin_the_client_cannot_speak_names_the_fix() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        let message = client_error(
            &conn,
            b"ERROR 2059 (HY000): Authentication plugin 'caching_sha2_password' cannot be loaded",
            "the query failed",
        )
        .to_string();
        assert!(message.contains("MariaDB client can't speak"), "{message}");
    }

    /// The exact failure of importing a MySQL 8 dump into MariaDB: no wrong
    /// credential or grant is involved, just a collation MariaDB has never
    /// implemented.
    #[test]
    fn a_mysql_8_only_collation_names_the_cross_engine_cause() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        let message = client_error(
            &conn,
            b"ERROR 1273 (HY000) at line 22: Unknown collation: 'utf8mb4_0900_ai_ci'",
            "the import failed",
        )
        .to_string();
        assert!(message.contains("ERROR 1273"), "{message}");
        assert!(message.contains("MySQL 8 server"), "{message}");
        assert!(message.contains("utf8mb4_unicode_ci"), "{message}");
    }

    #[test]
    fn an_error_with_no_known_pitfall_is_left_exactly_as_the_server_wrote_it() {
        let conn = conn_for(TlsMode::Preferred, Engine::MariaDb, None);
        let raw = "ERROR 1049 (42000): Unknown database 'nope'";
        assert_eq!(
            client_error(&conn, raw.as_bytes(), "the query failed").to_string(),
            raw
        );
    }

    #[test]
    fn option_values_escape_what_an_option_file_would_otherwise_eat() {
        assert_eq!(escape_option_value(r#"pa\ss"word"#), r#"pa\\ss\"word"#);
        assert_eq!(escape_option_value("plain"), "plain");
    }

    /// The ordinary race: the user clicks Cancel just as the dump finishes
    /// on its own. This must read as "nothing to stop", not as an error.
    #[test]
    fn cancelling_an_export_that_is_not_running_reports_nothing_to_stop() {
        assert!(!cancel_export("no_such_export_is_running"));
    }

    #[test]
    fn a_connection_name_becomes_a_safe_filename_prefix() {
        assert_eq!(slugify("Staging (EU)"), "staging-eu");
        assert_eq!(slugify("prod/db:1"), "prod-db-1");
        // Nothing usable left — the prefix still has to be a valid name.
        assert_eq!(slugify("!!!"), "remote");
    }

    #[test]
    fn the_temporary_credentials_file_is_deleted_when_it_goes_out_of_scope() {
        let path = {
            let defaults = TempDefaults::new("s3cret").unwrap();
            assert!(defaults.path.is_file());
            defaults.path.clone()
        };
        assert!(
            !path.exists(),
            "the password file must not outlive the command"
        );
    }

    #[test]
    fn timestamps_are_sortable_and_the_right_shape() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), 15, "{stamp}");
        assert_eq!(&stamp[8..9], "-");
        assert!(stamp.chars().filter(|c| c.is_ascii_digit()).count() == 14);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // A leap day, the case an off-by-one in the algorithm would break.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    /// Talks to a really-running MariaDB. Start it in Rezure first, then:
    /// `cargo test --lib services::database::tests::round_trip_against_a_real_server -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn round_trip_against_a_real_server() {
        let name = "rezure_selftest_db";
        let _ = drop_database(name);

        create_database(name, "utf8mb4_unicode_ci").unwrap();
        let found = list_databases().unwrap();
        let created = found
            .iter()
            .find(|db| db.name == name)
            .expect("the new database must show up in the list");
        assert_eq!(created.collation, "utf8mb4_unicode_ci");
        assert_eq!(created.table_count, 0);

        let dump = export_database(None, name).unwrap();
        assert!(dump.is_file(), "export must leave a real file behind");
        println!("dumped to {}", dump.display());

        drop_database(name).unwrap();
        assert!(
            !list_databases().unwrap().iter().any(|db| db.name == name),
            "the dropped database must be gone from the list"
        );
        let _ = std::fs::remove_file(dump);
    }

    /// The bug this guards against: `mysqldump --databases` bakes a `USE
    /// \`source\`;` into the file, and importing it under a *different*
    /// name used to run every statement under that embedded `USE` instead
    /// — silently recreating `source` (even after it had just been
    /// dropped) and leaving the name actually typed into "Import into"
    /// empty.
    ///
    /// `cargo test --lib services::database::tests::an_export_survives_being_imported_under_a_new_name -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn an_export_survives_being_imported_under_a_new_name() {
        let source = "rezure_selftest_rename_src";
        let target = "rezure_selftest_rename_dst";
        let _ = drop_database(source);
        let _ = drop_database(target);

        create_database(source, "utf8mb4_unicode_ci").unwrap();
        execute(&format!("CREATE TABLE `{source}`.`t` (id INT PRIMARY KEY)")).unwrap();
        execute(&format!("INSERT INTO `{source}`.`t` VALUES (1)")).unwrap();

        let dump = export_database(None, source).unwrap();

        // Gone, so a dump that still names `source` internally has nothing
        // to silently resurrect — if the bug were back, it would reappear
        // here instead of `target` getting the data.
        drop_database(source).unwrap();

        import_sql(target, &dump).unwrap();

        let found = list_databases().unwrap();
        assert!(
            found
                .iter()
                .any(|db| db.name == target && db.table_count == 1),
            "the renamed import must land in {target}, not {source}: {found:?}"
        );
        assert!(
            !found.iter().any(|db| db.name == source),
            "the import must not have silently recreated {source}: {found:?}"
        );

        drop_database(target).unwrap();
        let _ = std::fs::remove_file(dump);
    }
}
