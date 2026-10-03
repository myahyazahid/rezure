//! SQL Server Express LocalDB as a Rezure service — a local SQL Server for
//! projects that use one, started and stopped like everything else on the
//! Services page.
//!
//! # Why this isn't a `ProcessService`
//!
//! Every other service is a process Rezure spawns, owns, and tracks by PID.
//! LocalDB is not: `SqlLocalDB.exe` asks Windows to start the instance's
//! `sqlservr.exe`, and that process belongs to LocalDB's own machinery,
//! not to whoever asked. It also listens on no TCP port at all — clients
//! reach it over a named pipe, addressed as `(localdb)\Rezure`. So the state
//! here is read back from `SqlLocalDB info` every time rather than kept, and
//! the card shows that address where other services show a port.
//!
//! LocalDB also starts *itself* when a client connects to a stopped
//! instance. That's why nothing here caches "stopped": a PHP request can
//! turn it on between two polls.
//!
//! # The instance
//!
//! One named instance, [`INSTANCE`], created on first use. LocalDB instances
//! belong to the Windows user that created them, and the PHP Rezure spawns
//! runs as that same user — which is what lets a project connect with
//! Windows Authentication and no password at all, the same "no credentials
//! for a throwaway local server" stance `services::database` takes for
//! MariaDB.
//!
//! # Reaching it over TCP
//!
//! Most SQL clients — TablePlus, DBeaver, HeidiSQL — only speak to SQL Server
//! over TCP, and so can't reach a named pipe at all. `services::localdb_bridge`
//! forwards a fixed local port to whatever pipe the instance has right now,
//! and `services::mssql::ensure_localdb_login` gives those clients a login
//! they can use without Windows Authentication.
//!
//! Installing LocalDB is `services::msi`'s job.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{Service, ServiceInfo, ServiceStatus};
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;
use crate::utils::paths;

/// The instance Rezure creates and manages.
pub const INSTANCE: &str = "Rezure";

/// What a client puts in its `Server=` / `DB_HOST` to reach [`INSTANCE`].
pub const SERVER_ADDRESS: &str = r"(localdb)\Rezure";

/// The service id. Not `mssql-localdb`: the frontend and stored logs key
/// off it, and "sqlserver" is what someone types when filtering the logs.
pub const SERVICE_ID: &str = "sqlserver";

pub const SERVICE_NAME: &str = "SQL Server LocalDB";

/// `SqlLocalDB.exe` of the newest LocalDB installed.
///
/// Found by its documented install location — `Microsoft SQL Server\<NNN>\
/// Tools\Binn` under Program Files, where `NNN` is the major version times
/// ten (`160` for 2022) — rather than through `PATH`: the installer adds
/// itself to the machine `PATH`, but a Rezure that was already running when
/// it did so still has the old one.
pub fn locate() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("ProgramFiles")?).join("Microsoft SQL Server");
    newest_tool(&root)
}

fn newest_tool(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let version: u32 = entry.file_name().to_str()?.parse().ok()?;
            let exe = entry
                .path()
                .join("Tools")
                .join("Binn")
                .join("SqlLocalDB.exe");
            exe.is_file().then_some((version, exe))
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, exe)| exe)
}

pub fn is_installed() -> bool {
    locate().is_some()
}

/// `C:\rezure\data\mssql` — where the databases Rezure creates or restores
/// keep their `.mdf`/`.ldf` files.
///
/// Spelled out on every `CREATE DATABASE` because LocalDB's own default is
/// the root of the user's profile folder (`C:\Users\<name>\blog.mdf`), which
/// is the last place anyone looks for a database and the first one a cleanup
/// tool sweeps.
pub fn data_dir() -> Result<PathBuf, AppError> {
    let dir = paths::data()?.join("mssql");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    Ok(dir)
}

/// What `SqlLocalDB info Rezure` reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InstanceInfo {
    exists: bool,
    running: bool,
    version: String,
    /// `\\.\pipe\LOCALDB#2A8F1D3C\tsql\query` while running. A new name
    /// every time the instance starts, so it's never worth keeping.
    pipe: Option<String>,
}

/// Reads `SqlLocalDB info`'s `Key:   value` lines. An instance exists when
/// the record names it; a missing one prints a sentence instead.
fn parse_info(stdout: &str) -> InstanceInfo {
    let field = |key: &str| {
        stdout.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().to_string())
        })
    };
    InstanceInfo {
        exists: field("Name").is_some_and(|name| !name.is_empty()),
        running: field("State").is_some_and(|state| state.eq_ignore_ascii_case("Running")),
        version: field("Version").unwrap_or_default(),
        pipe: field("Instance pipe name")
            .and_then(|pipe| pipe.strip_prefix("np:").map(str::to_string))
            .filter(|pipe| !pipe.is_empty()),
    }
}

/// The newest version `SqlLocalDB versions` lists — `Microsoft SQL Server
/// 2022 (16.0.1000.6)` → `16.0.1000.6` — for the card to show before the
/// instance exists to report its own.
fn parse_versions(stdout: &str) -> String {
    stdout
        .lines()
        .filter_map(|line| {
            let inner = line.trim().strip_suffix(')')?;
            Some(inner[inner.rfind('(')? + 1..].to_string())
        })
        .max_by(|a, b| super::binaries::compare_versions(a, b))
        .unwrap_or_default()
}

/// The sentence `SqlLocalDB.exe` opens every failure with.
const FAILURE_MARKER: &str = "failed because of the following error";

/// The reason in a failure message — the lines after "… failed because of
/// the following error:", which is the part worth showing.
fn failure_reason(output: &str) -> Option<String> {
    let at = output.find(FAILURE_MARKER)?;
    let reason = output[at + FAILURE_MARKER.len()..]
        .trim_start_matches(':')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Some(if reason.is_empty() {
        output.trim().to_string()
    } else {
        reason
    })
}

/// Runs `SqlLocalDB <args>`, returning stdout on success and the tool's own
/// reason on failure.
///
/// The exit code can't decide which: `SqlLocalDB.exe` exits 0 even when it
/// fails — a `start` of an instance that doesn't exist included — and says
/// so only in text, on stdout for `info` and on stderr for the rest.
fn sqllocaldb(exe: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(exe)
        .args(args)
        .hidden()
        .output()
        .map_err(|e| format!("could not run SqlLocalDB.exe: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if let Some(reason) = failure_reason(&format!("{stdout}\n{stderr}")) {
        return Err(reason);
    }
    if !output.status.success() {
        let message = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or("SqlLocalDB.exe failed")
            .to_string();
        return Err(message);
    }
    Ok(stdout)
}

/// Status reads are cached this long, so the Services page's poll and a
/// Databases page load landing together cost one `SqlLocalDB info`, not
/// several. Short enough that LocalDB starting itself on a client's connect
/// still shows up on the next poll.
const INFO_CACHE: Duration = Duration::from_millis(1500);

fn info_cache() -> &'static Mutex<Option<(Instant, InstanceInfo)>> {
    static CACHE: Mutex<Option<(Instant, InstanceInfo)>> = Mutex::new(None);
    &CACHE
}

fn forget_cached_info() {
    *info_cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn instance_info(exe: &Path) -> InstanceInfo {
    let mut cache = info_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, info)) = cache.as_ref() {
        if at.elapsed() < INFO_CACHE {
            return info.clone();
        }
    }
    let mut info = match sqllocaldb(exe, &["info", INSTANCE]) {
        Ok(stdout) => parse_info(&stdout),
        Err(_) => InstanceInfo::default(),
    };
    if info.version.is_empty() {
        info.version = sqllocaldb(exe, &["versions"])
            .map(|stdout| parse_versions(&stdout))
            .unwrap_or_default();
    }
    *cache = Some((Instant::now(), info.clone()));
    info
}

/// Makes sure [`INSTANCE`] exists, creating it — stopped — if it doesn't.
/// Returns whether it had to.
///
/// Run as soon as LocalDB is installed, and at startup, rather than left to
/// the first Start: once the instance exists, LocalDB starts it by itself
/// the moment a client connects, so a project pointed at `(localdb)\Rezure`
/// works whether or not anyone pressed Start. Before it exists, it fails.
pub fn ensure_created() -> Result<bool, AppError> {
    let exe = locate().ok_or_else(|| AppError::BinaryNotInstalled(SERVICE_NAME.to_string()))?;
    if instance_info(&exe).exists {
        return Ok(false);
    }
    sqllocaldb(&exe, &["create", INSTANCE]).map_err(|reason| AppError::ProcessSpawnFailed {
        name: SERVICE_NAME.to_string(),
        reason,
    })?;
    forget_cached_info();
    log::info!("created the LocalDB instance {INSTANCE}");
    Ok(true)
}

/// Makes sure [`INSTANCE`] exists and is running, creating it on first use.
///
/// Called by the service's Start, and by `services::mssql` before it
/// connects: unlike LocalDB's automatic `MSSQLLocalDB`, a *named* instance is
/// never created on demand, and a project pointed at `(localdb)\Rezure`
/// before anyone pressed Start would otherwise fail with "instance not
/// found".
pub fn ensure_running() -> Result<(), AppError> {
    let exe = locate().ok_or_else(|| AppError::BinaryNotInstalled(SERVICE_NAME.to_string()))?;
    let info = instance_info(&exe);
    if info.running {
        return Ok(());
    }
    if !info.exists {
        ensure_created()?;
    }
    forget_cached_info();
    sqllocaldb(&exe, &["start", INSTANCE]).map_err(|reason| AppError::ProcessSpawnFailed {
        name: SERVICE_NAME.to_string(),
        reason,
    })?;
    forget_cached_info();
    Ok(())
}

/// The pipe the running instance listens on, starting it first if need be —
/// what `services::localdb_bridge` connects each TCP client to.
pub fn pipe_path() -> Result<String, AppError> {
    ensure_running()?;
    let exe = locate().ok_or_else(|| AppError::BinaryNotInstalled(SERVICE_NAME.to_string()))?;
    instance_info(&exe)
        .pipe
        .ok_or_else(|| AppError::ProcessSpawnFailed {
            name: SERVICE_NAME.to_string(),
            reason: "LocalDB reported no pipe for the running instance".to_string(),
        })
}

/// The LocalDB instance, on the Services page.
pub struct LocalDbService;

impl LocalDbService {
    fn stop_with(&self, kill: bool) -> Result<ServiceInfo, AppError> {
        let Some(exe) = locate() else {
            return Ok(self.info());
        };
        if instance_info(&exe).running {
            // Without `-k` this is a clean SHUTDOWN, which waits for open
            // transactions; `-k` kills the process, as Force stop promises.
            let mut args = vec!["stop", INSTANCE];
            if kill {
                args.push("-k");
            }
            let result = sqllocaldb(&exe, &args);
            forget_cached_info();
            result.map_err(|reason| AppError::ProcessSpawnFailed {
                name: SERVICE_NAME.to_string(),
                reason,
            })?;
        }
        Ok(self.info())
    }
}

impl Service for LocalDbService {
    fn id(&self) -> &str {
        SERVICE_ID
    }

    fn info(&self) -> ServiceInfo {
        let exe = locate();
        let instance = exe.as_deref().map(instance_info).unwrap_or_default();
        ServiceInfo {
            id: SERVICE_ID.to_string(),
            name: SERVICE_NAME.to_string(),
            category: "Database".to_string(),
            status: if instance.running {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            },
            version: instance.version,
            // LocalDB's own listener is a named pipe — see the module docs.
            // The port is the bridge's, when it's listening.
            port: super::localdb_bridge::listening_port().unwrap_or(0),
            // The process is LocalDB's, not Rezure's, so there is no PID to
            // sample CPU from.
            cpu_percent: None,
            cpu_history: Vec::new(),
            workers: None,
            ports: super::localdb_bridge::listening_port()
                .into_iter()
                .collect(),
            installed: exe.is_some(),
            install_id: Some("sqllocaldb".to_string()),
            web_url: None,
            endpoint: Some(SERVER_ADDRESS.to_string()),
        }
    }

    fn start(&self) -> Result<ServiceInfo, AppError> {
        ensure_running()?;
        Ok(self.info())
    }

    fn stop(&self) -> Result<ServiceInfo, AppError> {
        self.stop_with(false)
    }

    fn force_stop(&self) -> Result<ServiceInfo, AppError> {
        self.stop_with(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: &str = "Name:               Rezure\r\n\
        Version:            16.0.1000.6\r\n\
        Shared name:\r\n\
        Owner:              DESKTOP-1\\dev\r\n\
        Auto-create:        No\r\n\
        State:              Running\r\n\
        Last start time:    03/10/2026 18:40:12\r\n\
        Instance pipe name: np:\\\\.\\pipe\\LOCALDB#2A8F1D3C\\tsql\\query\r\n";

    /// Exactly what `SqlLocalDB info Rezure` prints, with exit code 0, for
    /// an instance nobody created yet.
    const MISSING: &str = "Printing of LocalDB instance \"Rezure\" information failed because \
        of the following error:\r\n\r\nLocalDB instance \"Rezure\" doesn't exist! \r\n";

    #[test]
    fn a_running_instance_is_read_from_its_info_record() {
        let info = parse_info(RUNNING);
        assert!(info.exists);
        assert!(info.running);
        assert_eq!(info.version, "16.0.1000.6");
        assert_eq!(
            info.pipe.as_deref(),
            Some(r"\\.\pipe\LOCALDB#2A8F1D3C\tsql\query")
        );
    }

    #[test]
    fn a_stopped_instance_exists_but_is_not_running() {
        let info = parse_info(&RUNNING.replace("Running", "Stopped"));
        assert!(info.exists);
        assert!(!info.running);
    }

    /// The tool reports a missing instance in a sentence, not a record —
    /// that's the signal to create it, not an error to show.
    #[test]
    fn a_missing_instance_reads_as_not_existing() {
        let info = parse_info(MISSING);
        assert!(!info.exists);
        assert!(!info.running);
    }

    /// `SqlLocalDB.exe` exits 0 when it fails, so the failure has to be
    /// read from what it says — this is what let a `start` of an instance
    /// that was never created pass as a success.
    #[test]
    fn a_failure_is_recognised_by_its_wording_and_reduced_to_the_reason() {
        assert_eq!(
            failure_reason(
                "Start of LocalDB instance \"Rezure\" failed because of the following error:\r\n\
                 The specified LocalDB instance does not exist.\r\n"
            )
            .as_deref(),
            Some("The specified LocalDB instance does not exist.")
        );
        assert!(failure_reason(MISSING).is_some());
        assert!(failure_reason("LocalDB instance \"Rezure\" started.\r\n").is_none());
    }

    #[test]
    fn the_newest_installed_version_is_read_from_the_versions_list() {
        let listed = "Microsoft SQL Server 2019 (15.0.4153.1)\r\n\
                      Microsoft SQL Server 2022 (16.0.1000.6)\r\n";
        assert_eq!(parse_versions(listed), "16.0.1000.6");
        assert_eq!(parse_versions(""), "");
    }

    #[test]
    fn the_newest_installed_major_version_is_chosen() {
        let root = std::env::temp_dir().join(format!("rezure-test-localdb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for version in ["150", "160", "Shared"] {
            let binn = root.join(version).join("Tools").join("Binn");
            std::fs::create_dir_all(&binn).unwrap();
            std::fs::write(binn.join("SqlLocalDB.exe"), b"").unwrap();
        }
        // A newer folder without the tool in it doesn't count.
        std::fs::create_dir_all(root.join("170")).unwrap();

        let found = newest_tool(&root).unwrap();
        assert!(found.starts_with(root.join("160")), "{}", found.display());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Reads the real LocalDB on this machine, if there is one. Run with:
    /// `cargo test --lib services::mssql_localdb::tests::print_real_instance -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_real_instance() {
        println!("SqlLocalDB.exe: {:?}", locate());
        println!("{:?}", LocalDbService.info());
    }
}
