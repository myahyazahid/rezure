//! PostgreSQL: which versions are on disk, which one is active, where each
//! major keeps its data, and the few things about running it that differ
//! from MariaDB. Installing is [`super::postgres_catalog`]'s job; the service
//! itself is `process::Launch::Postgres`.
//!
//! # One service, an active version
//!
//! Like Node.js (`services::node`), not like the MySQL-family profiles: the
//! one `postgres` service runs whichever installed version is active, and the
//! Switch page changes which. PostgreSQL listens on its own port (5432), so it
//! runs alongside the MariaDB service rather than taking turns with it — the
//! "one database server at a time" rule in `docs/v1/database-profiles.md` is
//! about `mysqld` datadirs only.
//!
//! # A data directory per major
//!
//! A data directory can only be opened by the major version that wrote it.
//! So each major gets its own, `data\postgres\<major>\`, and switching from
//! 17 to 18 starts 18 on a fresh one rather than trying to open 17's. Moving
//! the data across (`pg_upgrade`) is out of scope; a dump and an import is
//! the way, as between any two servers.
//!
//! # Roles and authentication
//!
//! Bootstrapped with `trust` authentication, listening on `127.0.0.1` only —
//! the same stance as Rezure's MariaDB and its passwordless `root`. `initdb`
//! creates the `postgres` superuser; Rezure adds a second one, `root`,
//! because that is the username Laravel's `config/database.php` falls back to
//! for its `pgsql` connection, so a project switched to PostgreSQL connects
//! without editing `.env`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use super::binaries::{self, InstalledRuntime};
use super::{ServiceManager, ServiceStatus};
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;
use crate::utils::paths;

/// The `binaries::discover` family, and the folder installs live under.
pub const FAMILY: &str = "postgres";
pub const SERVER_EXE: &str = "postgres.exe";
/// The console client — also what the Databases page runs every query with.
pub const CLIENT_EXE: &str = "psql.exe";
pub const DUMP_EXE: &str = "pg_dump.exe";
const INITDB_EXE: &str = "initdb.exe";
const CTL_EXE: &str = "pg_ctl.exe";
const READY_EXE: &str = "pg_isready.exe";

/// The service's id — what the Services page, the logs and Manage services
/// key off.
pub const SERVICE_ID: &str = "postgres";
pub const SERVICE_NAME: &str = "PostgreSQL";

pub const HOST: &str = "127.0.0.1";
/// PostgreSQL's own default, and what Laravel's `pgsql` connection assumes.
pub const PORT: u16 = 5432;
/// The superuser `initdb` creates.
pub const SUPERUSER: &str = "postgres";
/// The second superuser Rezure adds — see the module docs.
pub const APP_ROLE: &str = "root";
/// The database every server has, and the one maintenance queries run in.
pub const MAINTENANCE_DB: &str = "postgres";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostgresVersionStatus {
    /// The version string doubles as the id — same as
    /// `services::node::NodeVersionStatus`.
    pub id: String,
    pub version: String,
    /// What decides the data directory — see [`major_of`].
    pub major: String,
    pub installed: bool,
    pub active: bool,
    /// False for versions dropped into the user's own `custom` folder.
    pub managed: bool,
    pub path: String,
}

/// Every PostgreSQL version currently on disk, newest first.
pub fn installed() -> Vec<InstalledRuntime> {
    binaries::discover(FAMILY, SERVER_EXE)
}

pub fn is_installed() -> bool {
    !installed().is_empty()
}

fn active_cell() -> &'static Mutex<String> {
    static ACTIVE: OnceLock<Mutex<String>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(String::new()))
}

/// The active version, resolved against an already-taken scan — falls back
/// to the newest installed one when the stored choice is gone, the same way
/// `services::node::active_from` does.
fn active_from(present: &[InstalledRuntime]) -> String {
    let mut cell = active_cell().lock().unwrap_or_else(|e| e.into_inner());
    if present.iter().any(|runtime| runtime.version == *cell) {
        return cell.clone();
    }
    let fallback = present
        .first()
        .map(|runtime| runtime.version.clone())
        .unwrap_or_default();
    *cell = fallback.clone();
    fallback
}

/// Empty when nothing is installed.
pub fn active_id() -> String {
    active_from(&installed())
}

/// The active version's `postgres.exe`.
pub fn active_exe() -> Result<PathBuf, AppError> {
    let present = installed();
    let active = active_from(&present);
    present
        .into_iter()
        .find(|runtime| runtime.version == active)
        .map(|runtime| runtime.exe)
        .ok_or_else(|| AppError::BinaryNotInstalled(SERVICE_NAME.to_string()))
}

/// Switches the active version. Rejects anything not on disk. With the
/// service running, [`switch`] is the one to call — it stops it first.
pub fn set_active(version: &str) -> Result<Vec<PostgresVersionStatus>, AppError> {
    if !installed().iter().any(|runtime| runtime.version == version) {
        return Err(AppError::PostgresVersionNotFound(version.to_string()));
    }
    *active_cell().lock().unwrap_or_else(|e| e.into_inner()) = version.to_string();
    Ok(list())
}

/// Switches the active version and moves a running server along with it:
/// stopped on the version it's running — the only one whose data directory
/// it can be asked to shut down cleanly in — then started on the new one.
/// Returns whether it was restarted.
pub fn switch(manager: &ServiceManager, version: &str) -> Result<bool, AppError> {
    if !installed().iter().any(|runtime| runtime.version == version) {
        return Err(AppError::PostgresVersionNotFound(version.to_string()));
    }
    if active_id() == version {
        return Ok(false);
    }
    let service = manager.find(SERVICE_ID)?;
    let running = service.info().status != ServiceStatus::Stopped;
    if running {
        service.stop()?;
    }
    set_active(version)?;
    if running {
        service.start()?;
    }
    Ok(running)
}

pub fn list() -> Vec<PostgresVersionStatus> {
    let present = installed();
    let active = active_from(&present);
    present
        .into_iter()
        .map(|runtime| PostgresVersionStatus {
            active: runtime.version == active,
            major: major_of(&runtime.version),
            id: runtime.version.clone(),
            version: runtime.version,
            installed: true,
            managed: runtime.managed,
            path: runtime.dir.display().to_string(),
        })
        .collect()
}

/// The major version — `18` for `18.6`. Since PostgreSQL 10 the major is the
/// first number; before that it was the first two (`9.6.24` → `9.6`), which a
/// hand-dropped old build still reads correctly as.
pub fn major_of(version: &str) -> String {
    let mut parts = version.split('.');
    let first = parts.next().unwrap_or_default();
    match (first.parse::<u32>(), parts.next()) {
        (Ok(major), Some(minor)) if major < 10 => format!("{major}.{minor}"),
        _ => first.to_string(),
    }
}

/// `data\postgres\<major>` — where `version` keeps its databases.
pub fn data_dir(version: &str) -> Result<PathBuf, AppError> {
    Ok(paths::data()?.join(FAMILY).join(major_of(version)))
}

/// The folder holding `postgres.exe`, and every other binary of that build.
fn bin_dir(server_exe: &Path) -> Result<&Path, AppError> {
    server_exe
        .parent()
        .ok_or_else(|| AppError::Io("postgres.exe has no parent directory".to_string()))
}

/// A binary from the same build as the active server, for clients that run
/// beside it — `psql`, `pg_dump`.
pub fn active_tool(name: &str) -> Result<PathBuf, AppError> {
    let exe = bin_dir(&active_exe()?)?.join(name);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(AppError::BinaryNotInstalled(format!("PostgreSQL's {name}")))
    }
}

/// The folder of the newest installed build that has `psql.exe` — what a
/// connection to a PostgreSQL server Rezure doesn't run is queried with. The
/// newest, not the active one: `pg_dump` refuses a server newer than itself,
/// while a newer `pg_dump` reads every older server.
pub fn client_bin_dir() -> Result<PathBuf, AppError> {
    installed()
        .iter()
        .filter_map(|runtime| runtime.exe.parent())
        .find(|dir| dir.join(CLIENT_EXE).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::BinaryNotInstalled(format!("PostgreSQL's {CLIENT_EXE}")))
}

/// The last thing a failed tool said, for the error that reports it.
fn last_words(output: &std::process::Output) -> String {
    let last = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map(str::to_string)
    };
    last(&output.stderr)
        .or_else(|| last(&output.stdout))
        .unwrap_or_else(|| format!("exited with {}", output.status))
}

/// Creates an empty data directory's cluster: `initdb`, then the `root`
/// superuser. All or nothing — a failure removes what was made, so the next
/// start tries again instead of opening half a cluster.
///
/// `-E UTF8 --no-locale` is not optional: without it `initdb` takes the
/// Windows locale (`Indonesian_Indonesia.1252`, say), and every database is
/// created in WIN1252 instead of UTF-8.
pub fn bootstrap(server_exe: &Path, data_dir: &Path) -> Result<(), AppError> {
    let failed = |reason: String| {
        let _ = std::fs::remove_dir_all(data_dir);
        AppError::ProcessBootstrapFailed {
            name: SERVICE_NAME.to_string(),
            reason,
        }
    };
    let bin = bin_dir(server_exe)?;
    std::fs::create_dir_all(data_dir)
        .map_err(|e| failed(format!("could not create {}: {e}", data_dir.display())))?;

    let output = Command::new(bin.join(INITDB_EXE))
        .arg("-D")
        .arg(data_dir)
        .args(["-U", SUPERUSER, "-A", "trust", "-E", "UTF8", "--no-locale"])
        .arg("--no-instructions")
        .stdin(Stdio::null())
        .hidden()
        .output()
        .map_err(|e| failed(format!("could not run initdb: {e}")))?;
    if !output.status.success() {
        return Err(failed(format!("initdb: {}", last_words(&output))));
    }

    create_app_role(server_exe, data_dir).map_err(failed)
}

/// Adds the `root` superuser through single-user mode — no server to start,
/// no port to take, before anything else can connect.
fn create_app_role(server_exe: &Path, data_dir: &Path) -> Result<(), String> {
    let mut child = Command::new(server_exe)
        .arg("--single")
        .arg("-D")
        .arg(data_dir)
        // Nothing to make durable twice: if this fails, the cluster is
        // deleted anyway.
        .arg("-F")
        .args(["-c", "exit_on_error=true", MAINTENANCE_DB])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .hidden()
        .spawn()
        .map_err(|e| format!("could not run postgres --single: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // Single-user mode ends a statement at the newline.
        writeln!(stdin, "CREATE ROLE \"{APP_ROLE}\" SUPERUSER LOGIN;")
            .map_err(|e| format!("could not create the {APP_ROLE} role: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("could not create the {APP_ROLE} role: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not create the {APP_ROLE} role: {}",
            last_words(&output)
        ));
    }
    Ok(())
}

/// Whether the server is accepting connections — not merely listening: a
/// server recovering after a crash takes connections on its port and turns
/// every one away with "the database system is starting up" until it's done.
/// Falls back to a plain TCP connect when the build has no `pg_isready`.
pub fn is_ready(server_exe: &Path, port: u16) -> bool {
    let Some(ready) = server_exe
        .parent()
        .map(|bin| bin.join(READY_EXE))
        .filter(|exe| exe.is_file())
    else {
        return std::net::TcpStream::connect((HOST, port)).is_ok();
    };
    Command::new(ready)
        .args(["-h", HOST, "-p", &port.to_string(), "-U", SUPERUSER])
        .args(["-d", MAINTENANCE_DB, "-t", "1", "-q"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .hidden()
        .status()
        .is_ok_and(|status| status.success())
}

/// Asks the server running on `data_dir` to shut down — PostgreSQL's "fast"
/// mode, which rolls back open transactions and closes cleanly, rather than
/// "smart", which waits for every client to disconnect first (a PHP worker
/// holding a pooled connection would stall it to the timeout).
///
/// Returns whether the request was accepted; the caller waits for the
/// process to go.
pub fn request_shutdown(server_exe: &Path, data_dir: &Path) -> bool {
    let Some(ctl) = server_exe
        .parent()
        .map(|bin| bin.join(CTL_EXE))
        .filter(|exe| exe.is_file())
    else {
        return false;
    };
    Command::new(ctl)
        .arg("stop")
        .arg("-D")
        .arg(data_dir)
        .args(["-m", "fast", "--no-wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .hidden()
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_major_is_the_first_number_from_10_on() {
        assert_eq!(major_of("18.6"), "18");
        assert_eq!(major_of("17.11"), "17");
        assert_eq!(major_of("10.23"), "10");
        // Before 10 the major had two parts.
        assert_eq!(major_of("9.6.24"), "9.6");
        assert_eq!(major_of("custom-build"), "custom-build");
    }

    #[test]
    fn each_major_gets_its_own_data_directory() {
        let a = data_dir("18.6").unwrap();
        let b = data_dir("18.7").unwrap();
        let c = data_dir("17.11").unwrap();
        assert_eq!(a, b, "a patch release keeps the same data");
        assert_ne!(a, c);
        assert!(a.ends_with(Path::new("postgres").join("18")));
    }

    #[test]
    fn a_non_empty_list_always_has_exactly_one_active_version() {
        let versions = list();
        let active = versions.iter().filter(|v| v.active).count();
        assert_eq!(active, usize::from(!versions.is_empty()));
    }

    #[test]
    fn set_active_rejects_a_version_that_isnt_on_disk() {
        assert!(matches!(
            set_active("0.0-not-installed"),
            Err(AppError::PostgresVersionNotFound(_))
        ));
    }
}
