//! SSH tunnels for remote connections whose database port isn't reachable
//! directly.
//!
//! # Why shell out to `ssh.exe`
//!
//! Windows 10 and 11 ship OpenSSH, and it already solves everything an
//! embedded SSH library would make Rezure re-solve: key formats (PEM,
//! OpenSSH, ed25519), `~/.ssh/config`, `known_hosts`, ssh-agent, keepalives,
//! and every server-side quirk that has accumulated since 1999. Linking a
//! Rust SSH stack would add a large dependency whose job is to be *almost*
//! as compatible as the binary already installed.
//!
//! # How a password reaches `ssh.exe`
//!
//! Not on the command line, and not on stdin — `ssh` refuses both by
//! design. It asks a helper program named by `SSH_ASKPASS`, and
//! `OpenSSH_for_Windows_9.5p2` honours that (`SSH_ASKPASS_REQUIRE=force`
//! makes it do so even with a console attached).
//!
//! The helper is *this same executable*, re-entered through
//! [`serve_askpass`] before Tauri starts. A second binary would have to be
//! bundled, signed and located at runtime; the one already running needs
//! none of that.
//!
//! The password reaches the helper in the environment of the `ssh` child —
//! set per-spawn, never on Rezure's own process, so no other child (a
//! `mysqld`, a `php-cgi`) ever carries it. That environment is readable by
//! other processes running as the same user, which is the same audience
//! that can already read Credential Manager, and strictly better than an
//! argument list that every process listing displays.
//!
//! Key authentication is still the better answer and the one the UI offers
//! first: a key with a passphrase can't be used, because that passphrase
//! would need the same treatment and Rezure never asks for one.
//!
//! # Why the rest of the app doesn't know this exists
//!
//! A live tunnel is just a port on 127.0.0.1. `database::remote_conn` swaps
//! the host and port for it and everything downstream — queries, dumps,
//! imports, the handoff to DBeaver — runs completely unchanged. That is the
//! whole reason this was built last rather than first.

use std::collections::HashMap;
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::secrets;
use crate::config::connections::{Connection, SshAuth, SshTunnel};
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

/// Windows' own OpenSSH client, by absolute path.
///
/// Preferred over whatever `ssh` resolves to on PATH: Git for Windows and
/// several other tools ship their own build, and picking one by accident
/// means a different `known_hosts` and a different idea of where the user's
/// keys live than the one they configured.
const SYSTEM_SSH: &str = r"C:\Windows\System32\OpenSSH\ssh.exe";

/// How long to wait for the forwarded port to start accepting connections.
/// Covers the TCP connect, the key exchange and authentication.
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// Set on the `ssh` child when Rezure is answering its password prompt.
/// Both must be present for [`serve_askpass`] to reply, so launching
/// Rezure with a stray argument can never turn it into a credential
/// printer.
const ASKPASS_FLAG: &str = "REZURE_SSH_ASKPASS";
const ASKPASS_PASSWORD: &str = "REZURE_SSH_PASSWORD";

/// Answers `ssh`'s password prompt, when this process was started by `ssh`
/// as its `SSH_ASKPASS` helper.
///
/// Returns `true` when it handled the invocation, which is the caller's cue
/// to exit without starting the app.
///
/// `ssh` passes the prompt text as an argument and reads the answer from
/// stdout — that is the entire protocol.
pub fn serve_askpass() -> bool {
    if std::env::var(ASKPASS_FLAG).as_deref() != Ok("1") {
        return false;
    }
    let Ok(password) = std::env::var(ASKPASS_PASSWORD) else {
        return false;
    };
    // A GUI-subsystem process still writes to the pipe `ssh` handed it, so
    // this reaches the parent even with no console attached. Written and
    // flushed explicitly rather than via `println!`, whose panic-on-error
    // behaviour would surface as a silent exit code here.
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(password.as_bytes());
    let _ = out.write_all(b"\n");
    let _ = out.flush();
    true
}

/// A running `ssh -L`, and the local port it forwards.
///
/// Killed on drop, so a tunnel can never outlive the thing that wanted it —
/// including on a panic, which is exactly when a leaked `ssh.exe` holding a
/// port would be hardest to explain.
#[derive(Debug)]
pub struct Tunnel {
    child: Child,
    log: PathBuf,
    pub local_port: u16,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.log);
    }
}

fn registry() -> &'static Mutex<HashMap<String, Tunnel>> {
    static TUNNELS: OnceLock<Mutex<HashMap<String, Tunnel>>> = OnceLock::new();
    TUNNELS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn tunnels() -> MutexGuard<'static, HashMap<String, Tunnel>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

fn ssh_exe() -> PathBuf {
    let system = PathBuf::from(SYSTEM_SSH);
    if system.is_file() {
        system
    } else {
        // Every supported Windows build ships OpenSSH, but an image with the
        // optional feature removed still works if something else is on PATH.
        PathBuf::from("ssh")
    }
}

/// A free loopback port for the forward's local end.
///
/// Asking the OS for port 0 and reading back what it assigned is the only
/// race-free way to pick one; the listener is dropped immediately, so there
/// is a moment where another process could take it. `ExitOnForwardFailure`
/// turns that into a clean startup error rather than a tunnel that appears
/// to be up and forwards nothing.
fn free_local_port() -> Result<u16, AppError> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .map_err(|e| AppError::TunnelFailed(format!("no local port was available: {e}")))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|e| AppError::TunnelFailed(format!("no local port was available: {e}")))
}

/// Whether the forwarded port is accepting connections yet.
fn port_is_open(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// The last few lines `ssh` wrote before giving up — "Permission denied
/// (publickey)" and friends, which say far more than an exit code.
fn read_log(path: &Path) -> String {
    let mut buffer = String::new();
    if let Ok(mut file) = std::fs::File::open(path) {
        let _ = file.read_to_string(&mut buffer);
    }
    let tail: Vec<&str> = buffer
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(3)
        .collect();
    tail.into_iter().rev().collect::<Vec<_>>().join("; ")
}

/// Starts `ssh -N -L <free>:<db_host>:<db_port>` and waits for the forward
/// to come up.
///
/// `db_host` and `db_port` are resolved **on the SSH server**, which is why
/// they are almost always `127.0.0.1` and the database's real port: the
/// point of a tunnel is reaching something that only listens locally there.
pub fn open(
    ssh: &SshTunnel,
    ssh_password: Option<&str>,
    db_host: &str,
    db_port: u16,
) -> Result<Tunnel, AppError> {
    // Both failure modes are caught here, before a process is started, so
    // they read as "you left something out" rather than as an SSH error.
    if let SshAuth::Key { path } = &ssh.auth {
        if !Path::new(path).is_file() {
            return Err(AppError::TunnelFailed(format!(
                "no SSH key at {path} — pick the private key file itself, not a password or a \
                 folder"
            )));
        }
    }
    if matches!(ssh.auth, SshAuth::Password) && ssh_password.unwrap_or_default().is_empty() {
        return Err(AppError::TunnelFailed(format!(
            "no SSH password saved for {}@{}",
            ssh.user, ssh.host
        )));
    }

    let local_port = free_local_port()?;
    let log_path = std::env::temp_dir().join(format!(
        "rezure-ssh-{}-{}.log",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    // stderr goes to a file rather than a pipe: nothing reads this process
    // while it runs, and an unread pipe fills its buffer and blocks `ssh`
    // mid-session — a tunnel that dies after a few minutes of use.
    let log = std::fs::File::create(&log_path)
        .map_err(|e| AppError::TunnelFailed(format!("could not create the ssh log: {e}")))?;

    let mut command = Command::new(ssh_exe());
    command
        .args(["-N", "-T"])
        .args(["-p", &ssh.port.to_string()]);

    match &ssh.auth {
        SshAuth::Key { path } => {
            command
                .arg("-i")
                .arg(path)
                // No prompt is answerable from a GUI process, so anything
                // that would ask must fail instead of hanging. This also
                // rules out a passphrase-protected key.
                .args(["-o", "BatchMode=yes"])
                // Without this a server offering both would fall back to
                // asking for a password after the key is rejected.
                .args(["-o", "PreferredAuthentications=publickey"])
                .args(["-o", "IdentitiesOnly=yes"]);
        }
        SshAuth::Password => {
            // BatchMode is deliberately *not* set: it disables askpass, and
            // askpass is the entire mechanism here.
            command
                .args(["-o", "PreferredAuthentications=password"])
                // One attempt. A wrong password would otherwise be offered
                // three times, tripling the wait before the error appears.
                .args(["-o", "NumberOfPasswordPrompts=1"])
                .env("SSH_ASKPASS", std::env::current_exe().unwrap_or_default())
                // Without `force`, ssh only consults askpass when it has no
                // console to ask on — which is not a property this process
                // can rely on in a dev build.
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env(ASKPASS_FLAG, "1")
                .env(ASKPASS_PASSWORD, ssh_password.unwrap_or_default());
        }
    }

    let child = command
        // A first connection to an unknown host would otherwise stop on the
        // fingerprint question, which nothing here can answer.
        .args(["-o", "StrictHostKeyChecking=accept-new"])
        // Fail loudly if the local port can't be bound, instead of running
        // an SSH session that forwards nothing.
        .args(["-o", "ExitOnForwardFailure=yes"])
        // Keeps the session alive through a NAT or firewall idle timeout —
        // otherwise a tunnel silently dies between two exports.
        .args(["-o", "ServerAliveInterval=30"])
        .args(["-o", "ServerAliveCountMax=3"])
        .args(["-o", "ConnectTimeout=10"])
        .arg("-L")
        .arg(format!("127.0.0.1:{local_port}:{db_host}:{db_port}"))
        .arg(format!("{}@{}", ssh.user, ssh.host))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .hidden()
        .spawn()
        .map_err(|e| AppError::TunnelFailed(format!("couldn't start ssh: {e}")))?;

    let mut tunnel = Tunnel {
        child,
        log: log_path,
        local_port,
    };

    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        // A dead ssh is the common failure — a rejected key, an unreachable
        // host — and its own message is the diagnosis.
        if let Ok(Some(_)) = tunnel.child.try_wait() {
            let detail = read_log(&tunnel.log);
            return Err(AppError::TunnelFailed(if detail.is_empty() {
                format!("ssh to {}@{} exited immediately", ssh.user, ssh.host)
            } else {
                detail
            }));
        }
        if port_is_open(local_port) {
            return Ok(tunnel);
        }
        if Instant::now() >= deadline {
            let detail = read_log(&tunnel.log);
            return Err(AppError::TunnelFailed(format!(
                "the tunnel to {}@{} didn't come up within {}s{}",
                ssh.user,
                ssh.host,
                READY_TIMEOUT.as_secs(),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" — {detail}")
                }
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The local port for `connection`'s tunnel, starting one if needed.
///
/// Reuses a tunnel that is still alive, so a session of exports pays the
/// SSH handshake once rather than per operation.
pub fn ensure(connection: &Connection) -> Result<u16, AppError> {
    let ssh_password = secrets::resolve(&secrets::ssh_id(&connection.id));
    let ssh = connection
        .ssh
        .as_ref()
        .ok_or_else(|| AppError::TunnelFailed("this connection has no SSH settings".to_string()))?;

    let mut tunnels = tunnels();
    if let Some(existing) = tunnels.get_mut(&connection.id) {
        let alive = matches!(existing.child.try_wait(), Ok(None));
        if alive && port_is_open(existing.local_port) {
            return Ok(existing.local_port);
        }
        // Dropping it kills whatever is left of the dead session before a
        // replacement is started.
        tunnels.remove(&connection.id);
    }

    let tunnel = open(
        ssh,
        ssh_password.as_deref(),
        &connection.host,
        connection.port,
    )?;
    let port = tunnel.local_port;
    tunnels.insert(connection.id.clone(), tunnel);
    Ok(port)
}

/// The local port of a tunnel that is already running, without starting
/// one. For callers that can't fail, like the endpoint shown in the UI.
pub fn existing_port(connection_id: &str) -> Option<u16> {
    let mut tunnels = tunnels();
    let tunnel = tunnels.get_mut(connection_id)?;
    matches!(tunnel.child.try_wait(), Ok(None)).then_some(tunnel.local_port)
}

/// Closes the tunnel for one connection, if it has one.
pub fn close(connection_id: &str) {
    tunnels().remove(connection_id);
}

/// Closes every tunnel — called on app exit, so no `ssh.exe` outlives
/// Rezure holding a forwarded port.
pub fn close_all() {
    tunnels().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_port_is_actually_free() {
        let port = free_local_port().expect("a port must be available");
        assert!(port > 0);
        assert!(!port_is_open(port), "the probe listener must be released");
    }

    #[test]
    fn an_open_port_is_detected() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_is_open(port));
    }

    fn tunnel_with(auth: SshAuth) -> SshTunnel {
        SshTunnel {
            host: "example.com".to_string(),
            port: 22,
            user: "deploy".to_string(),
            auth,
        }
    }

    /// The mistake this was written for: a password typed into the key box.
    #[test]
    fn a_key_path_that_is_not_a_file_is_refused_before_ssh_is_started() {
        let ssh = tunnel_with(SshAuth::Key {
            path: "##ABRSk9dz6eH2".to_string(),
        });
        let err =
            open(&ssh, None, "127.0.0.1", 3306).expect_err("a non-existent key must be refused");
        let message = err.to_string();
        assert!(message.contains("no SSH key at"), "{message}");
        assert!(message.contains("not a password"), "{message}");
    }

    #[test]
    fn password_auth_without_a_password_is_refused_before_ssh_is_started() {
        let ssh = tunnel_with(SshAuth::Password);
        let err = open(&ssh, None, "127.0.0.1", 3306).expect_err("a missing password is refused");
        assert!(err.to_string().contains("no SSH password"), "{err}");
    }

    /// Both guards must hold, so that starting Rezure normally — or with a
    /// stray argument — can never print a credential.
    #[test]
    fn the_askpass_helper_stays_dormant_without_both_signals() {
        std::env::remove_var(ASKPASS_FLAG);
        std::env::remove_var(ASKPASS_PASSWORD);
        assert!(!serve_askpass(), "no signals at all");

        std::env::set_var(ASKPASS_FLAG, "1");
        assert!(!serve_askpass(), "the flag alone must not be enough");
        std::env::remove_var(ASKPASS_FLAG);

        std::env::set_var(ASKPASS_PASSWORD, "s3cret");
        assert!(!serve_askpass(), "a password alone must not be enough");
        std::env::remove_var(ASKPASS_PASSWORD);
    }

    #[test]
    fn the_log_tail_keeps_only_the_last_lines_in_order() {
        let path = std::env::temp_dir().join(format!("rezure-log-test-{}", std::process::id()));
        std::fs::write(&path, "first\n\nsecond\nthird\nfourth\n").unwrap();
        assert_eq!(read_log(&path), "second; third; fourth");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_connection_without_ssh_settings_cannot_be_tunnelled() {
        let connection = Connection {
            id: "c1".to_string(),
            name: "Direct".to_string(),
            host: "db.example.com".to_string(),
            port: 3306,
            user: "app".to_string(),
            engine: crate::services::db_engine::Engine::MySql,
            tls_mode: Default::default(),
            read_only: true,
            save_password: false,
            ssh: None,
            last_used_at: None,
        };
        assert!(ensure(&connection).is_err());
    }
}
