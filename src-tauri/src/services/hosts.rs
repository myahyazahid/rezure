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

/// The one step that actually needs admin rights: copies `staged` over
/// `dest` via a UAC-elevated `powershell -File`. `Start-Process -Verb
/// RunAs` throws if the user declines the prompt, which the `try/catch`
/// turns into exit code 1223 (`ERROR_CANCELLED`) so it's distinguishable
/// from a real failure.
fn elevate_copy(staged: &Path, dest: &Path) -> Result<(), AppError> {
    let dir = staging_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;

    let script_path = dir.join("apply-hosts.ps1");
    let script = format!(
        "Copy-Item -LiteralPath {} -Destination {} -Force\n",
        quote_ps(&staged.display().to_string()),
        quote_ps(&dest.display().to_string()),
    );
    fs::write(&script_path, script)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", script_path.display())))?;

    let launcher = format!(
        "try {{ \
            $p = Start-Process powershell -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',{}) -Verb RunAs -Wait -PassThru; \
            exit $p.ExitCode \
        }} catch {{ exit 1223 }}",
        quote_ps(&script_path.display().to_string()),
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &launcher])
        .status()
        .map_err(|e| AppError::HostsUpdateFailed(e.to_string()))?;

    match status.code() {
        Some(0) => Ok(()),
        Some(1223) => Err(AppError::HostsUpdateCancelled),
        other => Err(AppError::HostsUpdateFailed(format!(
            "elevated copy exited with status {other:?}"
        ))),
    }
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
