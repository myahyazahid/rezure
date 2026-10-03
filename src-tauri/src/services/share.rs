//! One-click public sharing of a local project via a Cloudflare Quick Tunnel.
//!
//! # Why cloudflared, and only cloudflared
//!
//! The feature's whole point is "share a project with nobody having to set
//! anything up first". ngrok's free tier now requires an account and an
//! authtoken before it will open a single tunnel, which breaks that promise
//! outright. `cloudflared tunnel --url <local>` needs neither — it prints a
//! random `https://<slug>.trycloudflare.com` URL that proxies straight back
//! to the local server, with no signup step at all.
//!
//! # Why cloudflared points at a local proxy, not nginx directly
//!
//! Every project shares one nginx process on port 80, routed by
//! `server_name` (`services::vhosts`), with a catch-all `default_server` for
//! anything that doesn't match (see `vhosts::ensure_main_config`). Whatever
//! reaches nginx has to carry the project's real domain (`fmt-app.test`) as
//! `Host`, or nginx sends it to the catch-all instead of the project that
//! was actually shared.
//!
//! cloudflared has its own flag for exactly that (`--http-host-header`), and
//! an earlier version of this module used it directly. The problem it
//! doesn't solve: most frameworks build absolute redirect URLs from that
//! same Host header (Laravel's `url()`/`redirect()` among them), so a
//! `302 Location: http://fmt-app.test/login` reaches the browser completely
//! unchanged — and `fmt-app.test` resolves to nothing outside this machine.
//! That is what a phone or a friend's laptop actually saw:
//! `DNS_PROBE_FINISHED_NXDOMAIN` on `fmt-app.test`, not on the tunnel URL.
//!
//! [`super::share_proxy`] is inserted between cloudflared and nginx
//! specifically to fix that one case: it sets the `Host` header nginx needs
//! (cloudflared no longer does, and drops `--http-host-header`), and rewrites
//! only a `Location` header naming the project's own domain to the tunnel's
//! public hostname instead. See that module for why a redirect header is the
//! one thing worth fixing here, and what's deliberately left alone.
//!
//! # Why this isn't a `Service`
//!
//! [`super::Service`] models one long-lived instance per service *type*,
//! started once at app launch (nginx, PHP, the database). A share is
//! per-project, ephemeral, and started on demand from a button — the same
//! shape as the existing SSH tunnels in [`super::tunnel`], not a
//! `ServiceManager` entry. This module mirrors that one's registry and
//! `Drop`-kills-the-child design directly, with cloudflared standing in for
//! `ssh.exe`. It is a separate module (and a separate `AppError` variant)
//! rather than an extension of `tunnel.rs` because the two are unrelated
//! concepts that happen to share a shape: one forwards a remote database
//! *into* Rezure, this one exposes a local project *out* to the internet.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use tauri::AppHandle;
use tokio::time::Instant;

use super::binaries;
use super::projects;
use super::share_proxy::{self, Proxy};
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

/// Pinned like a `binaries::MANIFEST` entry (checksum-verified download,
/// never trusted unchecked), but kept local rather than added to that
/// manifest: cloudflared ships one relevant Windows build, not a family of
/// installable versions, so the `BinaryPackage`/family machinery built for
/// PHP would be unused weight here. Re-verify and bump these together when
/// updating — checked against the real GitHub release at
/// <https://github.com/cloudflare/cloudflared/releases/tag/2026.9.1>.
const CLOUDFLARED_VERSION: &str = "2026.9.1";
const CLOUDFLARED_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/download/2026.9.1/cloudflared-windows-amd64.exe";
const CLOUDFLARED_SHA256: &str = "2837888cc0f5d58f15b6dc478376de90b4d3ba5241c7947455d1e0a0df429712";

/// How long to wait for cloudflared to print its `trycloudflare.com` URL.
/// Covers the download-less common case (binary already installed) plus the
/// tunnel's own handshake with Cloudflare's edge.
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// `C:\rezure\bin\cloudflared\<version>\cloudflared.exe` — same
/// `<family>/<version>` layout `binaries::install_root` uses for every other
/// downloaded runtime.
fn cloudflared_exe_path() -> Result<PathBuf, AppError> {
    Ok(binaries::install_root()?
        .join("cloudflared")
        .join(CLOUDFLARED_VERSION)
        .join("cloudflared.exe"))
}

/// Downloads and checksum-verifies cloudflared if it isn't already present.
/// Idempotent, like `binaries::install` — a share that's about to start
/// checks this first every time, and the common case after the first run is
/// "already there".
pub async fn ensure_installed(app: &AppHandle) -> Result<PathBuf, AppError> {
    let exe = cloudflared_exe_path()?;
    if exe.is_file() {
        return Ok(exe);
    }

    let bytes = binaries::download(app, "cloudflared", CLOUDFLARED_URL).await?;
    binaries::verify_checksum("cloudflared", CLOUDFLARED_SHA256, &bytes)?;

    let dir = exe
        .parent()
        .ok_or_else(|| AppError::Io("cloudflared install path has no parent".to_string()))?;
    std::fs::create_dir_all(dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    std::fs::write(&exe, &bytes)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", exe.display())))?;

    Ok(exe)
}

/// A running `cloudflared tunnel`, and the public URL it's serving.
///
/// Killed on drop, so a share can never outlive the thing that started it —
/// including on a panic, which is exactly when a leaked `cloudflared.exe`
/// still proxying to a project would be hardest to notice.
#[derive(Debug)]
struct Share {
    child: Child,
    log: PathBuf,
    url: String,
    /// Kept alive for as long as the tunnel is: dropping it stops the local
    /// rewrite proxy cloudflared forwards through. See `share_proxy`.
    _proxy: Proxy,
}

impl Drop for Share {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.log);
    }
}

fn registry() -> &'static Mutex<HashMap<String, Share>> {
    static SHARES: OnceLock<Mutex<HashMap<String, Share>>> = OnceLock::new();
    SHARES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn shares() -> MutexGuard<'static, HashMap<String, Share>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// The last few lines cloudflared wrote before giving up — same idea as
/// `tunnel::read_log`, ported rather than shared across modules for one
/// small, self-contained helper.
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

/// The first `https://*.trycloudflare.com` URL found in cloudflared's
/// startup log, if it has printed one yet.
fn find_quick_tunnel_url(log: &str) -> Option<String> {
    log.split(|c: char| c.is_whitespace() || c == '|')
        .find(|token| token.starts_with("https://") && token.contains(".trycloudflare.com"))
        .map(|url| url.trim_end_matches(['.', ',', ')']).to_string())
}

/// Starts sharing `project_id`, or returns the URL of a share already
/// running for it.
///
/// The project is re-resolved by id against a fresh scan (`projects::find`)
/// rather than trusting anything else from the frontend — the domain the
/// local proxy forces as `Host` has to be the real one nginx was actually
/// configured with.
pub async fn start(exe: &Path, project_id: &str) -> Result<String, AppError> {
    let project = projects::find(project_id)?;

    {
        let mut registry = shares();
        if let Some(existing) = registry.get_mut(project_id) {
            if matches!(existing.child.try_wait(), Ok(None)) {
                return Ok(existing.url.clone());
            }
            // Dropping it kills whatever is left of the dead tunnel before a
            // replacement is started.
            registry.remove(project_id);
        }
    }

    let proxy = share_proxy::spawn(project.domain.clone()).await?;

    let log_path = std::env::temp_dir().join(format!(
        "rezure-cloudflared-{}-{}.log",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    // stderr goes to a file, not a pipe: nothing reads this process while it
    // runs, and cloudflared keeps writing connection-health lines for as
    // long as the tunnel is up — an unread pipe would eventually fill and
    // block it.
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| AppError::ShareFailed(format!("could not create the log file: {e}")))?;

    // Points at the local rewrite proxy, not nginx directly — see the module
    // doc comment and `share_proxy` for why. The proxy sets `Host` itself,
    // so cloudflared no longer needs `--http-host-header`.
    let mut child = Command::new(exe)
        .args([
            "tunnel",
            "--url",
            &format!("http://127.0.0.1:{}", proxy.port),
        ])
        .arg("--no-autoupdate")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file))
        .hidden()
        .spawn()
        .map_err(|e| AppError::ShareFailed(format!("couldn't start cloudflared: {e}")))?;

    let deadline = Instant::now() + READY_TIMEOUT;
    let url = loop {
        // A dead cloudflared is the common failure mode here (no internet,
        // Cloudflare's edge unreachable) — its own log is the diagnosis.
        if let Ok(Some(_)) = child.try_wait() {
            let detail = read_log(&log_path);
            let _ = std::fs::remove_file(&log_path);
            return Err(AppError::ShareFailed(if detail.is_empty() {
                "cloudflared exited immediately".to_string()
            } else {
                detail
            }));
        }

        let log_contents = std::fs::read_to_string(&log_path).unwrap_or_default();
        if let Some(url) = find_quick_tunnel_url(&log_contents) {
            break url;
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let detail = read_log(&log_path);
            let _ = std::fs::remove_file(&log_path);
            return Err(AppError::ShareFailed(format!(
                "cloudflared didn't return a URL within {}s{}",
                READY_TIMEOUT.as_secs(),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" — {detail}")
                }
            )));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };

    // Now that the tunnel's own public hostname is known, the proxy can
    // start rewriting a `Location` header that names the project's domain
    // to point at it instead.
    if let Some(host) = url::Url::parse(&url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    {
        proxy.set_public_host(host);
    }

    shares().insert(
        project_id.to_string(),
        Share {
            child,
            log: log_path,
            url: url.clone(),
            _proxy: proxy,
        },
    );
    Ok(url)
}

/// The URL of a share already running for `project_id`, without starting
/// one — for the frontend to restore state after a page reload.
pub fn existing_url(project_id: &str) -> Option<String> {
    let mut registry = shares();
    let share = registry.get_mut(project_id)?;
    matches!(share.child.try_wait(), Ok(None)).then(|| share.url.clone())
}

/// Stops sharing one project, if it's being shared.
pub fn close(project_id: &str) {
    shares().remove(project_id);
}

/// Stops every active share — called on app exit, so no `cloudflared.exe`
/// outlives Rezure still proxying to a project that's no longer running.
pub fn close_all() {
    shares().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_share_for_an_unknown_project_is_refused_before_cloudflared_is_started() {
        let err = start(
            Path::new("cloudflared.exe"),
            "definitely-not-a-real-project-9f3a",
        )
        .await
        .expect_err("an unknown project id must be refused");
        assert!(matches!(err, AppError::ProjectNotFound(_)), "{err}");
    }

    #[test]
    fn closing_a_project_with_no_active_share_is_a_harmless_no_op() {
        close("no-such-project");
        assert!(existing_url("no-such-project").is_none());
    }

    #[test]
    fn the_log_tail_keeps_only_the_last_lines_in_order() {
        let path =
            std::env::temp_dir().join(format!("rezure-share-log-test-{}", std::process::id()));
        std::fs::write(&path, "first\n\nsecond\nthird\nfourth\n").unwrap();
        assert_eq!(read_log(&path), "second; third; fourth");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_quick_tunnel_url_is_found_among_ordinary_log_lines() {
        let log = "2026-09-12T00:00:00Z INF Thank you for trying Cloudflare Tunnel\n\
                    2026-09-12T00:00:01Z INF |  https://random-words-here.trycloudflare.com  |\n";
        assert_eq!(
            find_quick_tunnel_url(log).as_deref(),
            Some("https://random-words-here.trycloudflare.com")
        );
    }

    #[test]
    fn no_url_is_found_before_cloudflared_has_printed_one() {
        let log = "2026-09-12T00:00:00Z INF Starting tunnel\n";
        assert!(find_quick_tunnel_url(log).is_none());
    }
}
