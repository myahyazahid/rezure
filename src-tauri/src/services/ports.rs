//! Who is holding a port, and whether Rezure may take it back.
//!
//! "Port already in use" is the most common reason a service won't start,
//! and on its own it's a dead end — the user is told to stop something
//! without being told *what*. This module answers that, and classifies the
//! answer, because the right action differs sharply:
//!
//! - Rezure's own leftover process is safe to kill; it's usually an orphan
//!   from a previous run that outlived the app (see
//!   `process::ProcessService::reap_orphan` for the case that's meant to
//!   catch, and can't always).
//! - Another developer tool is the user's call, with a real name attached.
//! - A system process must never be killed, and saying so is more useful
//!   than failing to.

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::binaries;
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

/// What kind of thing is holding the port, which decides what Rezure offers
/// to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HolderKind {
    /// A process running one of Rezure's own binaries — almost always an
    /// orphan of a previous run. Safe to reclaim.
    Rezure,
    /// Someone else's process. Killable, but only the user can say whether
    /// it should be.
    Foreign,
    /// The kernel, a driver, or a process with no readable path — `System`
    /// (pid 4) holds port 80 whenever IIS or the Windows HTTP service is
    /// enabled. Killing it is impossible and asking would be misleading.
    System,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortHolder {
    pub port: u16,
    pub pid: u32,
    /// Process name, e.g. `nginx.exe` — what the user would recognize.
    pub name: String,
    /// Full path when readable, so a foreign process can be identified as
    /// Laragon's or XAMPP's rather than just "nginx".
    pub path: Option<String>,
    pub kind: HolderKind,
    /// A sentence naming the holder, ready to show.
    pub description: String,
}

impl PortHolder {
    /// Whether Rezure will offer to stop it. False for system processes,
    /// which can't be killed and shouldn't be suggested.
    pub fn reclaimable(&self) -> bool {
        self.kind != HolderKind::System
    }
}

/// Every listening TCP socket, as `(port, pid)`, via `netstat -ano`.
///
/// `netstat` rather than a Windows API call: it needs no new dependency,
/// and it's the same "shell out to a tool that ships with Windows" approach
/// `process::kill_process_tree` already takes with `taskkill`.
pub(crate) fn listening_sockets() -> Vec<(u16, u32)> {
    let Ok(output) = Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .hidden()
        .output()
    else {
        return Vec::new();
    };
    parse_listening(&String::from_utf8_lossy(&output.stdout))
}

fn parse_listening(text: &str) -> Vec<(u16, u32)> {
    let mut sockets = Vec::new();
    for line in text.lines() {
        // Every field is optional per line, and a missing one means "not a
        // connection row" — `netstat` opens with a blank line and a header.
        // These must `continue`, never `?`: returning on the first short
        // line would abandon the scan before reaching any data at all.
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [_proto, local, _foreign, state, pid] = fields[..] else {
            continue;
        };
        if !state.eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        // `local` is `0.0.0.0:80`, `127.0.0.1:80` or `[::]:80` — the port is
        // whatever follows the last colon, so an IPv6 address doesn't
        // confuse the split.
        let Some((_, bound_port)) = local.rsplit_once(':') else {
            continue;
        };
        if let (Ok(port), Ok(pid)) = (bound_port.parse::<u16>(), pid.parse::<u32>()) {
            sockets.push((port, pid));
        }
    }
    sockets
}

/// The PID listening on `port`.
fn listening_pid(port: u16) -> Option<u32> {
    listening_sockets()
        .into_iter()
        .find(|(bound, _)| *bound == port)
        .map(|(_, pid)| pid)
}

/// The ports `root` and every process it started are listening on, sorted.
///
/// What tells a dev server that came up on the wrong port apart from one
/// that's still starting: `npm` runs the real server as a grandchild, so
/// `root` itself never listens on anything.
pub fn ports_held_by_tree(root: u32) -> Vec<u16> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());

    let mut tree = std::collections::HashSet::from([root]);
    // Bounded: a recycled pid could in principle close a loop.
    for _ in 0..16 {
        let before = tree.len();
        for (pid, process) in sys.processes() {
            if process
                .parent()
                .is_some_and(|parent| tree.contains(&parent.as_u32()))
            {
                tree.insert(pid.as_u32());
            }
        }
        if tree.len() == before {
            break;
        }
    }

    let mut ports: Vec<u16> = listening_sockets()
        .into_iter()
        .filter(|(_, pid)| tree.contains(pid))
        .map(|(port, _)| port)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Identifies whoever is listening on `port`, or `None` if it's free.
pub fn holder(port: u16) -> Option<PortHolder> {
    let pid = listening_pid(port)?;

    // pid 0 (Idle) and 4 (System) are the kernel. `System` is what owns a
    // port handed to `http.sys`, i.e. IIS or the Windows HTTP service.
    if pid <= 4 {
        return Some(PortHolder {
            port,
            pid,
            name: "System".to_string(),
            path: None,
            kind: HolderKind::System,
            description: format!(
                "port {port} is reserved by Windows itself — usually IIS or the World Wide Web \
                 Publishing service. Stop that service to free it."
            ),
        });
    }

    let mut sys = System::new();
    let sys_pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sys_pid]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );

    let process = sys.process(sys_pid);
    let name = process
        .map(|p| p.name().to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let path = process
        .and_then(|p| p.exe())
        .map(|exe| exe.display().to_string());

    let kind = match &path {
        // No readable path means a protected process — not something to
        // offer to kill.
        None => HolderKind::System,
        Some(path) if is_rezure_binary(path) => HolderKind::Rezure,
        Some(_) => HolderKind::Foreign,
    };

    let description = match kind {
        HolderKind::Rezure => format!(
            "port {port} is held by a leftover Rezure process ({name}, pid {pid}) from an earlier \
             run"
        ),
        HolderKind::Foreign => match label_for(path.as_deref()) {
            Some(tool) => format!("port {port} is held by {tool} ({name}, pid {pid})"),
            None => format!("port {port} is held by {name} (pid {pid})"),
        },
        HolderKind::System => {
            format!("port {port} is held by a protected system process (pid {pid})")
        }
    };

    Some(PortHolder {
        port,
        pid,
        name,
        path,
        kind,
        description,
    })
}

/// Whether `path` is one of Rezure's own downloaded binaries — the test for
/// "this is our orphan, not somebody's running server".
fn is_rezure_binary(path: &str) -> bool {
    let path = path.to_lowercase();
    [binaries::install_root(), binaries::user_bin_root()]
        .into_iter()
        .flatten()
        .any(|root| path.starts_with(&root.display().to_string().to_lowercase()))
}

/// Names the tool a foreign process belongs to, so the user recognizes it
/// as *theirs* rather than as an anonymous `httpd.exe`.
fn label_for(path: Option<&str>) -> Option<&'static str> {
    let path = path?.to_lowercase();
    const TOOLS: &[(&str, &str)] = &[
        (r"\laragon", "Laragon"),
        (r"\xampp", "XAMPP"),
        (r"\wamp", "WAMP"),
        (r"\mamp", "MAMP"),
        (r"\docker", "Docker"),
    ];
    TOOLS
        .iter()
        .find(|(marker, _)| path.contains(marker))
        .map(|(_, label)| *label)
}

/// Kills whatever is listening on `port`.
///
/// Refuses a system process outright rather than failing obscurely, and
/// re-reads the holder instead of trusting a pid the caller passed in — the
/// process may have exited on its own between being shown and being killed,
/// and a recycled pid would mean killing something unrelated.
pub fn reclaim(port: u16) -> Result<(), AppError> {
    let Some(holder) = holder(port) else {
        // Already free — the thing the caller wanted is true.
        return Ok(());
    };

    if !holder.reclaimable() {
        return Err(AppError::PortHolderProtected {
            port,
            reason: holder.description,
        });
    }

    super::process::kill_process_tree(tree_root(holder.pid));

    // Confirm rather than assume: `taskkill` reporting success doesn't mean
    // the socket is released yet, and a still-held port would otherwise show
    // up as a confusing second failure on the retry.
    for _ in 0..25 {
        if listening_pid(port).is_none() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Err(AppError::PortHolderProtected {
        port,
        reason: format!("{} did not release port {port}", holder.name),
    })
}

/// The process to kill so that `pid` stays gone: the topmost ancestor still
/// running the same binary as `pid`.
///
/// The process holding a port isn't always the one that matters. nginx's
/// worker holds port 80, but its master respawns a killed worker at once, so
/// killing the holder alone freed nothing and the port came back under a
/// new pid on every retry. MySQL 8 on Windows is the same shape: a monitor
/// `mysqld` whose child `mysqld` does the listening. Climbing only while the
/// binary matches stops at whatever launched the server (Rezure, a shell,
/// Laragon), and it also skips a recycled parent pid that now belongs to
/// something unrelated.
fn tree_root(pid: u32) -> u32 {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );

    let mut current = Pid::from_u32(pid);
    // Bounded: two recycled pids could in principle name each other as
    // parent, and this must not spin on that.
    for _ in 0..8 {
        let Some(process) = sys.process(current) else {
            break;
        };
        let Some(parent) = process.parent().and_then(|ppid| sys.process(ppid)) else {
            break;
        };
        match (process.exe(), parent.exe()) {
            (Some(exe), Some(parent_exe)) if exe == parent_exe => current = parent.pid(),
            _ => break,
        }
    }
    current.as_u32()
}

/// The path a `PortHolder` reports, as a `PathBuf`, for callers comparing
/// against a resolved binary.
#[allow(dead_code)]
pub fn holder_path(holder: &PortHolder) -> Option<PathBuf> {
    holder.path.as_ref().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn netstat_rows_are_parsed_and_everything_else_skipped() {
        let text = "
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:80             0.0.0.0:0              LISTENING       4120
  TCP    [::1]:5173             [::]:0                 LISTENING       912
  TCP    127.0.0.1:51000        127.0.0.1:80           ESTABLISHED     77
";
        assert_eq!(parse_listening(text), vec![(80, 4120), (5173, 912)]);
    }

    /// The tree includes the root itself: this test process listening on a
    /// port of its own is found from its own pid.
    #[test]
    fn a_tree_reports_the_ports_its_processes_listen_on() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let held = ports_held_by_tree(std::process::id());
        drop(listener);
        assert!(held.contains(&port), "{held:?} should contain {port}");
    }

    #[test]
    fn a_free_port_has_no_holder() {
        // Bind and drop to find a port nothing wants.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        assert!(holder(port).is_none());
    }

    /// The port this test process itself is listening on has to come back
    /// with this process's own pid — that's the whole lookup working.
    #[test]
    fn a_bound_port_reports_the_process_holding_it() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let found = holder(port);
        drop(listener);

        let found = found.expect("a bound port must report a holder");
        assert_eq!(found.pid, std::process::id());
        assert_eq!(found.port, port);
        assert!(
            found.description.contains(&port.to_string()),
            "the description should name the port, got: {}",
            found.description
        );
    }

    /// Reclaiming a port nobody holds is the state the caller wanted, not
    /// an error — and it must not go looking for something to kill.
    #[test]
    fn reclaiming_a_free_port_is_a_no_op() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        assert!(reclaim(port).is_ok());
    }

    /// A child running the same binary as its parent resolves to the
    /// parent: the nginx master/worker shape, rebuilt from `cmd.exe`
    /// starting another `cmd.exe`, since that is on every Windows machine.
    #[test]
    fn a_same_binary_child_resolves_to_its_parent() {
        let mut outer = Command::new("cmd")
            .args(["/c", "cmd /c ping -n 30 127.0.0.1 >nul"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let outer_pid = Pid::from_u32(outer.id());

        let mut inner = None;
        for _ in 0..50 {
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            inner = sys
                .processes()
                .values()
                .find(|p| p.parent() == Some(outer_pid) && p.name().eq_ignore_ascii_case("cmd.exe"))
                .map(|p| p.pid().as_u32());
            if inner.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let root = inner.map(tree_root);
        super::super::process::kill_process_tree(outer.id());
        let _ = outer.wait();

        let inner = inner.expect("the inner cmd.exe never appeared");
        assert_ne!(inner, outer.id());
        assert_eq!(root, Some(outer.id()));
    }

    #[test]
    fn a_process_launched_by_another_binary_is_its_own_root() {
        // This test process was started by cargo (or a shell), not by
        // another copy of itself.
        assert_eq!(tree_root(std::process::id()), std::process::id());
    }

    #[test]
    fn known_tools_are_named_from_their_path() {
        assert_eq!(
            label_for(Some(r"C:\laragon\bin\nginx\nginx.exe")),
            Some("Laragon")
        );
        assert_eq!(
            label_for(Some(r"C:\xampp\apache\bin\httpd.exe")),
            Some("XAMPP")
        );
        assert_eq!(label_for(Some(r"C:\Program Files\thing\a.exe")), None);
        assert_eq!(label_for(None), None);
    }

    /// Rezure's own binaries have to be recognized as ours, or its orphans
    /// get treated as somebody else's running server.
    #[test]
    fn rezure_own_binaries_are_recognized() {
        let Ok(root) = binaries::install_root() else {
            return;
        };
        let ours = root.join("nginx").join("1.25.3").join("nginx.exe");
        assert!(is_rezure_binary(&ours.display().to_string()));
        assert!(!is_rezure_binary(r"C:\laragon\bin\nginx\nginx.exe"));
    }

    /// Reclaims a port from a leftover Rezure process, end to end, against
    /// the real machine.
    ///
    /// Refuses to touch anything that isn't ours: the point is to prove the
    /// orphan path, not to kill whatever happens to be listening. Run with:
    /// `cargo test --lib services::ports::tests::reclaim_a_real_rezure_orphan -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reclaim_a_real_rezure_orphan() {
        for port in [80, 3306, 9000] {
            let Some(found) = holder(port) else {
                println!("{port}: free");
                continue;
            };
            if found.kind != HolderKind::Rezure {
                println!("{port}: held by {:?} — leaving it alone", found.kind);
                continue;
            }

            println!("{port}: reclaiming {} (pid {})", found.name, found.pid);
            reclaim(port).expect("reclaiming our own orphan must succeed");
            assert!(
                holder(port).is_none(),
                "port {port} must be free once reclaimed"
            );
            println!("{port}: now free");
        }
    }

    /// What's really holding the ports Rezure wants. Run with:
    /// `cargo test --lib services::ports::tests::print_real_holders -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_real_holders() {
        for port in [80, 3306, 9000] {
            match holder(port) {
                Some(found) => println!(
                    "{port}: {:?} pid={} {} -> {}",
                    found.kind, found.pid, found.name, found.description
                ),
                None => println!("{port}: free"),
            }
        }
    }
}
