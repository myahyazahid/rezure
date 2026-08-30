//! Manages Rezure's own block of entries in the Windows `hosts` file
//! (`C:\Windows\System32\drivers\etc\hosts`) — one `127.0.0.1  <domain>`
//! line per detected project, so `<project>.test` resolves in the browser.
//!
//! Only the block between the two marker comments is ever touched; every
//! other line — including anything the user added themselves — is
//! preserved byte-for-byte. Writing to the real file needs admin rights,
//! which Windows can only grant through an interactive UAC prompt a human
//! has to click through; this is deliberately never triggered
//! automatically (e.g. as a side effect of listing projects) — only from
//! an explicit `sync_hosts` command the user chooses to run.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::projects::scan_projects;
use crate::utils::error::AppError;

const BEGIN_MARKER: &str = "# --- Rezure managed entries (do not edit below) ---";
const END_MARKER: &str = "# --- Rezure managed entries end ---";

pub fn hosts_file_path() -> PathBuf {
    PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts")
}

/// `%LOCALAPPDATA%\Rezure\data\hosts` — staging area for the file this
/// module hands to the elevated copy step, and the tiny script that
/// performs it.
fn staging_dir() -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base.join("Rezure").join("data").join("hosts"))
}

/// Domains currently inside Rezure's managed block, if any.
fn managed_domains(content: &str) -> Vec<String> {
    let Some(start) = content.find(BEGIN_MARKER) else {
        return Vec::new();
    };
    let Some(end) = content[start..].find(END_MARKER) else {
        return Vec::new();
    };
    content[start + BEGIN_MARKER.len()..start + end]
        .lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            let mut parts = line.split_whitespace();
            if parts.next()? == "127.0.0.1" {
                parts.next().map(str::to_string)
            } else {
                None
            }
        })
        .collect()
}

/// Rebuilds a hosts file's content with Rezure's managed block replaced by
/// entries for `domains` (or removed entirely if `domains` is empty),
/// leaving every other line untouched.
fn rebuild_hosts_content(existing: &str, domains: &[String]) -> String {
    let mut lines: Vec<&str> = existing.lines().collect();

    if let (Some(start), Some(end)) = (
        lines.iter().position(|l| l.trim() == BEGIN_MARKER),
        lines.iter().position(|l| l.trim() == END_MARKER),
    ) {
        if end >= start {
            lines.drain(start..=end);
        }
    }

    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }

    let mut output = lines.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }

    if !domains.is_empty() {
        output.push('\n');
        output.push_str(BEGIN_MARKER);
        output.push('\n');
        for domain in domains {
            output.push_str(&format!("127.0.0.1\t{domain}\n"));
        }
        output.push_str(END_MARKER);
        output.push('\n');
    }

    output
}

/// Checks whether `domain` resolves to 127.0.0.1 anywhere in the hosts
/// file at `path` — Rezure's managed block or a line the user added
/// themselves. Read-only, no elevation needed.
fn has_entry_at(domain: &str, path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    content.lines().any(|line| {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        parts.next() == Some("127.0.0.1") && parts.any(|p| p == domain)
    })
}

pub fn has_entry(domain: &str) -> bool {
    has_entry_at(domain, &hosts_file_path())
}

/// Wraps `s` as a single-quoted PowerShell string literal, doubling any
/// embedded single quotes — PowerShell's own escape for that context.
fn quote_ps(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The `powershell -Command` one-liner that re-launches `script_path`
/// elevated.
///
/// `-Verb RunAs` throws synchronously if the user declines the UAC prompt —
/// before `-Wait` would ever start blocking — so that failure mode alone is
/// reliable to catch here, and is surfaced as exit code 1223.
///
/// The script path is wrapped in *embedded* double quotes on top of the
/// PowerShell string literal: `Start-Process -ArgumentList @(...)` joins the
/// array elements with plain spaces and quotes none of them itself, so a
/// path containing spaces (`C:\Users\Jane Doe\...` — every Windows account
/// whose name has one) would otherwise reach the elevated PowerShell split
/// across several arguments, leaving `-File` holding only the first
/// fragment. That fails *before* the script runs, so it leaves no error log
/// behind — just the generic "hosts file wasn't updated" fallback. Windows
/// paths cannot contain `"`, so nothing further needs escaping.
fn elevation_launcher(script_path: &Path) -> String {
    let script_arg = quote_ps(&format!("\"{}\"", script_path.display()));
    format!(
        "try {{ Start-Process powershell -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',{script_arg}) -Verb RunAs -Wait; exit 0 }} catch {{ exit 1223 }}"
    )
}

/// The one step that actually needs admin rights: copies `staged` over
/// `dest` via a UAC-elevated `powershell -File`.
///
/// Deliberately does *not* trust `$p.ExitCode` from `Start-Process -Verb
/// RunAs -PassThru` as the success signal — that combination is known to
/// report bogus exit codes (a process launched via `ShellExecuteEx` for
/// elevation isn't always bound properly for exit-code retrieval). Instead
/// this waits for the elevated hop to finish either way, then checks
/// whether `dest` actually ended up matching `staged` — the one thing
/// that's both reliable to check (a plain file read, no elevation needed)
/// and the actual thing that was supposed to happen.
fn elevate_copy(staged: &Path, dest: &Path) -> Result<(), AppError> {
    let dir = staging_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;

    let script_path = dir.join("apply-hosts.ps1");
    let error_log_path = dir.join("apply-hosts.error.log");
    let _ = fs::remove_file(&error_log_path);

    // `-ErrorAction Stop` matters: `Copy-Item` failures (access denied,
    // missing path, ...) are *non-terminating* by default in PowerShell,
    // so without this the `catch` below silently never fires — the error
    // just prints to the (briefly-visible, then closed) elevated console
    // and the script carries on as if nothing happened.
    let script = format!(
        "try {{\n    Copy-Item -LiteralPath {} -Destination {} -Force -ErrorAction Stop\n}} catch {{\n    $_.Exception.Message | Out-File -FilePath {} -Encoding utf8\n    exit 1\n}}\n",
        quote_ps(&staged.display().to_string()),
        quote_ps(&dest.display().to_string()),
        quote_ps(&error_log_path.display().to_string()),
    );
    fs::write(&script_path, script)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", script_path.display())))?;

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &elevation_launcher(&script_path),
        ])
        .status()
        .map_err(|e| AppError::HostsUpdateFailed(e.to_string()))?;

    if status.code() == Some(1223) {
        return Err(AppError::HostsUpdateCancelled);
    }

    let actual = fs::read_to_string(dest).unwrap_or_default();
    let expected = fs::read_to_string(staged).unwrap_or_default();
    if actual == expected {
        return Ok(());
    }

    let detail = fs::read_to_string(&error_log_path)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            "the hosts file wasn't updated — was the admin prompt approved?".to_string()
        });
    Err(AppError::HostsUpdateFailed(detail.trim().to_string()))
}

/// Rewrites Rezure's managed block to match the current project scan.
/// Returns `true` if the hosts file actually changed, `false` if it was
/// already up to date (in which case no UAC prompt is shown at all — the
/// comparison happens before elevation is ever triggered).
pub fn sync_hosts_entries() -> Result<bool, AppError> {
    let mut domains: Vec<String> = scan_projects()?.into_iter().map(|p| p.domain).collect();
    domains.sort();
    domains.dedup();

    let dest = hosts_file_path();
    let existing = fs::read_to_string(&dest).unwrap_or_default();

    let mut current = managed_domains(&existing);
    current.sort();
    if current == domains {
        return Ok(false);
    }

    let updated = rebuild_hosts_content(&existing, &domains);

    let dir = staging_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    let staged = dir.join("hosts.pending");
    fs::write(&staged, &updated)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", staged.display())))?;

    elevate_copy(&staged, &dest)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_hosts_file(name: &str, content: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("rezure-test-hosts-{name}-{}", std::process::id()));
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn quote_ps_doubles_embedded_single_quotes() {
        assert_eq!(quote_ps("C:/plain/path"), "'C:/plain/path'");
        assert_eq!(quote_ps("it's here"), "'it''s here'");
    }

    #[test]
    fn rebuild_adds_a_fresh_managed_block_to_an_untouched_file() {
        let existing = "127.0.0.1\tlocalhost\n";
        let updated = rebuild_hosts_content(existing, &["blog.test".to_string()]);

        assert!(updated.starts_with("127.0.0.1\tlocalhost\n"));
        assert!(updated.contains(BEGIN_MARKER));
        assert!(updated.contains("127.0.0.1\tblog.test\n"));
        assert!(updated.contains(END_MARKER));
    }

    #[test]
    fn rebuild_preserves_the_users_own_lines_untouched() {
        let existing = format!(
            "127.0.0.1\tlocalhost\n192.168.1.5\tmy-nas.lan\n\n{BEGIN_MARKER}\n127.0.0.1\told-project.test\n{END_MARKER}\n"
        );
        let updated = rebuild_hosts_content(&existing, &["new-project.test".to_string()]);

        assert!(updated.contains("192.168.1.5\tmy-nas.lan"));
        assert!(!updated.contains("old-project.test"));
        assert!(updated.contains("new-project.test"));
    }

    #[test]
    fn rebuild_removes_the_block_entirely_when_no_domains_remain() {
        let existing =
            format!("127.0.0.1\tlocalhost\n\n{BEGIN_MARKER}\n127.0.0.1\tblog.test\n{END_MARKER}\n");
        let updated = rebuild_hosts_content(&existing, &[]);

        assert!(!updated.contains(BEGIN_MARKER));
        assert!(!updated.contains("blog.test"));
        assert!(updated.contains("127.0.0.1\tlocalhost"));
    }

    #[test]
    fn rebuild_is_idempotent() {
        let existing = "127.0.0.1\tlocalhost\n";
        let once = rebuild_hosts_content(existing, &["blog.test".to_string()]);
        let twice = rebuild_hosts_content(&once, &["blog.test".to_string()]);
        assert_eq!(once, twice);
    }

    #[test]
    fn managed_domains_reads_back_what_rebuild_wrote() {
        let updated = rebuild_hosts_content(
            "127.0.0.1\tlocalhost\n",
            &["blog.test".to_string(), "shop.test".to_string()],
        );
        let mut found = managed_domains(&updated);
        found.sort();
        assert_eq!(
            found,
            vec!["blog.test".to_string(), "shop.test".to_string()]
        );
    }

    #[test]
    fn has_entry_finds_a_domain_in_the_managed_block() {
        let path = temp_hosts_file(
            "managed",
            &format!(
                "127.0.0.1\tlocalhost\n\n{BEGIN_MARKER}\n127.0.0.1\tblog.test\n{END_MARKER}\n"
            ),
        );
        assert!(has_entry_at("blog.test", &path));
        assert!(!has_entry_at("missing.test", &path));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn has_entry_also_finds_a_domain_the_user_added_by_hand() {
        let path = temp_hosts_file("manual", "127.0.0.1  hand-added.test\n");
        assert!(has_entry_at("hand-added.test", &path));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn has_entry_ignores_a_missing_file_instead_of_panicking() {
        let path = std::env::temp_dir().join("rezure-test-hosts-does-not-exist");
        let _ = fs::remove_file(&path);
        assert!(!has_entry_at("blog.test", &path));
    }

    /// A username with a space in it (`C:\Users\Jane Doe\...`) must still
    /// reach the elevated PowerShell as a single `-File` argument — see
    /// `elevation_launcher`.
    #[test]
    fn launcher_quotes_a_script_path_containing_spaces() {
        let launcher = elevation_launcher(Path::new(r"C:\Users\Jane Doe\app\apply-hosts.ps1"));
        assert!(
            launcher.contains(r#"'"C:\Users\Jane Doe\app\apply-hosts.ps1"'"#),
            "path must carry its own double quotes inside the argument list: {launcher}"
        );
    }

    /// A UAC prompt can only be answered by a human on the Secure Desktop
    /// — nothing can click it programmatically, so the elevated write path
    /// can't be exercised by an automated test. This checks the one part
    /// that *is* safe to verify against the real hosts file: when nothing
    /// needs to change, `sync_hosts_entries` must return early with no
    /// write and no prompt. Requires `www_root()` to have no projects
    /// (true on a fresh checkout) and no leftover Rezure block in the real
    /// hosts file. Run with:
    /// `cargo test --lib services::hosts::tests::sync_is_a_true_no_op_when_already_up_to_date -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn sync_is_a_true_no_op_when_already_up_to_date() {
        let www = super::super::projects::www_root().unwrap();
        let has_projects = fs::read_dir(&www)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);
        assert!(
            !has_projects,
            "this check assumes an empty www_root() — found existing projects"
        );

        let existing = fs::read_to_string(hosts_file_path()).unwrap_or_default();
        assert!(
            !existing.contains(BEGIN_MARKER),
            "this check assumes no pre-existing Rezure block in the real hosts file"
        );

        assert!(!sync_hosts_entries().unwrap());
    }
}
