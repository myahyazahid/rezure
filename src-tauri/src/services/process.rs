//! Real, process-backed [`Service`] implementation — spawns a service's
//! portable binary (see [`crate::services::binaries`]) and tracks it with
//! `sysinfo`.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::{AppHandle, Emitter};

use super::binaries;
use super::database;
use super::db_engine;
use super::db_profiles;
use super::php_ini;
use super::vhosts::{self, PHP_FASTCGI_PORT};
use super::{Service, ServiceHandle, ServiceInfo, ServiceManager, ServiceStatus, CPU_HISTORY_LEN};
use crate::utils::error::AppError;

/// Event name the frontend subscribes to via `listen()` for a service's
/// stdout/stderr, line by line, as it's produced.
pub const LOG_EVENT: &str = "service://log";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLogLine {
    pub service_id: String,
    pub stream: LogStream,
    pub line: String,
}

/// Where a service's log lines go. Kept as a plain callback (rather than
/// threading an `AppHandle` through the `Service` trait) so `ProcessService`
/// stays testable without a running Tauri app — `real_services` is the only
/// place that wires it up to a real `app.emit`.
pub type LogSink = Arc<dyn Fn(&str, LogStream, &str) + Send + Sync>;

#[cfg(test)]
fn no_op_sink() -> LogSink {
    Arc::new(|_id, _stream, _line| {})
}

/// Per-service launch recipe, applied once the binary is confirmed installed.
enum Launch {
    /// Runs with Rezure's own generated config (`services::vhosts`), which
    /// `include`s a `.conf` per detected project — the binary's own
    /// extracted `conf/nginx.conf` is never touched.
    Nginx,
    /// `php-cgi -b 127.0.0.1:<port>` — a stateless FastCGI TCP responder,
    /// not the built-in dev server. There's no PHP-FPM on Windows; this is
    /// the standard substitute (what Laragon/XAMPP-style stacks use too).
    /// Only useful behind a reverse proxy that sets `SCRIPT_FILENAME` per
    /// request (i.e. an Nginx vhost) — visiting the port directly does
    /// nothing, unlike the old built-in-server mode.
    Php,
    /// `mysqld --datadir=<dir> --port=<port>`, where all three of the binary,
    /// the datadir and the port come from whichever profile is active (see
    /// `services::db_profiles`) — resolved at spawn time, not construction,
    /// which is what makes switching profiles take effect on restart. An
    /// empty datadir is bootstrapped first, per engine.
    Database,
}

pub struct ProcessService {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    port: u16,
    launch: Launch,
    log_sink: LogSink,
    child: Mutex<Option<Child>>,
    sys: Mutex<System>,
    cpu_history: Mutex<Vec<u8>>,
}

/// `%LOCALAPPDATA%\Rezure\data\<id>` — a service's own working data (PHP's
/// default docroot, MariaDB's datadir), separate from the versioned binary
/// cache under `binaries::install_root`.
fn runtime_dir(id: &str) -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base.join("Rezure").join("data").join(id))
}

/// Where a service's last-known PID is recorded across app restarts.
///
/// `child: Mutex<Option<Child>>` only lives as long as this `AppHandle`, so
/// if Rezure is force-closed or crashes while a service is running, the
/// next launch has no in-memory record of it — yet the OS process (and
/// whatever port it holds) is still alive, since Windows doesn't tie a
/// child's lifetime to its parent's. This file is the breadcrumb that lets
/// `reap_orphan` recognize and clean up exactly that leftover, and nothing
/// else — see its doc comment for the matching rule.
fn pid_file_path(id: &str) -> Result<PathBuf, AppError> {
    Ok(runtime_dir(id)?.join("service.pid"))
}

impl ProcessService {
    pub fn nginx(log_sink: LogSink) -> Result<Self, AppError> {
        binaries::find("nginx")?; // fail fast if the manifest entry is ever missing
        Ok(Self {
            id: "nginx",
            name: "Nginx",
            category: "Web server",
            port: 80,
            launch: Launch::Nginx,
            log_sink,
            child: Mutex::new(None),
            sys: Mutex::new(System::new()),
            cpu_history: Mutex::new(Vec::new()),
        })
    }

    pub fn php(log_sink: LogSink) -> Result<Self, AppError> {
        if binaries::family_packages("php").is_empty() {
            return Err(AppError::UnknownBinary("php".to_string()));
        }
        Ok(Self {
            id: "php",
            name: "PHP",
            category: "Runtime",
            port: PHP_FASTCGI_PORT,
            launch: Launch::Php,
            log_sink,
            child: Mutex::new(None),
            sys: Mutex::new(System::new()),
            cpu_history: Mutex::new(Vec::new()),
        })
    }

    /// The database service. Keeps the `mariadb` id it has always had — the
    /// frontend and stored logs key off it — while everything it actually
    /// runs now follows the active profile, which may well be MySQL.
    pub fn mariadb(log_sink: LogSink) -> Result<Self, AppError> {
        binaries::find("mariadb")?;
        Ok(Self {
            id: "mariadb",
            name: "Database",
            category: "Database",
            // Fallback only, for when no profile is resolvable — the real
            // port comes from the active profile via `current_port`.
            port: 3306,
            launch: Launch::Database,
            log_sink,
            child: Mutex::new(None),
            sys: Mutex::new(System::new()),
            cpu_history: Mutex::new(Vec::new()),
        })
    }

    /// The version shown on the service card — for PHP and the database,
    /// whichever one is active right now, not a fixed manifest entry.
    fn version(&self) -> String {
        match self.launch {
            Launch::Php => super::php::active_id(),
            Launch::Nginx => binaries::find("nginx")
                .map(|pkg| pkg.version.to_string())
                .unwrap_or_default(),
            Launch::Database => db_profiles::active()
                .map(|profile| profile.version)
                .unwrap_or_default(),
        }
    }

    /// The name shown on the service card. For the database this follows the
    /// active profile's engine, so a card reading "MariaDB" while MySQL is
    /// running can't happen.
    fn display_name(&self) -> String {
        match self.launch {
            Launch::Database => db_profiles::active()
                .map(|profile| profile.engine.label().to_string())
                .unwrap_or_else(|| self.name.to_string()),
            _ => self.name.to_string(),
        }
    }

    /// The port this service actually binds. Fixed for nginx and PHP; for
    /// the database it follows the active profile, which is what lets one
    /// profile sit on 3306 while another uses a different port.
    fn current_port(&self) -> u16 {
        match self.launch {
            Launch::Database => db_profiles::active()
                .map(|profile| profile.port)
                .unwrap_or(self.port),
            _ => self.port,
        }
    }

    /// The binary that actually ends up running.
    ///
    /// nginx and MariaDB are one pinned manifest version each. PHP is
    /// neither pinned nor necessarily in the manifest at all — it resolves
    /// through `services::php`, which scans what's installed on disk (a
    /// download or a folder the user dropped in) and follows whichever
    /// version is currently active, so a switch takes effect the next time
    /// the service starts. It's also the one case where the spawned binary
    /// differs from the one that identifies the install: `php.exe` is what
    /// gets found, `php-cgi.exe` beside it is what gets run.
    ///
    /// Shared by `command()` (to build the spawn) and `reap_orphan()` (to
    /// recognize a leftover from a previous run), so the two can never
    /// drift apart.
    fn resolved_exe(&self) -> Result<PathBuf, AppError> {
        match self.launch {
            Launch::Php => {
                let exe = super::php::active_exe()?;
                Ok(exe
                    .parent()
                    .ok_or_else(|| AppError::Io("php.exe has no parent directory".to_string()))?
                    .join("php-cgi.exe"))
            }
            Launch::Nginx => binaries::exe_path(binaries::find("nginx")?),
            // Resolved through the active profile, so a switch to a MySQL
            // profile spawns MySQL's own `mysqld.exe`, not MariaDB's.
            Launch::Database => {
                let profile = db_profiles::active()
                    .ok_or_else(|| AppError::BinaryNotInstalled("Database".to_string()))?;
                db_profiles::resolve_server_exe(&profile)
            }
        }
    }

    /// Builds the `Command` to spawn, preparing any per-service runtime
    /// state (PHP's docroot, MariaDB's data directory) it needs first.
    fn command(&self) -> Result<Command, AppError> {
        let exe = self.resolved_exe()?;

        let mut cmd = match &self.launch {
            Launch::Nginx => {
                let config_path = vhosts::ensure_main_config(&exe)?;
                let mut cmd = Command::new(&exe);
                cmd.args(["-c".as_ref(), config_path.as_os_str()])
                    .args(["-p".as_ref(), vhosts::nginx_runtime_dir()?.as_os_str()]);
                cmd
            }
            Launch::Php => {
                let ini_path = php_ini::ensure_php_ini(&exe)?;
                let mut cmd = Command::new(&exe);
                cmd.arg("-c")
                    .arg(&ini_path)
                    .arg("-b")
                    .arg(format!("127.0.0.1:{}", self.port));
                cmd
            }
            Launch::Database => {
                let profile = db_profiles::active()
                    .ok_or_else(|| AppError::BinaryNotInstalled("Database".to_string()))?;
                let data_dir = PathBuf::from(&profile.datadir_path);

                // Only ever writes into a folder that's empty — an adopted
                // datadir already holds someone's data and is opened as-is.
                if db_engine::needs_bootstrap(&data_dir) {
                    profile.engine.bootstrap(&exe, &data_dir)?;
                }

                let mut cmd = Command::new(&exe);
                cmd.arg(format!("--datadir={}", data_dir.display()))
                    .arg(format!("--port={}", profile.port))
                    .arg("--bind-address=127.0.0.1")
                    .arg("--console");
                cmd
            }
        };

        // Piped, not inherited: a GUI-subsystem build has no console handles
        // to inherit, and piping is what lets us stream lines out below.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(cmd)
    }

    /// The address this service actually binds to — must match `command()`
    /// exactly, since that's what makes the port-conflict check meaningful.
    fn bind_addr(&self) -> &'static str {
        match self.launch {
            Launch::Nginx => "0.0.0.0",
            Launch::Php | Launch::Database => "127.0.0.1",
        }
    }

    /// Spawns a thread per stream that forwards each line to `log_sink` as
    /// it's written, until the pipe closes (the process exits or is
    /// killed) — no shutdown signal needed, it just runs out of input.
    fn spawn_log_readers(&self, child: &mut Child) {
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(self.log_sink.clone(), self.id, LogStream::Stdout, stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(self.log_sink.clone(), self.id, LogStream::Stderr, stderr);
        }
    }

    /// Checks whether the tracked child is still alive, reaping it (and
    /// returning `None`) if it has exited — whether from a `stop()` or a
    /// crash, `std::process` can't tell the difference after the fact.
    fn poll_pid(&self) -> Option<u32> {
        let mut child_guard = self.child.lock().unwrap();
        let still_running = child_guard
            .as_mut()
            .map(|child| matches!(child.try_wait(), Ok(None)))
            .unwrap_or(false);

        if still_running {
            child_guard.as_ref().map(|child| child.id())
        } else {
            *child_guard = None;
            None
        }
    }

    /// Refreshes `pid`'s CPU usage via the service's own long-lived
    /// `System`, appending the sample to the sparkline history. A single
    /// refresh with no prior baseline reads as 0% — expected right after a
    /// fresh start, and self-corrects on the next call.
    fn sample_cpu(&self, pid: u32) -> Option<u8> {
        let pid = Pid::from_u32(pid);
        let cpu = {
            let mut sys = self.sys.lock().unwrap();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_cpu(),
            );
            sys.process(pid).map(|p| p.cpu_usage())?
        };

        let sample = cpu.round().clamp(0.0, 100.0) as u8;
        let mut history = self.cpu_history.lock().unwrap();
        history.push(sample);
        if history.len() > CPU_HISTORY_LEN {
            let excess = history.len() - CPU_HISTORY_LEN;
            history.drain(0..excess);
        }
        Some(sample)
    }
}

fn spawn_log_reader(
    sink: LogSink,
    service_id: &'static str,
    stream: LogStream,
    reader: impl Read + Send + 'static,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            sink(service_id, stream, &line);
        }
    });
}

/// Binds `addr:port` just to immediately drop the listener and free it
/// again — if the bind itself fails, something else already owns it. This
/// can't fully rule out a race against whatever grabs the port between the
/// check and the real spawn, but it catches the common case (another dev
/// server, or a leftover process) before wasting time trying to start.
///
/// `addr` matters more than it looks: Windows, unlike Linux, generally
/// allows a `0.0.0.0:PORT` bind and a `127.0.0.1:PORT` bind to coexist
/// without `SO_EXCLUSIVEADDRUSE` (which `std`/`socket2` don't expose), so
/// checking the wrong address can silently miss a real conflict. Probing
/// the exact address the service itself will bind to is the check this
/// can actually make reliably.
fn ensure_port_available(addr: &str, port: u16, name: &str) -> Result<(), AppError> {
    TcpListener::bind((addr, port))
        .map(|_| ())
        .map_err(|_| AppError::PortInUse {
            port,
            name: name.to_string(),
        })
}

/// How long a database server gets to flush and close on its own before
/// it's force-killed.
///
/// Generous on purpose. A clean InnoDB shutdown has to flush dirty pages,
/// and the whole point of the profile switcher is pointing at datadirs
/// measured in gigabytes — a timeout tuned for Rezure's own near-empty
/// datadir would force-kill exactly the large, valuable ones.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// Asks a running server to shut down cleanly.
///
/// `mysqladmin shutdown` is the supported way in: it connects and issues a
/// real shutdown, so InnoDB flushes and closes its tablespaces instead of
/// being cut off mid-write. `mysqladmin.exe` is spelled the same on both
/// engines — MariaDB ships it as an alias beside `mariadb-admin.exe`.
///
/// Returns whether the *request* was accepted; the caller still waits for
/// the process to actually go.
fn request_graceful_shutdown(server_exe: &Path, port: u16) -> bool {
    let Some(bin_dir) = server_exe.parent() else {
        return false;
    };
    let admin = bin_dir.join(db_engine::ADMIN_EXE);
    if !admin.is_file() {
        return false;
    }

    Command::new(&admin)
        .args([
            "--protocol=TCP",
            "-h",
            database::HOST,
            "-P",
            &port.to_string(),
            "-u",
            database::USER,
            "shutdown",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Polls until `child` exits or `timeout` elapses.
fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return false,
        }
    }
    false
}

impl Service for ProcessService {
    fn id(&self) -> &str {
        self.id
    }

    fn info(&self) -> ServiceInfo {
        let pid = self.poll_pid();

        let cpu_percent = match pid {
            Some(pid) => self.sample_cpu(pid),
            None => {
                self.cpu_history.lock().unwrap().clear();
                None
            }
        };

        // Best-effort: an empty version just leaves the badge blank, which
        // is the honest answer when nothing is installed to report on.
        let version = self.version();

        ServiceInfo {
            id: self.id.to_string(),
            name: self.display_name(),
            category: self.category.to_string(),
            status: if pid.is_some() {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            },
            version,
            port: self.current_port(),
            cpu_percent,
            cpu_history: self.cpu_history.lock().unwrap().clone(),
        }
    }

    fn start(&self) -> Result<ServiceInfo, AppError> {
        if self.poll_pid().is_some() {
            return Ok(self.info());
        }

        // Resolving the exe *is* the installed check now: for PHP there's no
        // manifest entry to look up, just whatever the disk scan found.
        if !self.resolved_exe().is_ok_and(|exe| exe.is_file()) {
            return Err(AppError::BinaryNotInstalled(self.name.to_string()));
        }

        self.reap_orphan()?;
        ensure_port_available(self.bind_addr(), self.current_port(), self.name)?;

        let mut cmd = self.command()?;
        let mut child = cmd.spawn().map_err(|e| AppError::ProcessSpawnFailed {
            name: self.name.to_string(),
            reason: e.to_string(),
        })?;

        if let Ok(path) = pid_file_path(self.id) {
            let _ = fs::write(&path, child.id().to_string());
        }

        self.spawn_log_readers(&mut child);
        *self.child.lock().unwrap() = Some(child);
        Ok(self.info())
    }

    /// Stops the service, giving a database the chance to close cleanly
    /// first.
    ///
    /// nginx and `php-cgi` hold no state worth flushing, so they go straight
    /// to `kill_process_tree`. A database does not: force-killing `mysqld`
    /// leaves the datadir needing crash recovery on next start, and against
    /// an adopted multi-gigabyte datadir that is somebody's real data. So it
    /// is asked to shut down, given [`GRACEFUL_SHUTDOWN_TIMEOUT`] to do it,
    /// and only force-killed if it doesn't — a hung server still has to be
    /// stoppable.
    fn stop(&self) -> Result<ServiceInfo, AppError> {
        let mut child_guard = self.child.lock().unwrap();
        if let Some(mut child) = child_guard.take() {
            let stopped_cleanly = matches!(self.launch, Launch::Database)
                && self
                    .resolved_exe()
                    .is_ok_and(|exe| request_graceful_shutdown(&exe, self.current_port()))
                && wait_for_exit(&mut child, GRACEFUL_SHUTDOWN_TIMEOUT);

            if !stopped_cleanly {
                if matches!(self.launch, Launch::Database) {
                    log::warn!(
                        "{} did not shut down cleanly in {}s — force-killing it",
                        self.id,
                        GRACEFUL_SHUTDOWN_TIMEOUT.as_secs()
                    );
                }
                kill_process_tree(child.id());
            }
            let _ = child.wait();
        }
        drop(child_guard);
        if let Ok(path) = pid_file_path(self.id) {
            let _ = fs::remove_file(&path);
        }
        self.cpu_history.lock().unwrap().clear();
        Ok(self.info())
    }
}

impl ProcessService {
    /// Cleans up a leftover instance of this exact service from a previous
    /// Rezure run — see [`pid_file_path`] for why this can happen. Only
    /// acts when the recorded PID is *still alive and still running this
    /// service's own binary*; a dead PID (clean shutdown) or one recycled
    /// by an unrelated process is left completely alone, since a stale
    /// record is the only thing this can verify — it's never treated as
    /// license to kill whatever happens to be using the port.
    fn reap_orphan(&self) -> Result<(), AppError> {
        let pid_path = pid_file_path(self.id)?;
        let Ok(recorded) = fs::read_to_string(&pid_path) else {
            return Ok(());
        };
        // Whatever happens next, this record is about to be superseded by
        // either a fresh start or a confirmed-dead entry — never left
        // pointing at a PID that's no longer meaningful.
        let _ = fs::remove_file(&pid_path);

        let Ok(pid) = recorded.trim().parse::<u32>() else {
            return Ok(());
        };

        let expected_exe = self.resolved_exe()?;
        let sys_pid = Pid::from_u32(pid);
        let is_ours = {
            let mut sys = self.sys.lock().unwrap();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[sys_pid]),
                true,
                ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
            );
            sys.process(sys_pid)
                .and_then(|p| p.exe())
                .is_some_and(|exe| exe == expected_exe)
        };

        if is_ours {
            kill_process_tree(pid);
        }
        Ok(())
    }
}

/// Terminates `pid` and every process it spawned. Plain `Child::kill()`
/// only signals the one process it tracks — Windows has no `fork()`, so
/// nginx's worker (and anything the worker itself spawns) is a genuinely
/// separate PID that a single `TerminateProcess` call never reaches,
/// leaving it running and still holding the port. `taskkill /T` walks and
/// kills the whole tree; falling back to a plain kill covers the rare case
/// where `taskkill.exe` isn't reachable.
fn kill_process_tree(pid: u32) {
    let tree_killed = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if !tree_killed {
        // Best-effort fallback — still better than leaving it running.
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The services Rezure actually manages — one per downloadable package in
/// `binaries::MANIFEST`. Panics only if that manifest and these hardcoded
/// ids drift apart, which `tests::every_service_matches_the_manifest`
/// catches at build time.
pub fn real_services(app: AppHandle) -> ServiceManager {
    let sink: LogSink = Arc::new(move |service_id, stream, line| {
        let payload = ServiceLogLine {
            service_id: service_id.to_string(),
            stream,
            line: line.to_string(),
        };
        if let Err(err) = app.emit(LOG_EVENT, payload) {
            log::warn!("failed to emit service log line: {err}");
        }
    });

    let services: Vec<ServiceHandle> = vec![
        Arc::new(ProcessService::nginx(sink.clone()).expect("nginx must be in binaries::MANIFEST")),
        Arc::new(ProcessService::php(sink.clone()).expect("php must be in binaries::MANIFEST")),
        Arc::new(ProcessService::mariadb(sink).expect("mariadb must be in binaries::MANIFEST")),
    ];
    ServiceManager::new(services)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_service_matches_the_manifest() {
        assert!(ProcessService::nginx(no_op_sink()).is_ok());
        assert!(ProcessService::php(no_op_sink()).is_ok());
        assert!(ProcessService::mariadb(no_op_sink()).is_ok());
    }

    #[test]
    fn a_freshly_constructed_service_reports_stopped() {
        let service = ProcessService::nginx(no_op_sink()).unwrap();
        let info = service.info();

        assert_eq!(info.id, "nginx");
        assert_eq!(info.status, ServiceStatus::Stopped);
        assert_eq!(info.cpu_percent, None);
        assert!(info.cpu_history.is_empty());
    }

    #[test]
    fn stopping_a_service_that_never_started_is_a_harmless_no_op() {
        let service = ProcessService::mariadb(no_op_sink()).unwrap();
        let info = service.stop().unwrap();
        assert_eq!(info.status, ServiceStatus::Stopped);
    }

    #[test]
    fn a_port_already_bound_is_reported_before_spawning() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let err = ensure_port_available("127.0.0.1", port, "Test").unwrap_err();
        assert!(matches!(err, AppError::PortInUse { port: p, .. } if p == port));

        drop(listener);
        ensure_port_available("127.0.0.1", port, "Test")
            .expect("port must be free once the listener drops");
    }

    #[test]
    fn reap_orphan_is_a_no_op_when_theres_no_recorded_pid() {
        let service = ProcessService::mariadb(no_op_sink()).unwrap();
        let pid_path = pid_file_path(service.id).unwrap();
        let _ = fs::remove_file(&pid_path);

        assert!(service.reap_orphan().is_ok());
    }

    #[test]
    fn reap_orphan_leaves_a_live_pid_alone_when_its_not_this_services_binary() {
        let service = ProcessService::nginx(no_op_sink()).unwrap();
        let pid_path = pid_file_path(service.id).unwrap();
        fs::create_dir_all(pid_path.parent().unwrap()).unwrap();
        // The current test process is guaranteed alive, but it's the test
        // binary, not nginx.exe — reap_orphan must recognize the mismatch
        // and leave it running rather than killing an unrelated process.
        fs::write(&pid_path, std::process::id().to_string()).unwrap();

        service.reap_orphan().unwrap();

        assert!(
            !pid_path.exists(),
            "a resolved record — match or not — must not be left pointing at a stale pid"
        );
        assert!(
            sysinfo::System::new_all()
                .process(Pid::from_u32(std::process::id()))
                .is_some(),
            "the current process must still be running"
        );
    }

    /// Real spawn/stop against the actual downloaded binary — not run by
    /// default since it needs the package installed first (via the
    /// Binaries panel, or `binaries::install`) and takes a few seconds.
    /// Run explicitly with:
    /// `cargo test --lib services::process::tests::start_and_stop_a_real_mariadb -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn start_and_stop_a_real_mariadb() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let recorded = received.clone();
        let sink: LogSink = Arc::new(move |_id, _stream, line| {
            recorded.lock().unwrap().push(line.to_string());
        });

        let service = ProcessService::mariadb(sink).unwrap();

        let started = service
            .start()
            .expect("mariadb must be installed to run this test");
        assert_eq!(started.status, ServiceStatus::Running);
        assert!(started.cpu_history.is_empty() || started.cpu_percent.is_some());

        // mysqld logs its startup sequence to stderr — give the reader
        // thread a moment to catch at least one line.
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(
            !received.lock().unwrap().is_empty(),
            "expected at least one log line from mysqld's startup"
        );

        let stopped = service.stop().unwrap();
        assert_eq!(stopped.status, ServiceStatus::Stopped);
    }

    /// Same as `start_and_stop_a_real_mariadb`, for nginx. Run with:
    /// `cargo test --lib services::process::tests::start_and_stop_a_real_nginx -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn start_and_stop_a_real_nginx() {
        let service = ProcessService::nginx(no_op_sink()).unwrap();

        let started = service
            .start()
            .expect("nginx must be installed to run this test");
        assert_eq!(started.status, ServiceStatus::Running);

        let stopped = service.stop().unwrap();
        assert_eq!(stopped.status, ServiceStatus::Stopped);

        // nginx on Windows spawns a real worker *process* (no `fork()`),
        // which a plain `TerminateProcess` on just the master never
        // reaches — this regression-tests `kill_process_tree` actually
        // takes the whole tree down, not just the master.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let leftover = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq nginx.exe", "/NH"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("nginx.exe"))
            .unwrap_or(false);
        assert!(!leftover, "a worker process survived stop()");
    }

    /// Same as `start_and_stop_a_real_mariadb`, for `php-cgi`'s FastCGI
    /// mode. Run with:
    /// `cargo test --lib services::process::tests::start_and_stop_a_real_php -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn start_and_stop_a_real_php() {
        let service = ProcessService::php(no_op_sink()).unwrap();

        let started = service
            .start()
            .expect("php must be installed to run this test");
        assert_eq!(started.status, ServiceStatus::Running);

        let stopped = service.stop().unwrap();
        assert_eq!(stopped.status, ServiceStatus::Stopped);
    }

    /// The reload path in one assertion, with no processes involved: the
    /// binary a spawn *would* use has to follow the active version, and be
    /// that version's own `php-cgi.exe`.
    ///
    /// This is what makes the Switch page's auto-reload work at all — the
    /// restart only lands on the new version because `resolved_exe`
    /// re-resolves at spawn time instead of caching a path. Runs anywhere,
    /// so it guards that property on every `cargo test`.
    #[test]
    fn the_binary_a_spawn_would_use_follows_the_active_php_version() {
        use crate::services::php;

        let installed = php::installed();
        if installed.len() < 2 {
            // Nothing to switch between on this machine; the assertions
            // below would be vacuous rather than wrong.
            return;
        }

        let original_active = php::active_id();
        let service = ProcessService::php(no_op_sink()).unwrap();

        for runtime in &installed {
            php::set_active(&runtime.version).unwrap();

            let exe = service.resolved_exe().unwrap();
            assert!(
                exe.ends_with("php-cgi.exe"),
                "php runs as php-cgi, got {}",
                exe.display()
            );
            assert!(
                exe.starts_with(&runtime.dir),
                "after switching to {}, the spawn should resolve inside {} — got {}",
                runtime.version,
                runtime.dir.display(),
                exe.display()
            );
        }

        php::set_active(&original_active).unwrap();
    }

    /// Switches the active PHP version *while the service is running* and
    /// confirms the actual spawned process — not just what the code claims
    /// — moved to the new version's own `php-cgi.exe`.
    ///
    /// This is the mechanism behind the Switch page's auto-reload:
    /// `commands::php::set_active_php_version` does exactly this pair
    /// (`set_active` then `restart`) when the service is up. Needs at least
    /// two installed PHP versions. Run with:
    /// `cargo test --lib services::process::tests::switching_php_version_reloads_onto_the_new_binary -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn switching_php_version_reloads_onto_the_new_binary() {
        use crate::services::php;

        let installed = php::installed();
        assert!(
            installed.len() >= 2,
            "need at least 2 installed PHP versions to run this test, found {}",
            installed.len()
        );

        // Rezure itself holds this port whenever PHP is running, and this
        // test needs to spawn its own — say so plainly instead of failing
        // later with a bare PortInUse.
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", PHP_FASTCGI_PORT)).is_ok(),
            "port {PHP_FASTCGI_PORT} is busy — stop PHP in Rezure before running this test"
        );

        let original_active = php::active_id();
        let service = ProcessService::php(no_op_sink()).unwrap();

        // Start on the first version, then switch to each of the others
        // without stopping first — the restart is what has to move it.
        php::set_active(&installed[0].version).unwrap();
        service.start().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(
            running_php_cgi_path().contains(&installed[0].version),
            "expected the service to start on {}, got: {}",
            installed[0].version,
            running_php_cgi_path()
        );

        for runtime in installed.iter().skip(1) {
            php::set_active(&runtime.version).unwrap();
            service.restart().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(400));

            let running = running_php_cgi_path();
            println!("switched to {} -> {}", runtime.version, running.trim());
            assert!(
                running.contains(&runtime.version),
                "after switching to {} the running php-cgi should be under it, got: {running}",
                runtime.version
            );
        }

        service.stop().unwrap();
        php::set_active(&original_active).unwrap();
    }

    /// Path of whatever `php-cgi.exe` is running right now, straight from
    /// the OS — the point is to check the real process, not our own record
    /// of what we think we spawned.
    fn running_php_cgi_path() -> String {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-Process -Name php-cgi -ErrorAction SilentlyContinue).Path",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// The full pipeline for real: a project folder on disk -> vhost
    /// generated -> nginx + php-cgi both started through the actual
    /// `ProcessService`/`vhosts` code (not the manual shell script this was
    /// prototyped with) -> curl through nginx with a `Host` header and get
    /// back real PHP output. Requires nginx and php installed. Run with:
    /// `cargo test --lib services::process::tests::vhost_pipeline_serves_a_real_project -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn vhost_pipeline_serves_a_real_project() {
        use crate::services::projects::www_root;
        use crate::services::vhosts::sync_vhosts;

        let project_dir = www_root().unwrap().join("rezure-pipeline-test");
        let _ = fs::remove_dir_all(&project_dir);
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(
            project_dir.join("index.php"),
            r#"<?php echo "pipeline ok: " . $_SERVER['REQUEST_URI'];"#,
        )
        .unwrap();

        sync_vhosts().expect("project must be detected and its vhost written");

        let php = ProcessService::php(no_op_sink()).unwrap();
        let nginx = ProcessService::nginx(no_op_sink()).unwrap();

        let cleanup = || {
            let _ = nginx.stop();
            let _ = php.stop();
            let _ = fs::remove_dir_all(&project_dir);
        };

        php.start()
            .expect("php-cgi must be installed to run this test");
        if let Err(err) = nginx.start() {
            cleanup();
            panic!("nginx failed to start: {err}");
        }

        // Give nginx a moment to finish binding before the first request.
        std::thread::sleep(std::time::Duration::from_millis(500));

        let body = Command::new("curl")
            .args([
                "-s",
                "-H",
                "Host: rezure-pipeline-test.test",
                "http://127.0.0.1/foo?x=1",
            ])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());

        cleanup();

        let body = body.expect("curl must be available to run this test");
        assert_eq!(body, "pipeline ok: /foo?x=1");
    }
}
