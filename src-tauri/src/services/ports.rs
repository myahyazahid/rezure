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

/// The PID listening on `port`, via `netstat -ano`.
///
/// `netstat` rather than a Windows API call: it needs no new dependency,
/// and it's the same "shell out to a tool that ships with Windows" approach
/// `process::kill_process_tree` already takes with `taskkill`.
fn listening_pid(port: u16) -> Option<u32> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);

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
        if bound_port.parse::<u16>() == Ok(port) {
            return pid.parse().ok();
        }
    }
    None
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

    super::process::kill_process_tree(holder.pid);

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
