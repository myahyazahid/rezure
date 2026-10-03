//! The Databases page's SQL Server client — list, create, drop, export and
//! import, against Rezure's own LocalDB instance or a SQL Server elsewhere.
//!
//! # Why ODBC and not a client binary
//!
//! The MySQL side drives the `mysql.exe` that ships beside every server
//! build (`services::database`). SQL Server has no equivalent that comes
//! for free — but the Microsoft ODBC Driver is already a hard requirement
//! for PHP's `sqlsrv` extensions (`services::odbc`), so talking through it
//! costs no second download, returns typed rows instead of text to parse,
//! and gets Windows Authentication and LocalDB's named pipe from the driver
//! rather than from Rezure.
//!
//! # Draining every result
//!
//! SQL Server reports the outcome of a batch one result at a time: an error
//! in its third statement, or the "Processed 320 pages" messages a `BACKUP`
//! streams while it runs, only arrive as the client asks for the next result
//! (`SQLMoreResults`). A client that stops asking and closes the statement
//! instead sends the server an *attention* — which cancels a `BACKUP` or
//! `RESTORE` mid-way and swallows the error of any statement after the first.
//! [`run`] therefore walks every result of every batch to the end.
//!
//! # Names
//!
//! Database names are interpolated into statements — they can't be bound as
//! parameters — so they go through the same `validate_identifier` as the
//! MySQL side before getting here, and are bracket-quoted on top of that.
//! File paths go into `N'…'` literals with `'` doubled.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use odbc_api::handles::{SqlText, Statement};
use odbc_api::{ConnectionOptions, Cursor, CursorImpl, ResultSetMetadata};
use tauri::AppHandle;

use super::database::{self, DatabaseInfo, DbClient, ExportProgress, ServerInfo};
use super::odbc::{self, OdbcDriver};
use super::projects::scan_projects;
use super::{connections, mssql_localdb, secrets, tunnel};
use crate::config::connections::{Connection, ServerKind, SshTunnel, TlsMode};
use crate::utils::error::AppError;

/// How long a connect may take before it's called unreachable — the same
/// ten seconds the MySQL client is given (`database::CONNECT_TIMEOUT_SECS`).
const LOGIN_TIMEOUT_SECS: u32 = 10;

/// The databases SQL Server itself owns — never listed, never droppable.
const SYSTEM_DATABASES: [&str; 4] = ["master", "tempdb", "model", "msdb"];

/// The collations offered for a new database, when the server has them —
/// the server's own default is always offered first on top of these.
///
/// A short list on purpose: `sys.fn_helpcollations()` returns over five
/// thousand names, and the choice a developer actually faces is "the classic
/// default, or UTF-8". The `_UTF8` ones exist from SQL Server 2019.
const OFFERED_COLLATIONS: [&str; 7] = [
    "SQL_Latin1_General_CP1_CI_AS",
    "Latin1_General_CI_AS",
    "Latin1_General_100_CI_AS_SC",
    "Latin1_General_100_CI_AS_SC_UTF8",
    "Latin1_General_100_CS_AS_SC_UTF8",
    "Latin1_General_100_BIN2",
    "Latin1_General_100_BIN2_UTF8",
];

/// How a connection signs in.
#[derive(Clone)]
enum Auth {
    /// As the Windows user Rezure runs as — no password anywhere.
    Windows,
    Sql {
        user: String,
        password: Option<String>,
    },
}

/// Everything needed to open a connection to one SQL Server.
pub struct Client {
    driver: OdbcDriver,
    /// What goes after `Server=` — `host,port`, `host\INSTANCE`, or
    /// `(localdb)\Rezure`.
    server: String,
    /// The endpoint as the user knows it, for error messages.
    host: String,
    port: u16,
    auth: Auth,
    tls: TlsMode,
    trust_server_certificate: bool,
    /// The saved connection this is; `None` for a probe of an unsaved form.
    connection: Option<Connection>,
    /// Kept alive while it's in use: dropping it closes the SSH forward a
    /// probe opened.
    _tunnel: Option<tunnel::Tunnel>,
}

/// `Server=` for a host and port. Port 0 means the user left it empty — a
/// named instance (`host\SQLEXPRESS`), whose port SQL Server Browser hands
/// out — so the host goes in as typed.
fn server_address(host: &str, port: u16) -> String {
    if port == 0 {
        host.to_string()
    } else {
        format!("{host},{port}")
    }
}

/// Quotes a connection-string value when it needs it.
///
/// `;` ends a value and `{`/`}` delimit a quoted one, so a password holding
/// either would otherwise be cut short or read as something else — and fail
/// login with nothing pointing at the cause. Inside braces only `}` needs
/// escaping, by doubling.
fn odbc_value(value: &str) -> String {
    let needs_quoting = value.contains([';', '{', '}', '='])
        || value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace);
    if needs_quoting {
        format!("{{{}}}", value.replace('}', "}}"))
    } else {
        value.to_string()
    }
}

/// `[name]`, with any `]` doubled — T-SQL's own escape for a bracketed name.
fn bracket(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

/// `N'text'`, with any `'` doubled.
fn nstring(text: &str) -> String {
    format!("N'{}'", text.replace('\'', "''"))
}

/// The first line of a diagnostic, without the `[Microsoft][ODBC Driver 18
/// for SQL Server][SQL Server]` prefix that leads every one of them and says
/// nothing the user doesn't already know.
fn clean_message(message: &str) -> String {
    let mut rest = message.trim();
    while let Some(stripped) = rest.strip_prefix('[') {
        match stripped.split_once(']') {
            Some((_, after)) => rest = after.trim_start(),
            None => break,
        }
    }
    rest.lines().next().unwrap_or(rest).trim().to_string()
}

/// SQLSTATEs that mean the server was never reached — as opposed to reached
/// and refusing, which keeps the server's own wording.
fn is_unreachable(state: &str) -> bool {
    matches!(state, "08001" | "08S01" | "HYT00" | "HYT01")
}

/// Why a connect never reached a server, in words.
///
/// Spelled out here rather than passed through: the driver's first
/// diagnostic for a failed connect is often a bare fragment — literally
/// `SQL Server Network Interfaces:` — with the explanation in a later record
/// that never makes it into the error. The two causes are kept apart for the
/// same reason `database::connect_failure` keeps them apart.
fn unreachable_reason(state: &str) -> &'static str {
    if state.starts_with("HYT") {
        "the connection timed out. Nothing answered, which usually means a firewall is dropping \
         the port, or SQL Server isn't accepting TCP/IP connections"
    } else {
        "the server wasn't found. Check the host and port (or instance name), and that SQL \
         Server is running and allows remote connections"
    }
}

/// Errors whose wording is right but doesn't say what to do — the SQL
/// Server counterpart of `database::hint_for`.
fn hint_for(message: &str) -> Option<&'static str> {
    let lower = message.to_lowercase();
    if lower.contains("certificate") {
        return Some(
            "ODBC Driver 18 encrypts by default and refuses a certificate it can't verify, which \
             is what most development and in-house servers have. Turn on \"Trust server \
             certificate\" for this connection if you trust the server — or, in a Laravel \
             project, uncomment 'trust_server_certificate' in config/database.php and set \
             DB_TRUST_SERVER_CERTIFICATE=true",
        );
    }
    if lower.contains("login failed") && lower.contains("untrusted domain") {
        return Some(
            "Windows Authentication only works for accounts the server's domain knows — sign \
             in with a SQL Server login and password instead",
        );
    }
    None
}

impl Client {
    /// The client for a saved connection.
    pub fn for_connection(connection: Connection) -> Result<Self, AppError> {
        let driver = odbc::require()?;

        let (server, host, port) = if connection.managed {
            // Named instances aren't created on demand, and a stopped one
            // is quicker to start explicitly than to wait out a login
            // timeout for.
            mssql_localdb::ensure_running()?;
            // Best-effort here: the page works without it, only handing off
            // to a TCP-only client needs it, and that path checks again.
            if let Err(err) = ensure_localdb_login() {
                log::warn!("could not set up the {LOCALDB_LOGIN} login on LocalDB: {err}");
            }
            (
                mssql_localdb::SERVER_ADDRESS.to_string(),
                connection.host.clone(),
                0,
            )
        } else if connection.ssh.is_some() {
            let local = tunnel::ensure(&connection)?;
            (
                server_address(database::HOST, local),
                database::HOST.to_string(),
                local,
            )
        } else {
            (
                server_address(&connection.host, connection.port),
                connection.host.clone(),
                connection.port,
            )
        };

        let auth = if connections::needs_no_password(&connection) {
            Auth::Windows
        } else {
            Auth::Sql {
                user: connection.user.clone(),
                password: secrets::resolve(&connection.id),
            }
        };

        Ok(Self {
            driver,
            server,
            host,
            port,
            auth,
            tls: connection.tls_mode,
            trust_server_certificate: connection.trust_server_certificate,
            connection: Some(connection),
            _tunnel: None,
        })
    }

    fn connection_string(&self, database: &str) -> String {
        let mut cs = format!(
            "Driver={{{}}};Server={};Database={};APP=Rezure;",
            self.driver.name,
            odbc_value(&self.server),
            odbc_value(database),
        );
        match &self.auth {
            Auth::Windows => cs.push_str("Trusted_Connection=yes;"),
            Auth::Sql { user, password } => {
                cs.push_str(&format!("UID={};", odbc_value(user)));
                if let Some(password) = password {
                    cs.push_str(&format!("PWD={};", odbc_value(password)));
                }
            }
        }
        // "Automatic" leaves the driver's own default alone: encrypt on 18,
        // don't on 17 — the same "say nothing" the MySQL side does for it.
        match self.tls {
            TlsMode::Preferred => {}
            TlsMode::Required => cs.push_str("Encrypt=yes;"),
            TlsMode::Disabled => cs.push_str("Encrypt=no;"),
        }
        if self.trust_server_certificate {
            cs.push_str("TrustServerCertificate=yes;");
        }
        cs
    }

    /// Whether this is Rezure's own LocalDB — the one SQL Server treated
    /// like the local MySQL: writable, files kept under `data\mssql`.
    fn is_managed(&self) -> bool {
        self.connection.as_ref().is_some_and(|c| c.managed)
    }

    fn is_remote(&self) -> bool {
        !self.is_managed()
    }

    /// Refuses a write on a connection marked read-only, before anything
    /// reaches the server.
    fn check_writable(&self) -> Result<(), AppError> {
        match &self.connection {
            Some(connection) if connection.read_only => Err(AppError::ConnectionReadOnly {
                name: connection.name.clone(),
            }),
            _ => Ok(()),
        }
    }

    /// Turns an ODBC error into one worth showing.
    fn error(&self, err: odbc_api::Error) -> AppError {
        let (state, message) = match &err {
            odbc_api::Error::Diagnostics { record, .. } => (
                record.state.as_str().to_string(),
                clean_message(&odbc_api::handles::slice_to_cow_utf8(&record.message)),
            ),
            other => (String::new(), other.to_string()),
        };

        // "Data source name not found and no default driver specified" —
        // the driver was there when this client was built and isn't now.
        if state == "IM002" {
            return AppError::OdbcDriverMissing;
        }
        if is_unreachable(&state) && !message.to_lowercase().contains("certificate") {
            let location = if self.port == 0 {
                self.host.clone()
            } else {
                format!("{}:{}", self.host, self.port)
            };
            return AppError::DatabaseQueryFailed(format!(
                "can't reach {location} — {}",
                unreachable_reason(&state)
            ));
        }
        match hint_for(&message) {
            Some(hint) => AppError::DatabaseQueryFailed(format!("{message} — {hint}")),
            None => AppError::DatabaseQueryFailed(message),
        }
    }

    fn connect(&self, database: &str) -> Result<odbc_api::Connection<'static>, AppError> {
        let environment = odbc_api::environment()
            .map_err(|e| AppError::Database(format!("could not open ODBC: {e}")))?;
        environment
            .connect_with_connection_string(
                &self.connection_string(database),
                ConnectionOptions {
                    login_timeout_sec: Some(LOGIN_TIMEOUT_SECS),
                    packet_size: None,
                },
            )
            .map_err(|e| self.error(e))
    }

    /// Opens a connection and runs one batch on it.
    fn query(&self, database: &str, sql: &str) -> Result<Vec<Vec<Option<String>>>, AppError> {
        let conn = self.connect(database)?;
        run(&conn, sql).map_err(|e| self.error(e))
    }
}

/// Runs one batch and returns the rows of every result set it produced, each
/// value as text — `None` for `NULL`.
///
/// Walks every result to the end; see the module docs for why stopping
/// early is not an option.
fn run(
    conn: &odbc_api::Connection<'_>,
    sql: &str,
) -> Result<Vec<Vec<Option<String>>>, odbc_api::Error> {
    // An owned statement handle, so the cursor wrapped around it below never
    // closes it on drop — only freeing the handle ends the batch.
    let mut statement = conn.preallocate()?.into_handle();
    let text = SqlText::new(sql);
    // Safe: no parameters are bound, so nothing the call could dereference
    // has to outlive it.
    unsafe { statement.exec_direct(&text) }
        .on_no_data(|| ())
        .into_result(&statement)?;

    // Safe: a statement that has just been executed may be treated as a
    // cursor; one with no columns simply has no rows to fetch, and asking
    // for its next result is exactly what has to happen next.
    let mut cursor = unsafe { CursorImpl::new(statement) };
    let mut rows = Vec::new();
    let mut buffer = Vec::new();
    loop {
        let columns = cursor.num_result_cols()?;
        if columns > 0 {
            while let Some(mut row) = cursor.next_row()? {
                let mut values = Vec::with_capacity(columns as usize);
                for column in 1..=columns as u16 {
                    values.push(
                        row.get_wide_text(column, &mut buffer)?
                            .then(|| String::from_utf16_lossy(&buffer)),
                    );
                }
                rows.push(values);
            }
        }
        match cursor.more_results()? {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(rows)
}

fn text(row: &[Option<String>], index: usize) -> String {
    row.get(index).cloned().flatten().unwrap_or_default()
}

fn number(row: &[Option<String>], index: usize) -> u64 {
    text(row, index).trim().parse().unwrap_or(0)
}

/// The fields of an unsaved connection, for "Test connection".
pub struct Probe<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub password: Option<&'a str>,
    pub windows_auth: bool,
    pub tls: TlsMode,
    pub trust_server_certificate: bool,
    pub ssh: Option<&'a SshTunnel>,
    pub ssh_password: Option<&'a str>,
}

/// Connects with a form's values and reports the server's version — the
/// first line of `@@VERSION`, e.g. `Microsoft SQL Server 2022 (RTM) -
/// 16.0.1000.6 (X64)`.
pub fn probe(request: Probe<'_>) -> Result<String, AppError> {
    let driver = odbc::require()?;
    let host = request.host.trim();
    let opened = match request.ssh {
        Some(ssh) => Some(tunnel::open(ssh, request.ssh_password, host, request.port)?),
        None => None,
    };
    let (server, shown_host, port) = match &opened {
        Some(tunnel) => (
            server_address(database::HOST, tunnel.local_port),
            database::HOST.to_string(),
            tunnel.local_port,
        ),
        None => (
            server_address(host, request.port),
            host.to_string(),
            request.port,
        ),
    };
    let client = Client {
        driver,
        server,
        host: shown_host,
        port,
        auth: if request.windows_auth {
            Auth::Windows
        } else {
            Auth::Sql {
                user: request.user.trim().to_string(),
                password: request
                    .password
                    .filter(|p| !p.is_empty())
                    .map(str::to_string),
            }
        },
        tls: request.tls,
        trust_server_certificate: request.trust_server_certificate,
        connection: None,
        _tunnel: opened,
    };

    let rows = client.query("master", "SELECT @@VERSION")?;
    let version = rows.first().map(|row| text(row, 0)).unwrap_or_default();
    Ok(version
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("connected")
        .to_string())
}

/// The SQL login SQL clients that can't do Windows Authentication use on
/// Rezure's LocalDB — see `services::localdb_bridge`.
pub const LOCALDB_LOGIN: &str = "rezure";

/// Makes sure Rezure's LocalDB has the passwordless `rezure` login, as
/// `sysadmin`.
///
/// The same stance as Rezure's MariaDB and its passwordless `root`, decided
/// the same way: a throwaway development server, reachable only from this
/// machine — over its pipe, or the bridge on `127.0.0.1` — where a password
/// would only move into a client's saved settings. It exists because
/// TablePlus can't sign in with Windows Authentication at all.
///
/// Done once per run, through a Windows-authenticated connection — the
/// account that created the instance is its `sysadmin`.
pub fn ensure_localdb_login() -> Result<(), AppError> {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.load(Ordering::SeqCst) {
        return Ok(());
    }
    mssql_localdb::ensure_running()?;
    let client = Client {
        driver: odbc::require()?,
        server: mssql_localdb::SERVER_ADDRESS.to_string(),
        host: mssql_localdb::SERVER_ADDRESS.to_string(),
        port: 0,
        auth: Auth::Windows,
        tls: TlsMode::Preferred,
        trust_server_certificate: false,
        connection: None,
        _tunnel: None,
    };
    let login = nstring(LOCALDB_LOGIN);
    client.query(
        "master",
        &format!(
            "IF SUSER_ID({login}) IS NULL \
             CREATE LOGIN {name} WITH PASSWORD = N'', CHECK_POLICY = OFF, CHECK_EXPIRATION = OFF; \
             IF IS_SRVROLEMEMBER(N'sysadmin', {login}) = 0 \
             ALTER SERVER ROLE [sysadmin] ADD MEMBER {name};",
            name = bracket(LOCALDB_LOGIN),
        ),
    )?;
    DONE.store(true, Ordering::SeqCst);
    Ok(())
}

/// Where a SQL client on this machine connects to reach a SQL Server
/// connection over TCP — what `services::db_clients` hands TablePlus,
/// DBeaver and HeidiSQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpTarget {
    pub host: String,
    /// `None` for a named instance, whose port SQL Server Browser hands out.
    pub port: Option<u16>,
    /// `None` means Windows Authentication.
    pub user: Option<String>,
    /// The server offers no TLS at all — Rezure's LocalDB through the bridge
    /// — so a client that would insist on it has to be told not to.
    pub no_encryption: bool,
    /// Accept a certificate that can't be verified — the connection's own
    /// setting.
    pub trust_certificate: bool,
}

/// The TCP endpoint for `connection`, or `None` when there isn't one a
/// client could use: Rezure's LocalDB while its bridge isn't listening.
pub fn tcp_target(connection: &Connection) -> Option<TcpTarget> {
    if connection.managed {
        return super::localdb_bridge::listening_port().map(|port| TcpTarget {
            host: database::HOST.to_string(),
            port: Some(port),
            user: Some(LOCALDB_LOGIN.to_string()),
            no_encryption: true,
            trust_certificate: false,
        });
    }
    let (host, port) = match connection
        .ssh
        .as_ref()
        .and_then(|_| tunnel::existing_port(&connection.id))
    {
        Some(local) => (database::HOST.to_string(), local),
        None => (connection.host.clone(), connection.port),
    };
    Some(TcpTarget {
        host,
        port: (port != 0).then_some(port),
        user: (!connections::needs_no_password(connection)).then(|| connection.user.clone()),
        no_encryption: connection.tls_mode == TlsMode::Disabled,
        trust_certificate: connection.trust_server_certificate,
    })
}

/// What another client on this machine has to dial to reach `connection` —
/// for handing off to SSMS. A tunnelled connection's own host only means
/// something on the SSH server, so it's the live forward's port, when there
/// is one.
pub fn client_server_name(connection: &Connection) -> String {
    if connection.managed {
        return mssql_localdb::SERVER_ADDRESS.to_string();
    }
    match connection
        .ssh
        .as_ref()
        .and_then(|_| tunnel::existing_port(&connection.id))
    {
        Some(local) => server_address(database::HOST, local),
        None => server_address(&connection.host, connection.port),
    }
}

/// What the Databases page shows about a SQL Server connection.
pub fn server_info(connection: &Connection) -> ServerInfo {
    let windows = connections::needs_no_password(connection);
    let server = if connection.managed {
        mssql_localdb::SERVER_ADDRESS.to_string()
    } else {
        server_address(&connection.host, connection.port)
    };
    ServerInfo {
        host: connection.host.clone(),
        port: connection.port,
        user: if windows {
            "Windows user".to_string()
        } else {
            connection.user.clone()
        },
        has_password: !windows && secrets::resolve(&connection.id).is_some(),
        // The DSN PHP's pdo_sqlsrv takes, which is also what Laravel builds
        // from DB_HOST/DB_PORT.
        dsn: format!("sqlsrv:Server={server}"),
        remote: !connection.managed,
        label: connection.name.clone(),
        read_only: connection.read_only,
        kind: ServerKind::Sqlserver,
        // BACKUP and RESTORE read and write the *server's* disk; only
        // Rezure's own LocalDB shares this machine's.
        export_supported: connection.managed,
        import_extensions: if connection.managed {
            vec!["sql".to_string(), "bak".to_string()]
        } else {
            vec!["sql".to_string()]
        },
    }
}

impl DbClient for Client {
    fn list_databases(&self) -> Result<Vec<DatabaseInfo>, AppError> {
        // `sys.master_files` sizes are in 8 KB pages. A login without the
        // permission to see them gets no rows from it, which reads as a
        // size of zero rather than a failure.
        let rows = self.query(
            "master",
            "SET NOCOUNT ON; \
             SELECT d.name, d.collation_name, \
                    COALESCE(SUM(CAST(f.size AS bigint)), 0) * 8192 \
             FROM sys.databases d \
             LEFT JOIN sys.master_files f ON f.database_id = d.database_id \
             WHERE d.database_id > 4 AND HAS_DBACCESS(d.name) = 1 \
             GROUP BY d.name, d.collation_name \
             ORDER BY d.name",
        )?;
        let names: Vec<String> = rows.iter().map(|row| text(row, 0)).collect();
        let tables = self.table_counts(&names);

        // As on the MySQL side: a project folder on this machine says
        // nothing about a schema on somebody else's server.
        let projects: Vec<(String, String)> = if self.is_remote() {
            Vec::new()
        } else {
            scan_projects()
                .map(|found| found.into_iter().map(|p| (p.id, p.domain)).collect())
                .unwrap_or_default()
        };

        Ok(rows
            .iter()
            .map(|row| {
                let name = text(row, 0);
                DatabaseInfo {
                    used_by: database::used_by(&name, &projects),
                    table_count: tables.get(&name).copied().unwrap_or(0),
                    collation: text(row, 1),
                    size_bytes: number(row, 2),
                    name,
                }
            })
            .collect())
    }

    /// The server's default collation first, then whichever of
    /// [`OFFERED_COLLATIONS`] it has.
    fn list_collations(&self) -> Result<Vec<String>, AppError> {
        let wanted = OFFERED_COLLATIONS
            .iter()
            .map(|name| nstring(name))
            .collect::<Vec<_>>()
            .join(", ");
        let rows = self.query(
            "master",
            &format!(
                "SET NOCOUNT ON; \
                 SELECT CAST(SERVERPROPERTY('Collation') AS nvarchar(128)), 0 \
                 UNION ALL \
                 SELECT name, 1 FROM sys.fn_helpcollations() WHERE name IN ({wanted}) \
                 ORDER BY 2, 1"
            ),
        )?;
        let mut collations: Vec<String> = Vec::new();
        for row in rows {
            let name = text(&row, 0);
            if !name.is_empty() && !collations.contains(&name) {
                collations.push(name);
            }
        }
        Ok(collations)
    }

    fn create_database(&self, name: &str, collation: &str) -> Result<(), AppError> {
        self.check_writable()?;
        let placement = self.file_placement(name)?;
        self.query(
            "master",
            &format!(
                "CREATE DATABASE {}{placement} COLLATE {collation}",
                bracket(name)
            ),
        )
        .map(|_| ())
    }

    fn drop_database(&self, name: &str) -> Result<(), AppError> {
        if SYSTEM_DATABASES.contains(&name.to_lowercase().as_str()) {
            return Err(AppError::DatabaseQueryFailed(format!(
                "`{name}` is one of SQL Server's own databases and can't be dropped"
            )));
        }
        self.check_writable()?;
        // A PHP worker still holding a connection makes a plain DROP fail
        // with "currently in use". On Rezure's own instance those sessions
        // are this developer's own and get closed, as SSMS's "Close existing
        // connections" would; on anyone else's server they may be someone
        // else's, so the server's refusal stands.
        let sql = if self.is_managed() {
            format!(
                "ALTER DATABASE {name} SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE {name}",
                name = bracket(name)
            )
        } else {
            format!("DROP DATABASE {}", bracket(name))
        };
        self.query("master", &sql).map(|_| ())
    }

    /// Backs `name` up to a timestamped `.bak` in the dumps folder.
    ///
    /// Only for Rezure's own LocalDB: `BACKUP … TO DISK` writes wherever the
    /// *server* is, which for any other SQL Server is not this machine.
    /// `COPY_ONLY`, so taking one never disturbs a backup chain somebody
    /// else relies on.
    fn export_database(&self, app: Option<&AppHandle>, name: &str) -> Result<PathBuf, AppError> {
        if !self.is_managed() {
            return Err(AppError::UnsupportedOnSqlServer(
                "exporting from a SQL Server Rezure doesn't run isn't supported — BACKUP writes \
                 the .bak onto that server's own disk, not this machine's"
                    .to_string(),
            ));
        }
        let dir = database::dumps_dir()?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
        let dest = dir.join(format!("{name}-{}.bak", database::timestamp()));

        let estimated_total_bytes = self
            .query(
                "master",
                &format!(
                    "SET NOCOUNT ON; SELECT COALESCE(SUM(CAST(size AS bigint)), 0) * 8192 \
                     FROM sys.master_files WHERE database_id = DB_ID({}) AND type = 0",
                    nstring(name)
                ),
            )
            .ok()
            .and_then(|rows| rows.first().map(|row| number(row, 0)))
            .filter(|bytes| *bytes > 0);

        let sql = format!(
            "BACKUP DATABASE {} TO DISK = {} WITH COPY_ONLY, INIT, FORMAT, CHECKSUM",
            bracket(name),
            nstring(&dest.display().to_string())
        );

        // The backup blocks this thread until it finishes; the file it
        // writes is watched from another, the way the MySQL export watches
        // `mysqldump`'s output.
        let done = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            if let Some(app) = app {
                scope.spawn(|| {
                    while !done.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(400));
                        let bytes_written = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                        database::emit_export_progress(
                            app,
                            &ExportProgress {
                                name: name.to_string(),
                                bytes_written,
                                estimated_total_bytes,
                            },
                        );
                    }
                });
            }
            let result = self.query("master", &sql);
            done.store(true, Ordering::SeqCst);
            result
        });

        if let Err(err) = result {
            let _ = std::fs::remove_file(&dest);
            return Err(err);
        }
        Ok(dest)
    }

    /// Loads `file` into `name`: a `.bak` is restored, a `.sql` script run.
    fn import(&self, name: &str, file: &Path) -> Result<(), AppError> {
        self.check_writable()?;
        let extension = file
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();
        match extension.as_str() {
            "bak" => self.restore(name, file),
            "sql" => self.run_script(name, file),
            _ => Err(AppError::UnsupportedOnSqlServer(format!(
                "{} isn't a .sql script or a .bak backup",
                file.display()
            ))),
        }
    }
}

impl Client {
    /// User tables per database, in one round trip.
    ///
    /// Best-effort: a database that goes offline between the two queries
    /// fails the whole batch, and that should cost the counts, not the list.
    fn table_counts(&self, names: &[String]) -> std::collections::HashMap<String, u64> {
        if names.is_empty() {
            return Default::default();
        }
        let union = names
            .iter()
            .map(|name| {
                format!(
                    "SELECT {}, COUNT(*) FROM {}.sys.tables WHERE is_ms_shipped = 0",
                    nstring(name),
                    bracket(name)
                )
            })
            .collect::<Vec<_>>()
            .join(" UNION ALL ");
        match self.query("master", &format!("SET NOCOUNT ON; {union}")) {
            Ok(rows) => rows
                .iter()
                .map(|row| (text(row, 0), number(row, 1)))
                .collect(),
            Err(err) => {
                log::warn!("could not count SQL Server tables: {err}");
                Default::default()
            }
        }
    }

    /// `ON (…) LOG ON (…)` placing a new database's files under
    /// `data\mssql` — see `mssql_localdb::data_dir`. Empty for any server
    /// but Rezure's own, whose disk isn't this machine's.
    fn file_placement(&self, name: &str) -> Result<String, AppError> {
        if !self.is_managed() {
            return Ok(String::new());
        }
        let dir = mssql_localdb::data_dir()?;
        Ok(format!(
            " ON (NAME = {data_name}, FILENAME = {data_file}) \
             LOG ON (NAME = {log_name}, FILENAME = {log_file})",
            data_name = nstring(name),
            data_file = nstring(&dir.join(format!("{name}.mdf")).display().to_string()),
            log_name = nstring(&format!("{name}_log")),
            log_file = nstring(&dir.join(format!("{name}_log.ldf")).display().to_string()),
        ))
    }

    /// Restores a `.bak` as `name`, replacing it if it exists.
    ///
    /// The backup names the files it was taken from — the original
    /// database's, on the original machine — so every one of them is
    /// `MOVE`d under `data\mssql`, named after the database it is restored
    /// as. `REPLACE` is what lets a backup of `shop` land as `shop_copy`,
    /// and what overwrites an existing `name`: the dialog that leads here
    /// says so before the user confirms.
    fn restore(&self, name: &str, file: &Path) -> Result<(), AppError> {
        if !self.is_managed() {
            return Err(AppError::UnsupportedOnSqlServer(
                "restoring a .bak onto a SQL Server Rezure doesn't run isn't supported — the \
                 server reads the file from its own disk, where this one isn't"
                    .to_string(),
            ));
        }
        let source = nstring(&file.display().to_string());
        let files = self.query(
            "master",
            &format!("SET NOCOUNT ON; RESTORE FILELISTONLY FROM DISK = {source}"),
        )?;
        let dir = mssql_localdb::data_dir()?;
        let mut moves = Vec::new();
        let (mut data_files, mut log_files) = (0, 0);
        for row in &files {
            let logical = text(row, 0);
            // Column 3 is the file's type: D for data, L for log.
            let target = if text(row, 2).eq_ignore_ascii_case("L") {
                log_files += 1;
                match log_files {
                    1 => format!("{name}_log.ldf"),
                    n => format!("{name}_log{n}.ldf"),
                }
            } else {
                data_files += 1;
                match data_files {
                    1 => format!("{name}.mdf"),
                    n => format!("{name}_{n}.ndf"),
                }
            };
            moves.push(format!(
                "MOVE {} TO {}",
                nstring(&logical),
                nstring(&dir.join(target).display().to_string())
            ));
        }

        // Exclusive access, or the restore stops at "the database is in
        // use" — the same reasoning as `drop_database`.
        let mut sql = format!(
            "IF DB_ID({literal}) IS NOT NULL ALTER DATABASE {name} SET SINGLE_USER WITH ROLLBACK IMMEDIATE; \
             RESTORE DATABASE {name} FROM DISK = {source} WITH REPLACE, RECOVERY",
            literal = nstring(name),
            name = bracket(name),
        );
        for clause in moves {
            sql.push_str(", ");
            sql.push_str(&clause);
        }
        sql.push_str(&format!(
            "; ALTER DATABASE {} SET MULTI_USER",
            bracket(name)
        ));
        self.query("master", &sql).map(|_| ())
    }

    /// Runs a `.sql` script into `name`, creating the database first if it
    /// isn't there — the same convenience the MySQL import gives.
    fn run_script(&self, name: &str, file: &Path) -> Result<(), AppError> {
        let bytes = std::fs::read(file)
            .map_err(|e| AppError::Io(format!("could not read {}: {e}", file.display())))?;
        let script = decode_script(&bytes);

        let placement = self.file_placement(name)?;
        self.query(
            "master",
            &format!(
                "IF DB_ID({}) IS NULL CREATE DATABASE {}{placement}",
                nstring(name),
                bracket(name)
            ),
        )?;

        let conn = self.connect(name)?;
        for (index, batch) in split_batches(&script).iter().enumerate() {
            run(&conn, batch).map_err(|e| match self.error(e) {
                AppError::DatabaseQueryFailed(message) => {
                    AppError::DatabaseQueryFailed(format!("batch {}: {message}", index + 1))
                }
                other => other,
            })?;
        }
        Ok(())
    }
}

/// A script's text, from whichever encoding it was saved in.
///
/// SSMS's "Generate Scripts" writes UTF-16 with a byte-order mark by
/// default; everything else writes UTF-8. Reading the former as the latter
/// turns every statement into garbage the server rejects on line one.
fn decode_script(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Splits a script on `GO` lines.
///
/// `GO` isn't T-SQL — it's the batch separator `sqlcmd` and SSMS act on
/// before anything reaches the server, which would reject it as a syntax
/// error. Statements like `CREATE PROCEDURE` must be the first in their
/// batch, so the split has to happen exactly where the script's author put
/// it. `GO 5` repeats its batch, as both tools do.
fn split_batches(script: &str) -> Vec<String> {
    let mut batches = Vec::new();
    let mut current = String::new();
    for line in script.lines() {
        let trimmed = line.trim();
        let mut words = trimmed.split_whitespace();
        let is_go = words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("GO"));
        if is_go {
            let count: Option<usize> = match words.next() {
                None => Some(1),
                Some(n) if words.next().is_none() => n.parse().ok(),
                Some(_) => None,
            };
            if let Some(count) = count {
                if !current.trim().is_empty() {
                    for _ in 0..count.max(1) {
                        batches.push(current.clone());
                    }
                }
                current.clear();
                continue;
            }
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        batches.push(current);
    }
    batches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(auth: Auth, tls: TlsMode, trust: bool) -> Client {
        Client {
            driver: OdbcDriver {
                name: "ODBC Driver 18 for SQL Server".to_string(),
                major: 18,
            },
            server: server_address("db.example.com", 1433),
            host: "db.example.com".to_string(),
            port: 1433,
            auth,
            tls,
            trust_server_certificate: trust,
            connection: None,
            _tunnel: None,
        }
    }

    #[test]
    fn a_named_instance_goes_in_as_typed_and_a_port_after_a_comma() {
        assert_eq!(
            server_address(r"office\SQLEXPRESS", 0),
            r"office\SQLEXPRESS"
        );
        assert_eq!(
            server_address("db.example.com", 1433),
            "db.example.com,1433"
        );
    }

    /// A password with `;` would otherwise end the value early and send the
    /// rest as a separate (unknown) attribute.
    #[test]
    fn values_that_would_break_the_connection_string_are_braced() {
        assert_eq!(odbc_value("plain"), "plain");
        assert_eq!(odbc_value("pa;ss"), "{pa;ss}");
        assert_eq!(odbc_value("a}b;c"), "{a}}b;c}");
        assert_eq!(odbc_value(" padded"), "{ padded}");
    }

    #[test]
    fn windows_authentication_carries_no_user_or_password() {
        let cs = client(Auth::Windows, TlsMode::Preferred, false).connection_string("master");
        assert!(cs.contains("Trusted_Connection=yes;"), "{cs}");
        assert!(!cs.contains("UID=") && !cs.contains("PWD="), "{cs}");
        assert!(
            cs.starts_with("Driver={ODBC Driver 18 for SQL Server};"),
            "{cs}"
        );
    }

    #[test]
    fn automatic_encryption_leaves_the_drivers_default_alone() {
        let auth = Auth::Sql {
            user: "sa".to_string(),
            password: Some("x".to_string()),
        };
        let cs = client(auth.clone(), TlsMode::Preferred, false).connection_string("master");
        assert!(!cs.contains("Encrypt="), "{cs}");
        assert!(!cs.contains("TrustServerCertificate"), "{cs}");

        let cs = client(auth.clone(), TlsMode::Disabled, false).connection_string("master");
        assert!(cs.contains("Encrypt=no;"), "{cs}");
        let cs = client(auth, TlsMode::Required, true).connection_string("master");
        assert!(cs.contains("Encrypt=yes;") && cs.contains("TrustServerCertificate=yes;"));
    }

    #[test]
    fn names_and_paths_are_quoted_the_way_t_sql_escapes_them() {
        assert_eq!(bracket("shop"), "[shop]");
        assert_eq!(bracket("a]b"), "[a]]b]");
        assert_eq!(
            nstring(r"C:\Users\O'Brien\x.bak"),
            r"N'C:\Users\O''Brien\x.bak'"
        );
    }

    #[test]
    fn the_driver_prefix_is_dropped_from_messages() {
        assert_eq!(
            clean_message(
                "[Microsoft][ODBC Driver 18 for SQL Server][SQL Server]Database 'shop' already exists."
            ),
            "Database 'shop' already exists."
        );
    }

    /// The error a self-signed server gives ODBC Driver 18: the fix is a
    /// setting, and the message alone doesn't say which.
    #[test]
    fn a_certificate_failure_names_the_setting_that_fixes_it() {
        let hint = hint_for(
            "SSL Provider: The certificate chain was issued by an authority that is not trusted.",
        )
        .unwrap();
        assert!(hint.contains("Trust server certificate"), "{hint}");
        assert!(hint.contains("DB_TRUST_SERVER_CERTIFICATE"), "{hint}");
        assert!(hint_for("Invalid object name 'users'.").is_none());
    }

    #[test]
    fn scripts_are_split_on_go_lines_only() {
        let script = "CREATE TABLE t (id int)\nGO\nCREATE PROCEDURE p AS SELECT 1\ngo\n\
                      INSERT INTO t VALUES (1) -- not a GO line\nGO 2\nSELECT 'GOOD'\n";
        let batches = split_batches(script);
        assert_eq!(batches.len(), 5, "{batches:?}");
        assert!(batches[1].starts_with("CREATE PROCEDURE"));
        assert_eq!(batches[2], batches[3], "GO 2 repeats its batch");
        assert!(batches[4].contains("'GOOD'"));
    }

    #[test]
    fn a_utf16_script_from_ssms_reads_as_text() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "SELECT N'é'".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_script(&bytes), "SELECT N'é'");
        assert_eq!(decode_script(b"\xEF\xBB\xBFSELECT 1"), "SELECT 1");
    }

    #[test]
    fn a_system_database_is_refused_before_anything_connects() {
        let client = client(Auth::Windows, TlsMode::Preferred, false);
        for name in ["master", "TempDB", "model", "msdb"] {
            let err = client.drop_database(name).unwrap_err();
            assert!(err.to_string().contains("own databases"), "{err}");
        }
    }

    fn localdb_connection() -> Connection {
        Connection {
            id: "selftest-localdb".to_string(),
            name: mssql_localdb::SERVICE_NAME.to_string(),
            host: mssql_localdb::SERVER_ADDRESS.to_string(),
            port: 0,
            user: String::new(),
            kind: ServerKind::Sqlserver,
            engine: crate::services::db_engine::Engine::MariaDb,
            windows_auth: true,
            trust_server_certificate: false,
            managed: true,
            tls_mode: TlsMode::Preferred,
            read_only: false,
            save_password: false,
            ssh: None,
            last_used_at: None,
        }
    }

    /// Whether ODBC Driver 18 reaches LocalDB with its defaults, or needs
    /// the certificate trusted — decides what the requirements check tells a
    /// Laravel project pointed at `(localdb)\Rezure`. Needs LocalDB and the
    /// driver installed. Run with:
    /// `cargo test --lib services::mssql::tests::print_localdb_encryption_behaviour -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_localdb_encryption_behaviour() {
        mssql_localdb::ensure_running().unwrap();
        for trust in [false, true] {
            let result = probe(Probe {
                host: mssql_localdb::SERVER_ADDRESS,
                port: 0,
                user: "",
                password: None,
                windows_auth: true,
                tls: TlsMode::Preferred,
                trust_server_certificate: trust,
                ssh: None,
                ssh_password: None,
            });
            println!("trust={trust}: {result:?}");
        }
    }

    /// Everything the Databases page does, against Rezure's real LocalDB
    /// instance: create, a `GO`-separated script, list, a `.bak` export, a
    /// restore under another name, an error from a later statement, drop.
    /// Needs LocalDB and the ODBC Driver installed. Run with:
    /// `cargo test --lib services::mssql::tests::round_trip_against_localdb -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn round_trip_against_localdb() {
        let client = Client::for_connection(localdb_connection()).unwrap();
        let (source, copy) = ("rezure_selftest_mssql", "rezure_selftest_mssql_copy");
        let _ = client.drop_database(source);
        let _ = client.drop_database(copy);

        let collations = client.list_collations().unwrap();
        println!("collations offered: {collations:?}");
        client.create_database(source, &collations[0]).unwrap();

        let dir = std::env::temp_dir();
        let script = dir.join("rezure-selftest.sql");
        std::fs::write(
            &script,
            "CREATE TABLE t (id int PRIMARY KEY, label nvarchar(20))\nGO\n\
             CREATE PROCEDURE p AS SELECT COUNT(*) FROM t\nGO\n\
             INSERT INTO t VALUES (1, N'caf\u{e9}')\nGO\n",
        )
        .unwrap();
        client.import(source, &script).unwrap();

        let listed = client.list_databases().unwrap();
        let created = listed.iter().find(|db| db.name == source).unwrap();
        println!("{created:?}");
        assert_eq!(created.table_count, 1);
        assert!(created.size_bytes > 0);

        let bak = client.export_database(None, source).unwrap();
        assert!(bak.is_file());
        println!("backed up to {}", bak.display());

        client.import(copy, &bak).unwrap();
        let rows = client
            .query(copy, "SET NOCOUNT ON; SELECT label FROM t")
            .unwrap();
        assert_eq!(rows[0][0].as_deref(), Some("caf\u{e9}"), "{rows:?}");
        assert!(mssql_localdb::data_dir()
            .unwrap()
            .join(format!("{copy}.mdf"))
            .is_file());

        // An error in a later statement of a batch only arrives when the
        // results are drained — it must not be swallowed.
        let bad = dir.join("rezure-selftest-bad.sql");
        std::fs::write(&bad, "SELECT 1; SELECT * FROM no_such_table").unwrap();
        let err = client.import(source, &bad).unwrap_err().to_string();
        println!("bad script: {err}");
        assert!(err.contains("no_such_table"), "{err}");

        client.drop_database(source).unwrap();
        client.drop_database(copy).unwrap();
        assert!(!client
            .list_databases()
            .unwrap()
            .iter()
            .any(|db| db.name == source || db.name == copy));
        for file in [script, bad, bak] {
            let _ = std::fs::remove_file(file);
        }
    }

    #[test]
    fn exporting_from_a_server_elsewhere_is_refused_up_front() {
        let client = client(Auth::Windows, TlsMode::Preferred, false);
        assert!(matches!(
            client.export_database(None, "shop"),
            Err(AppError::UnsupportedOnSqlServer(_))
        ));
    }
}
