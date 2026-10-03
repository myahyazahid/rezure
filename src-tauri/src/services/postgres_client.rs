//! The Databases page's PostgreSQL client — list, create, drop, export and
//! import, against Rezure's own PostgreSQL or one elsewhere.
//!
//! # `psql` and `pg_dump`, not a driver crate
//!
//! The same choice as the MySQL side (`services::database`): every
//! PostgreSQL build Rezure installs ships its own `psql.exe` and
//! `pg_dump.exe`, so this drives those. Reads use `psql`'s unaligned,
//! tuples-only output (`-A -t`), tab-separated.
//!
//! # Text goes through stdin and stdout, never argv
//!
//! Windows hands a console program its arguments in the ANSI code page, so
//! an `é` passed in `psql -c` reached the server as the CP1252 byte `0xE9`
//! and was refused as invalid UTF-8. Statements and imported scripts are fed
//! to `psql` on stdin, and `pg_dump` writes to stdout into a file Rezure
//! opened — the bytes stay UTF-8 the whole way, and no path is handed to a
//! tool to re-encode either.
//!
//! # Passwords
//!
//! Never in argv, where any process listing shows it, and not in
//! `PGPASSWORD` either, which the environment of a running process exposes
//! the same way: a temporary password file (`PGPASSFILE`) that is deleted
//! when the client is dropped — the counterpart of the MySQL side's
//! `--defaults-file`. Every run also passes `-w`, so a server that wants a
//! password Rezure doesn't have says so instead of waiting on a prompt
//! nobody can see.
//!
//! # Names
//!
//! Database names arrive through `database::validate_identifier`, as on the
//! other two kinds of server, and are double-quoted in statements on top of
//! that. They're handed to `psql` and `pg_dump` through `PGDATABASE`, never
//! as an argument: both read an argument containing `=` as a whole
//! connection string.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::AppHandle;

use super::database::{self, DatabaseInfo, DbClient, ExportProgress, ServerInfo};
use super::projects::scan_projects;
use super::{connections, postgres, secrets, tunnel};
use crate::config::connections::{Connection, ServerKind, SshTunnel, TlsMode};
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

/// How long a connect may take — the same ten seconds the other two clients
/// get.
const CONNECT_TIMEOUT_SECS: u32 = 10;

/// The databases every PostgreSQL server has — never listed, never
/// droppable. `postgres` is the maintenance database, the counterpart of SQL
/// Server's `master`.
const SYSTEM_DATABASES: [&str; 3] = ["postgres", "template0", "template1"];

/// The encoding a new database is created in. A short list on purpose, like
/// SQL Server's collations: the choice a developer actually faces is UTF-8.
const OFFERED_ENCODINGS: [&str; 1] = ["UTF8"];

/// `"name"`, with any `"` doubled — PostgreSQL's own escape.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// `'text'`, with any `'` doubled.
fn quote_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The libpq `sslmode` for a connection's TLS setting.
fn sslmode(tls: TlsMode) -> &'static str {
    match tls {
        TlsMode::Disabled => "disable",
        TlsMode::Preferred => "prefer",
        TlsMode::Required => "require",
    }
}

/// A temporary `PGPASSFILE` holding one password, deleted when dropped —
/// see the module docs.
struct TempPassfile {
    path: PathBuf,
}

impl TempPassfile {
    fn new(password: &str) -> Result<Self, AppError> {
        let path = std::env::temp_dir().join(format!(
            "rezure-{}-{}.pgpass",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, passfile_line(password)).map_err(|e| {
            AppError::Io(format!("could not write the temporary password file: {e}"))
        })?;
        Ok(Self { path })
    }
}

impl Drop for TempPassfile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One `.pgpass` line matching any host, port, database and user. `:` and
/// `\` are the file's own separators, escaped with a backslash.
fn passfile_line(password: &str) -> String {
    let escaped = password.replace('\\', "\\\\").replace(':', "\\:");
    format!("*:*:*:*:{escaped}\n")
}

/// Everything needed to run `psql` or `pg_dump` against one server.
pub struct Client {
    /// The folder holding `psql.exe` and `pg_dump.exe`.
    bin: PathBuf,
    host: String,
    port: u16,
    user: String,
    tls: TlsMode,
    passfile: Option<TempPassfile>,
    /// The saved connection this is; `None` for a probe of an unsaved form.
    connection: Option<Connection>,
    /// Kept alive while it's in use: dropping it closes the SSH forward a
    /// probe opened.
    _tunnel: Option<tunnel::Tunnel>,
}

impl Client {
    /// The client for a saved connection.
    pub fn for_connection(connection: Connection) -> Result<Self, AppError> {
        // Rezure's own server is queried with its own build; anyone else's
        // with the newest one installed, whose `pg_dump` can read the most
        // server versions (it refuses a server newer than itself).
        let bin = if connection.managed {
            postgres::active_tool(postgres::CLIENT_EXE)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| AppError::Io("psql.exe has no parent directory".to_string()))?
        } else {
            postgres::client_bin_dir()?
        };
        let (host, port) = match &connection.ssh {
            Some(_) => (database::HOST.to_string(), tunnel::ensure(&connection)?),
            None => (connection.host.clone(), connection.port),
        };
        let passfile = if connections::needs_no_password(&connection) {
            None
        } else {
            secrets::resolve(&connection.id)
                .map(|password| TempPassfile::new(&password))
                .transpose()?
        };
        Ok(Self {
            bin,
            host,
            port,
            user: connection.user.clone(),
            tls: connection.tls_mode,
            passfile,
            connection: Some(connection),
            _tunnel: None,
        })
    }

    /// Whether this is Rezure's own server — writable, "used by" shown.
    fn is_managed(&self) -> bool {
        self.connection.as_ref().is_some_and(|c| c.managed)
    }

    fn check_writable(&self) -> Result<(), AppError> {
        match &self.connection {
            Some(connection) if connection.read_only => Err(AppError::ConnectionReadOnly {
                name: connection.name.clone(),
            }),
            _ => Ok(()),
        }
    }

    /// One of this build's tools, with the connection in its environment.
    fn command(&self, tool: &str, database: &str) -> Result<Command, AppError> {
        let exe = self.bin.join(tool);
        if !exe.is_file() {
            return Err(AppError::BinaryNotInstalled(format!("PostgreSQL's {tool}")));
        }
        let mut cmd = Command::new(exe);
        cmd.env("PGHOST", &self.host)
            .env("PGPORT", self.port.to_string())
            .env("PGUSER", &self.user)
            .env("PGDATABASE", database)
            .env("PGSSLMODE", sslmode(self.tls))
            .env("PGCONNECT_TIMEOUT", CONNECT_TIMEOUT_SECS.to_string())
            .env("PGAPPNAME", "Rezure")
            .env("PGCLIENTENCODING", "UTF8")
            // A variable left in the user's environment would otherwise
            // override what was asked for here.
            .env_remove("PGPASSWORD")
            .env_remove("PGSERVICE")
            .env_remove("PGOPTIONS");
        match &self.passfile {
            Some(passfile) => cmd.env("PGPASSFILE", &passfile.path),
            None => cmd.env_remove("PGPASSFILE"),
        };
        cmd.stdin(Stdio::null()).hidden();
        Ok(cmd)
    }

    /// Runs `sql` in `database` and returns its rows. Fed on stdin — see the
    /// module docs.
    fn query(&self, database: &str, sql: &str) -> Result<Vec<Vec<String>>, AppError> {
        let mut child = self
            .command(postgres::CLIENT_EXE, database)?
            .args(["-X", "-w", "-q", "-A", "-t", "-F", "\t"])
            .args(["-v", "ON_ERROR_STOP=1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            // Ignored: `psql` exits without reading when it can't connect,
            // and what it printed on the way out is the error worth showing.
            let _ = stdin.write_all(sql.as_bytes());
            let _ = stdin.write_all(b";\n");
        }
        let output = child
            .wait_with_output()
            .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(self.error(&output.stderr, "the query failed"));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.split('\t').map(str::to_string).collect())
            .collect())
    }

    /// Turns a failed run into an error worth showing — the server's own
    /// message, except for the failures it words least helpfully.
    fn error(&self, stderr: &[u8], fallback: &str) -> AppError {
        let text = String::from_utf8_lossy(stderr);
        let message = clean_message(&text).unwrap_or_else(|| fallback.to_string());
        let lower = text.to_lowercase();

        if lower.contains("connection refused") {
            // The same error shape as a remote server that's down, which is
            // what the Databases page recognises to offer "start it from
            // Services" instead of a raw message.
            if self.is_managed() {
                return self.unreachable(
                    "Rezure's PostgreSQL isn't running — start it from the Services page",
                );
            }
            return self.unreachable(
                "the connection was refused. The host is reachable but nothing is listening on \
                 that port — check the server is running, and that this is the right port",
            );
        }
        if lower.contains("timeout expired") || lower.contains("timed out") {
            return self.unreachable(
                "the connection timed out. Nothing answered, which usually means a firewall is \
                 dropping the port, or the server only listens on its own machine",
            );
        }
        if lower.contains("could not translate host name") {
            return self.unreachable("that host name doesn't resolve");
        }
        match hint_for(&lower) {
            Some(hint) => AppError::DatabaseQueryFailed(format!("{message} — {hint}")),
            None => AppError::DatabaseQueryFailed(message),
        }
    }

    fn unreachable(&self, reason: &str) -> AppError {
        AppError::ServerUnreachable {
            host: self.host.clone(),
            port: self.port,
            reason: reason.to_string(),
        }
    }

    /// Whether `name` exists.
    fn exists(&self, name: &str) -> Result<bool, AppError> {
        let rows = self.query(
            postgres::MAINTENANCE_DB,
            &format!(
                "SELECT 1 FROM pg_database WHERE datname = {}",
                quote_literal(name)
            ),
        )?;
        Ok(!rows.is_empty())
    }

    /// User tables per database. PostgreSQL can't count across databases in
    /// one query, so each is asked on its own, side by side.
    ///
    /// Best-effort: a database that can't be read costs its count, not the
    /// list.
    fn table_counts(&self, names: &[String]) -> std::collections::HashMap<String, u64> {
        let count = |name: &String| {
            self.query(
                name,
                "SELECT count(*) FROM pg_catalog.pg_tables \
                 WHERE schemaname NOT IN ('pg_catalog', 'information_schema')",
            )
            .ok()
            .and_then(|rows| rows.first().and_then(|row| row.first()).cloned())
            .and_then(|value| value.trim().parse().ok())
        };
        std::thread::scope(|scope| {
            let handles: Vec<_> = names
                .iter()
                .map(|name| (name, scope.spawn(move || count(name))))
                .collect();
            handles
                .into_iter()
                .filter_map(|(name, handle)| {
                    handle
                        .join()
                        .ok()
                        .flatten()
                        .map(|count| (name.clone(), count))
                })
                .collect()
        })
    }
}

/// The first line of what `psql` printed, without the `psql: error:` and
/// `ERROR:` prefixes that say nothing the user doesn't already know. A
/// failure in a script keeps its line number.
fn clean_message(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut rest = line;
    // `psql:<stdin>:12: ERROR:  …` — where in an imported script it failed.
    let mut script_line = None;
    if let Some((number, message)) = rest
        .strip_prefix("psql:<stdin>:")
        .and_then(|after| after.split_once(": "))
    {
        if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
            script_line = Some(number);
            rest = message;
        }
    }
    for prefix in ["psql: error:", "pg_dump: error:", "ERROR:", "FATAL:"] {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped.trim_start();
        }
    }
    // `connection to server at "host" (addr), port 5432 failed: FATAL:  …`
    // — the part after the last `failed:` is the reason.
    if let Some((_, reason)) = rest.rsplit_once("failed: ") {
        rest = reason.trim_start_matches("FATAL:").trim();
    }
    Some(match script_line {
        Some(number) => format!("line {number}: {rest}"),
        None => rest.to_string(),
    })
}

/// Errors whose wording is right but doesn't say what to do — the
/// PostgreSQL counterpart of `database::hint_for`.
fn hint_for(lower: &str) -> Option<&'static str> {
    if lower.contains("no password supplied") {
        return Some(
            "this server wants a password and none is saved for the connection — unlock it \
             from the connection switcher",
        );
    }
    if lower.contains("no pg_hba.conf entry") {
        return Some(
            "the server's pg_hba.conf doesn't let this machine in. Add a line for your address \
             (or use an SSH tunnel), then reload the server",
        );
    }
    if lower.contains("server version mismatch") {
        return Some(
            "pg_dump can't dump a server newer than itself — install that PostgreSQL version \
             (or a newer one) from the Switch page",
        );
    }
    None
}

/// The fields of an unsaved connection, for "Test connection".
pub struct Probe<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub password: Option<&'a str>,
    pub tls: TlsMode,
    pub ssh: Option<&'a SshTunnel>,
    pub ssh_password: Option<&'a str>,
}

/// Connects with a form's values and reports the server's version, e.g.
/// `PostgreSQL 16.4`.
pub fn probe(request: Probe<'_>) -> Result<String, AppError> {
    let bin = postgres::client_bin_dir()?;
    let host = request.host.trim();
    let opened = match request.ssh {
        Some(ssh) => Some(tunnel::open(ssh, request.ssh_password, host, request.port)?),
        None => None,
    };
    let (host, port) = match &opened {
        Some(tunnel) => (database::HOST.to_string(), tunnel.local_port),
        None => (host.to_string(), request.port),
    };
    let client = Client {
        bin,
        host,
        port,
        user: request.user.trim().to_string(),
        tls: request.tls,
        passfile: match request.password {
            Some(password) if !password.is_empty() => Some(TempPassfile::new(password)?),
            _ => None,
        },
        connection: None,
        _tunnel: opened,
    };
    let rows = client.query(postgres::MAINTENANCE_DB, "SELECT version()")?;
    let version = rows
        .first()
        .and_then(|row| row.first())
        .cloned()
        .unwrap_or_default();
    // `PostgreSQL 16.4 on x86_64-pc-linux-gnu, compiled by gcc …` — the
    // part before ` on ` is the answer.
    Ok(version
        .split(" on ")
        .next()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("connected")
        .to_string())
}

/// Where a SQL client on this machine connects to reach `connection`: host,
/// port and user. A tunnelled connection's own host only means something on
/// the SSH server, so it's the live forward's port, when there is one.
pub fn endpoint(connection: &Connection) -> (String, u16, String) {
    match connection
        .ssh
        .as_ref()
        .and_then(|_| tunnel::existing_port(&connection.id))
    {
        Some(local) => (database::HOST.to_string(), local, connection.user.clone()),
        None => (
            connection.host.clone(),
            connection.port,
            connection.user.clone(),
        ),
    }
}

/// What the Databases page shows about a PostgreSQL connection.
pub fn server_info(connection: &Connection) -> ServerInfo {
    ServerInfo {
        host: connection.host.clone(),
        port: connection.port,
        user: connection.user.clone(),
        has_password: !connections::needs_no_password(connection)
            && secrets::resolve(&connection.id).is_some(),
        // The URL form both libpq and Laravel's `DB_URL` take.
        dsn: format!(
            "postgresql://{}@{}:{}",
            connection.user, connection.host, connection.port
        ),
        remote: !connection.managed,
        label: connection.name.clone(),
        read_only: connection.read_only,
        kind: ServerKind::Postgres,
        // `pg_dump` runs here and connects out, so unlike SQL Server's
        // BACKUP it works against any server.
        export_supported: true,
        import_extensions: vec!["sql".to_string()],
    }
}

impl DbClient for Client {
    fn list_databases(&self) -> Result<Vec<DatabaseInfo>, AppError> {
        let excluded = SYSTEM_DATABASES
            .iter()
            .map(|name| quote_literal(name))
            .collect::<Vec<_>>()
            .join(", ");
        // `pg_database_size` needs CONNECT on the database, which the filter
        // already requires — a database this login can't open isn't listed.
        let rows = self.query(
            postgres::MAINTENANCE_DB,
            &format!(
                "SELECT datname, pg_encoding_to_char(encoding), pg_database_size(datname) \
                 FROM pg_database \
                 WHERE NOT datistemplate AND datname NOT IN ({excluded}) \
                 AND has_database_privilege(datname, 'CONNECT') \
                 ORDER BY datname"
            ),
        )?;
        let names: Vec<String> = rows.iter().filter_map(|row| row.first().cloned()).collect();
        let tables = self.table_counts(&names);

        // As on the other two kinds of server: a project folder on this
        // machine says nothing about a database on somebody else's.
        let projects: Vec<(String, String)> = if self.is_managed() {
            scan_projects()
                .map(|found| found.into_iter().map(|p| (p.id, p.domain)).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(rows
            .into_iter()
            .filter(|row| row.len() >= 3)
            .map(|row| DatabaseInfo {
                used_by: database::used_by(&row[0], &projects),
                table_count: tables.get(&row[0]).copied().unwrap_or(0),
                collation: row[1].clone(),
                size_bytes: row[2].trim().parse().unwrap_or(0),
                name: row[0].clone(),
            })
            .collect())
    }

    /// The encodings offered for a new database — PostgreSQL's counterpart
    /// of a collation, as far as the New database dialog goes.
    fn list_collations(&self) -> Result<Vec<String>, AppError> {
        Ok(OFFERED_ENCODINGS.iter().map(|e| e.to_string()).collect())
    }

    /// `encoding` takes the collation's place — see [`Self::list_collations`].
    /// From `template0`, the template a database in a different encoding
    /// than the server's default can be made from.
    fn create_database(&self, name: &str, encoding: &str) -> Result<(), AppError> {
        self.check_writable()?;
        self.query(
            postgres::MAINTENANCE_DB,
            &format!(
                "CREATE DATABASE {} ENCODING {} TEMPLATE template0",
                quote_ident(name),
                quote_literal(encoding)
            ),
        )
        .map(|_| ())
    }

    fn drop_database(&self, name: &str) -> Result<(), AppError> {
        if SYSTEM_DATABASES.contains(&name.to_lowercase().as_str()) {
            return Err(AppError::DatabaseQueryFailed(format!(
                "`{name}` is one of PostgreSQL's own databases and can't be dropped"
            )));
        }
        self.check_writable()?;
        // A PHP worker holding a pooled connection makes a plain DROP fail
        // with "being accessed by other users". On Rezure's own server those
        // sessions are this developer's and get closed — `WITH (FORCE)`,
        // PostgreSQL 13 and later; on anyone else's they may be someone
        // else's, so the server's refusal stands.
        let sql = if self.is_managed() {
            format!("DROP DATABASE {} WITH (FORCE)", quote_ident(name))
        } else {
            format!("DROP DATABASE {}", quote_ident(name))
        };
        self.query(postgres::MAINTENANCE_DB, &sql).map(|_| ())
    }

    /// Dumps `name` to a timestamped plain `.sql` file.
    ///
    /// `--no-owner --no-privileges`, so the dump imports into a database of
    /// another name, owned by another role — what Import offers — without
    /// failing on `ALTER … OWNER TO` a role the target doesn't have.
    fn export_database(&self, app: Option<&AppHandle>, name: &str) -> Result<PathBuf, AppError> {
        let dir = database::dumps_dir()?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
        // As on the MySQL side: a dump pulled from another server is prefixed
        // with the connection's name, or it's indistinguishable from a local
        // one in the folder.
        let prefix = match &self.connection {
            Some(connection) if !connection.managed => {
                format!("{}-", database::slugify(&connection.name))
            }
            _ => String::new(),
        };
        let dest = dir.join(format!("{prefix}{name}-{}.sql", database::timestamp()));

        let estimated_total_bytes = self
            .query(
                postgres::MAINTENANCE_DB,
                &format!("SELECT pg_database_size({})", quote_literal(name)),
            )
            .ok()
            .and_then(|rows| rows.first().and_then(|row| row.first()).cloned())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|bytes| *bytes > 0);

        let file = std::fs::File::create(&dest)
            .map_err(|e| AppError::Io(format!("could not create {}: {e}", dest.display())))?;
        let mut command = self.command(postgres::DUMP_EXE, name)?;
        command
            .args(["-w", "--format=plain", "--no-owner", "--no-privileges"])
            .arg("--encoding=UTF8")
            .stdout(file)
            .stderr(Stdio::piped());

        // `pg_dump` blocks this thread until it finishes; the file it writes
        // is watched from another, as the other exports do.
        let done = AtomicBool::new(false);
        let output = std::thread::scope(|scope| {
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
            let output = command.output();
            done.store(true, Ordering::SeqCst);
            output
        });

        let output = output.map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;
        if !output.status.success() {
            let _ = std::fs::remove_file(&dest);
            return Err(self.error(&output.stderr, "the export failed"));
        }
        Ok(dest)
    }

    /// Runs a `.sql` file in `name`, creating the database first if needed.
    /// Stops at the first error (`ON_ERROR_STOP`) rather than carrying on
    /// past it, so what's reported is the cause, not the hundred failures it
    /// led to. Not one transaction: like the MySQL import, a file that fails
    /// half-way leaves the first half applied.
    fn import(&self, name: &str, file: &Path) -> Result<(), AppError> {
        self.check_writable()?;
        if !file
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
        {
            return Err(AppError::DatabaseQueryFailed(format!(
                "{} isn't a .sql file — PostgreSQL imports here are plain SQL scripts",
                file.display()
            )));
        }
        if !self.exists(name)? {
            self.create_database(name, OFFERED_ENCODINGS[0])?;
        }
        let script = std::fs::File::open(file)
            .map_err(|e| AppError::Io(format!("could not read {}: {e}", file.display())))?;
        let output = self
            .command(postgres::CLIENT_EXE, name)?
            .args(["-X", "-w", "-q", "-v", "ON_ERROR_STOP=1"])
            .stdin(script)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(self.error(&output.stderr, "the import failed"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_strings_are_quoted_the_way_postgresql_escapes_them() {
        assert_eq!(quote_ident("shop"), "\"shop\"");
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(quote_literal("it's"), "'it''s'");
    }

    #[test]
    fn a_password_file_escapes_its_own_separators() {
        assert_eq!(passfile_line("plain"), "*:*:*:*:plain\n");
        assert_eq!(passfile_line(r"a:b\c"), "*:*:*:*:a\\:b\\\\c\n");
    }

    #[test]
    fn tls_settings_map_onto_sslmode() {
        assert_eq!(sslmode(TlsMode::Disabled), "disable");
        assert_eq!(sslmode(TlsMode::Preferred), "prefer");
        assert_eq!(sslmode(TlsMode::Required), "require");
    }

    #[test]
    fn a_failed_connect_keeps_only_the_reason() {
        let text = "psql: error: connection to server at \"db.example.com\" (10.0.0.5), port \
                    5432 failed: FATAL:  password authentication failed for user \"app\"\n";
        assert_eq!(
            clean_message(text).as_deref(),
            Some("password authentication failed for user \"app\"")
        );
        assert_eq!(
            clean_message("ERROR:  database \"x\" already exists\n").as_deref(),
            Some("database \"x\" already exists")
        );
        assert_eq!(clean_message("\n\n"), None);
        assert_eq!(
            clean_message("psql:<stdin>:12: ERROR:  relation \"users\" does not exist\n")
                .as_deref(),
            Some("line 12: relation \"users\" does not exist")
        );
    }

    #[test]
    fn a_missing_password_says_where_to_give_one() {
        assert!(hint_for("fe_sendauth: no password supplied").is_some());
        assert!(hint_for("relation \"users\" does not exist").is_none());
    }

    fn managed_client() -> Client {
        Client {
            bin: PathBuf::from("."),
            host: postgres::HOST.to_string(),
            port: postgres::PORT,
            user: postgres::SUPERUSER.to_string(),
            tls: TlsMode::Disabled,
            passfile: None,
            connection: Some(Connection {
                id: "pg".to_string(),
                name: postgres::SERVICE_NAME.to_string(),
                host: postgres::HOST.to_string(),
                port: postgres::PORT,
                user: postgres::SUPERUSER.to_string(),
                kind: ServerKind::Postgres,
                engine: crate::services::db_engine::Engine::MariaDb,
                windows_auth: false,
                trust_server_certificate: false,
                managed: true,
                tls_mode: TlsMode::Disabled,
                read_only: false,
                save_password: false,
                ssh: None,
                last_used_at: None,
            }),
            _tunnel: None,
        }
    }

    /// Refused before anything is run — the client above has no binaries.
    #[test]
    fn a_system_database_is_refused_before_anything_connects() {
        for name in SYSTEM_DATABASES {
            assert!(matches!(
                managed_client().drop_database(name),
                Err(AppError::DatabaseQueryFailed(_))
            ));
        }
    }

    #[test]
    fn rezures_own_server_not_running_reads_as_unreachable() {
        let err = managed_client().error(
            b"psql: error: connection to server at \"127.0.0.1\", port 5432 failed: \
              Connection refused (0x0000274D/10061)\n",
            "x",
        );
        assert!(matches!(err, AppError::ServerUnreachable { .. }), "{err}");
        assert!(err.to_string().contains("Services page"), "{err}");
    }

    /// Against the real PostgreSQL service, which has to be running:
    /// create, list, export, import into a copy, drop. Run with
    /// `cargo test --lib services::postgres_client::tests::round_trip_against_local_postgres -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn round_trip_against_local_postgres() {
        let mut client = managed_client();
        client.bin = postgres::active_tool(postgres::CLIENT_EXE)
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let name = "rezure_roundtrip";
        let copy = "rezure_roundtrip_copy";
        let _ = client.drop_database(name);
        let _ = client.drop_database(copy);

        client.create_database(name, "UTF8").unwrap();
        client
            .query(
                name,
                "CREATE TABLE notes (id serial PRIMARY KEY, body text); \
                 INSERT INTO notes (body) VALUES ('héllo'), ('wörld')",
            )
            .unwrap();
        let listed = client.list_databases().unwrap();
        let entry = listed.iter().find(|d| d.name == name).unwrap();
        println!("{entry:?}");
        assert_eq!(entry.collation, "UTF8");
        assert_eq!(entry.table_count, 1);

        let dump = client.export_database(None, name).unwrap();
        println!("dump: {}", dump.display());
        client.import(copy, &dump).unwrap();
        let rows = client
            .query(copy, "SELECT body FROM notes ORDER BY id")
            .unwrap();
        assert_eq!(
            rows,
            vec![vec!["héllo".to_string()], vec!["wörld".to_string()]]
        );

        client.drop_database(name).unwrap();
        client.drop_database(copy).unwrap();
        let _ = std::fs::remove_file(dump);
    }
}
