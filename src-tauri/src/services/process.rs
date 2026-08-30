//! Real, process-backed [`Service`] implementation — spawns a service's
//! portable binary (see [`crate::services::binaries`]) and tracks it with
//! `sysinfo`.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::{AppHandle, Emitter};

use super::binaries::{self, BinaryPackage};
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
    /// `mysqld --datadir=<dir> --port=<port>`; the data directory is
    /// bootstrapped with `mariadb-install-db` the first time it's started.
    MariaDb { data_dir: PathBuf },
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

    pub fn mariadb(log_sink: LogSink) -> Result<Self, AppError> {
        binaries::find("mariadb")?;
        Ok(Self {
            id: "mariadb",
            name: "MariaDB",
            category: "Database",
            port: 3306,
            launch: Launch::MariaDb {
                data_dir: runtime_dir("mariadb")?.join("data"),
            },
            log_sink,
            child: Mutex::new(None),
            sys: Mutex::new(System::new()),
            cpu_history: Mutex::new(Vec::new()),
        })
    }

    /// Resolves which binary this service currently runs. Fixed for
    /// nginx/MariaDB (one version each); for PHP this follows whatever
    /// `services::php` currently reports as active, so a version switch
    /// takes effect the next time the service starts.
    fn package(&self) -> Result<&'static BinaryPackage, AppError> {
        match self.launch {
            Launch::Nginx => binaries::find("nginx"),
            Launch::Php => super::php::active_package(),
            Launch::MariaDb { .. } => binaries::find("mariadb"),
        }
    }

    /// The binary that actually ends up running — `php` is the one case
    /// where this differs from `binaries::exe_path`: the manifest points at
    /// `php.exe`, but what gets spawned is `php-cgi.exe`, which ships
    /// alongside it in the same zip. Shared by `command()` (to build the
    /// spawn) and `reap_orphan()` (to recognize a leftover from a previous
    /// run), so the two can never drift apart.
    fn resolved_exe(&self) -> Result<PathBuf, AppError> {
        let exe = binaries::exe_path(self.package()?)?;
        match self.launch {
            Launch::Php => Ok(exe
                .parent()
                .ok_or_else(|| AppError::Io("php.exe has no parent directory".to_string()))?
                .join("php-cgi.exe")),
            Launch::Nginx | Launch::MariaDb { .. } => Ok(exe),
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
            Launch::MariaDb { data_dir } => {
                ensure_mariadb_data_dir(&exe, data_dir)?;
                let mut cmd = Command::new(&exe);
                cmd.arg(format!("--datadir={}", data_dir.display()))
                    .arg(format!("--port={}", self.port))
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
            Launch::Php | Launch::MariaDb { .. } => "127.0.0.1",
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

/// Bootstraps MariaDB's system tables via `mariadb-install-db` the first
/// time it's started — `mysqld` refuses to run against an empty datadir.
fn ensure_mariadb_data_dir(exe: &Path, data_dir: &Path) -> Result<(), AppError> {
    let already_initialized = data_dir
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    if already_initialized {
        return Ok(());
    }

    fs::create_dir_all(data_dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", data_dir.display())))?;

    let bin_dir = exe
        .parent()
        .ok_or_else(|| AppError::ProcessBootstrapFailed {
            name: "MariaDB".to_string(),
            reason: "could not locate the mariadb bin directory".to_string(),
        })?;
    let installer = bin_dir.join("mariadb-install-db.exe");

    // Older Windows builds of `mariadb-install-db` (unlike the Linux
    // installer) don't support `--auth-root-authentication-method` — root
    // is created passwordless and localhost-only by default, which is fine
    // for a local dev database.
    let output = Command::new(&installer)
        .current_dir(bin_dir)
        .arg(format!("--datadir={}", data_dir.display()))
        .output()
        .map_err(|e| AppError::ProcessBootstrapFailed {
            name: "MariaDB".to_string(),
            reason: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(AppError::ProcessBootstrapFailed {
            name: "MariaDB".to_string(),
            reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(())
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

        // Best-effort — `package()` only fails if a family has zero
        // manifest entries, which the constructors already guard against.
        let version = self
            .package()
            .map(|pkg| pkg.version.to_string())
            .unwrap_or_default();

        ServiceInfo {
            id: self.id.to_string(),
            name: self.name.to_string(),
            category: self.category.to_string(),
            status: if pid.is_some() {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            },
            version,
            port: self.port,
            cpu_percent,
            cpu_history: self.cpu_history.lock().unwrap().clone(),
        }
    }

    fn start(&self) -> Result<ServiceInfo, AppError> {
        if self.poll_pid().is_some() {
            return Ok(self.info());
        }

        if !binaries::is_installed(self.package()?) {
            return Err(AppError::BinaryNotInstalled(self.name.to_string()));
        }

        self.reap_orphan()?;
        ensure_port_available(self.bind_addr(), self.port, self.name)?;

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

    fn stop(&self) -> Result<ServiceInfo, AppError> {
        let mut child_guard = self.child.lock().unwrap();
        if let Some(mut child) = child_guard.take() {
            kill_process_tree(child.id());
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

    /// Switches the active PHP version and confirms the *actual spawned
    /// process* — not just what the code claims — points at that
    /// version's own `php-cgi.exe`. Needs at least two PHP versions
    /// installed. Run with:
    /// `cargo test --lib services::process::tests::switching_php_version_changes_which_binary_actually_runs -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn switching_php_version_changes_which_binary_actually_runs() {
        use crate::services::php;

        let versions = crate::services::binaries::family_packages("php");
        let installed: Vec<_> = versions
            .iter()
            .filter(|pkg| crate::services::binaries::is_installed(pkg))
            .collect();
        assert!(
            installed.len() >= 2,
            "need at least 2 installed PHP versions to run this test"
        );

        let original_active = php::active_id();

        for pkg in &installed {
            php::set_active(pkg.id).unwrap();

            let service = ProcessService::php(no_op_sink()).unwrap();
            service.start().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));

            let output = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "(Get-Process -Name php-cgi -ErrorAction SilentlyContinue).Path",
                ])
                .output()
                .unwrap();
            let running_path = String::from_utf8_lossy(&output.stdout).to_string();

            service.stop().unwrap();

            assert!(
                running_path.contains(pkg.version),
                "expected the running php-cgi to be under {}, got: {running_path}",
                pkg.version
            );
        }

        php::set_active(&original_active).unwrap();
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
