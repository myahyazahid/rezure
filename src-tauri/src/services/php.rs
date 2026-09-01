//! Which PHP versions are installed, which one is active, and how new ones
//! get there.
//!
//! There are two ways in, and they meet on the filesystem rather than in a
//! registry:
//!
//! 1. **Install from the catalog** — Rezure downloads a build from php.net
//!    (see [`super::php_catalog`]), checksum-verifies it, and extracts it
//!    into `binaries::install_root()/php/<version>/`.
//! 2. **Drop it in** — the user unpacks a build they downloaded themselves
//!    into `binaries::user_bin_root()/php/<anything>/`, Laragon-style.
//!
//! Both are found by the same [`binaries::discover`] scan, so neither needs
//! to register anything and deleting a folder is a complete uninstall. The
//! only difference Rezure keeps track of is `managed`: a dropped-in build
//! never passed a checksum, and the UI says so.
//!
//! The active version is process-wide in-memory state (a `OnceLock<Mutex<_>>`
//! rather than a manager threaded through every call). `commands::php::set_active_php_version`
//! mirrors a switch into `config::settings::Settings::active_php_version`,
//! and `lib.rs`'s setup restores it on the next launch — if that version is
//! no longer installed, [`active_from`]'s self-healing fallback picks the
//! newest one instead.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::AppHandle;

use super::binaries::{self, ArchiveInstall, InstalledRuntime};
use super::php_catalog;
use super::php_ini;
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

const FAMILY: &str = "php";
const EXE: &str = "php.exe";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpVersionStatus {
    /// The version string doubles as the id — with versions discovered on
    /// disk rather than listed in a fixed manifest, there's no other stable
    /// handle to use.
    pub id: String,
    pub version: String,
    pub installed: bool,
    pub active: bool,
    /// False for versions dropped into the user's own `bin` folder — those
    /// were never checksum-verified by Rezure.
    pub managed: bool,
    /// Folder this version lives in, shown as a tooltip so it's obvious
    /// which copy is which.
    pub path: String,
}

/// Every PHP version currently on disk, newest first.
pub fn installed() -> Vec<InstalledRuntime> {
    binaries::discover(FAMILY, EXE)
}

fn active_cell() -> &'static Mutex<String> {
    static ACTIVE: OnceLock<Mutex<String>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(String::new()))
}

/// The active version, resolved against an *already-taken* snapshot.
///
/// Callers that also need the list pass their own scan in rather than
/// triggering a second one. Two scans can disagree — a folder in the
/// drop-in root can appear or vanish between them — and when they do, the
/// version marked active is one the list doesn't contain, so the UI shows
/// an installed PHP with nothing active. Resolving both from one snapshot
/// makes that impossible rather than unlikely.
///
/// Self-healing on top of that: the stored choice goes stale in ways
/// nothing notifies us about (the user deletes a folder, or installs their
/// first version after the app started with none), so it's checked against
/// the snapshot and falls back to the newest installed version, writing
/// that choice back.
fn active_from(present: &[InstalledRuntime]) -> String {
    let mut cell = active_cell().lock().unwrap();

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

pub fn active_id() -> String {
    active_from(&installed())
}

/// The active version's `php.exe`.
pub fn active_exe() -> Result<PathBuf, AppError> {
    let present = installed();
    let active = active_from(&present);
    if active.is_empty() {
        return Err(AppError::BinaryNotInstalled("PHP".to_string()));
    }
    present
        .into_iter()
        .find(|runtime| runtime.version == active)
        .map(|runtime| runtime.exe)
        .ok_or(AppError::PhpVersionNotFound(active))
}

/// Switches the active version. Rejects anything not on disk — installing
/// is a separate, explicit step.
pub fn set_active(version: &str) -> Result<Vec<PhpVersionStatus>, AppError> {
    if !installed().iter().any(|runtime| runtime.version == version) {
        return Err(AppError::PhpVersionNotFound(version.to_string()));
    }
    *active_cell().lock().unwrap() = version.to_string();
    Ok(list())
}

pub fn list() -> Vec<PhpVersionStatus> {
    let present = installed();
    let active = active_from(&present);
    present
        .into_iter()
        .map(|runtime| PhpVersionStatus {
            active: runtime.version == active,
            id: runtime.version.clone(),
            version: runtime.version,
            installed: true,
            managed: runtime.managed,
            path: runtime.dir.display().to_string(),
        })
        .collect()
}

/// Downloads and installs a version from php.net's catalog.
pub async fn install(app: &AppHandle, version: &str) -> Result<Vec<PhpVersionStatus>, AppError> {
    if installed().iter().any(|runtime| runtime.version == version) {
        return Ok(list());
    }

    let release = php_catalog::find(version).await?;
    let dest_dir = binaries::install_root()?
        .join(FAMILY)
        .join(&release.version);
    let php_dir = dest_dir.clone();

    binaries::install_archive(
        app,
        &ArchiveInstall {
            id: &release.version,
            label: "PHP",
            download_url: &release.download_url,
            sha256: &release.sha256,
            dest_dir,
            exe_relative_path: EXE,
        },
    )
    .await?;

    write_cli_php_ini(&php_dir);
    Ok(list())
}

/// Registers a PHP build the user downloaded themselves by copying it into
/// the drop-in folder.
///
/// A copy, not a reference: the scan-the-folder model stays true — a
/// version *is* a folder under one of the two roots, with no separate list
/// of external paths to keep in sync. Copying also leaves the user's
/// original download alone.
pub async fn add_from_folder(source: PathBuf) -> Result<Vec<PhpVersionStatus>, AppError> {
    tokio::task::spawn_blocking(move || add_from_folder_blocking(&source))
        .await
        .map_err(|e| AppError::Io(format!("copy task panicked: {e}")))??;
    Ok(list())
}

fn add_from_folder_blocking(source: &Path) -> Result<(), AppError> {
    // Accept either the folder holding `php.exe` or a parent one level up,
    // since an extracted zip often nests everything in its own folder and
    // picking the outer one is the easy mistake to make.
    let root = locate_php_root(source).ok_or_else(|| {
        AppError::Io(format!(
            "no php.exe in {} (or one folder below it)",
            source.display()
        ))
    })?;

    let version = read_php_version(&root.join(EXE))?;
    let dest = binaries::user_bin_root()?.join(FAMILY).join(&version);
    if dest.join(EXE).is_file() {
        return Err(AppError::PhpVersionAlreadyInstalled(version));
    }

    std::fs::create_dir_all(&dest)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dest.display())))?;

    if let Err(err) = copy_dir(&root, &dest) {
        // Half a PHP install would read as a real version on the next scan.
        let _ = std::fs::remove_dir_all(&dest);
        return Err(err);
    }

    write_cli_php_ini(&dest);
    Ok(())
}

/// Gives a freshly installed version the `php.ini` the official zip leaves
/// out, so `php` from a terminal has its extensions from the first run —
/// without it, PHP loads none at all and Laravel reports "could not find
/// driver" the moment a project talks to MySQL.
///
/// Best-effort: an install that produced a working `php.exe` is a
/// successful install, and [`super::php_path::sync`] writes the ini again
/// on the next switch if this didn't take.
fn write_cli_php_ini(php_dir: &Path) {
    match php_ini::ensure_cli_php_ini(php_dir) {
        Ok(Some(path)) => log::info!("wrote {}", path.display()),
        Ok(None) => {}
        Err(err) => log::warn!("could not write php.ini in {}: {err}", php_dir.display()),
    }
}

fn locate_php_root(source: &Path) -> Option<PathBuf> {
    if source.join(EXE).is_file() {
        return Some(source.to_path_buf());
    }
    std::fs::read_dir(source)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|child| child.join(EXE).is_file())
}

/// Asks the binary itself what version it is, rather than trusting the
/// folder name — a hand-renamed folder would otherwise install a version
/// under the wrong number and silently mislabel every switch after it.
fn read_php_version(exe: &Path) -> Result<String, AppError> {
    let output = Command::new(exe)
        .args(["-r", "echo PHP_VERSION;"])
        .hidden()
        .output()
        .map_err(|e| AppError::Io(format!("could not run {}: {e}", exe.display())))?;

    if !output.status.success() {
        return Err(AppError::Io(format!(
            "{} isn't a working PHP binary",
            exe.display()
        )));
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() || !version.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(AppError::Io(format!(
            "{} reported an unreadable version: {version:?}",
            exe.display()
        )));
    }
    Ok(version)
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), AppError> {
    let entries = std::fs::read_dir(from)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", from.display())))?;

    for entry in entries {
        let entry = entry.map_err(|e| AppError::Io(e.to_string()))?;
        let source = entry.path();
        let dest = to.join(entry.file_name());

        if source.is_dir() {
            std::fs::create_dir_all(&dest)
                .map_err(|e| AppError::Io(format!("could not create {}: {e}", dest.display())))?;
            copy_dir(&source, &dest)?;
        } else {
            std::fs::copy(&source, &dest)
                .map_err(|e| AppError::Io(format!("could not copy to {}: {e}", dest.display())))?;
        }
    }
    Ok(())
}

/// Deletes an installed version's folder.
pub fn remove(version: &str) -> Result<Vec<PhpVersionStatus>, AppError> {
    let runtime = installed()
        .into_iter()
        .find(|runtime| runtime.version == version)
        .ok_or_else(|| AppError::PhpVersionNotFound(version.to_string()))?;

    std::fs::remove_dir_all(&runtime.dir)
        .map_err(|e| AppError::Io(format!("could not remove {}: {e}", runtime.dir.display())))?;

    // `active_id` re-picks on its own once the folder is gone.
    Ok(list())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-empty list always has exactly one active entry — the flag and
    /// the list have to come from the same scan for that to hold.
    #[test]
    fn a_non_empty_list_always_has_exactly_one_active_version() {
        let versions = list();
        let active = versions.iter().filter(|v| v.active).count();
        if versions.is_empty() {
            assert_eq!(active, 0);
        } else {
            assert_eq!(active, 1, "installed versions: {versions:?}");
        }
    }

    /// The regression this guards: resolving the active version from one
    /// snapshot and building the list from another lets a folder appear or
    /// disappear in between, leaving every entry inactive.
    #[test]
    fn the_active_version_is_resolved_from_the_snapshot_it_is_listed_with() {
        let present = installed();
        let active = active_from(&present);
        if present.is_empty() {
            assert!(active.is_empty());
        } else {
            assert!(
                present.iter().any(|runtime| runtime.version == active),
                "the active version must be one of the versions it was chosen from"
            );
        }
    }

    #[test]
    fn every_listed_version_is_installed_by_definition() {
        assert!(list().iter().all(|v| v.installed));
    }

    #[test]
    fn set_active_rejects_a_version_that_isnt_on_disk() {
        let err = set_active("1.0.0-not-installed").unwrap_err();
        assert!(matches!(err, AppError::PhpVersionNotFound(_)));
    }

    #[test]
    fn remove_rejects_a_version_that_isnt_on_disk() {
        assert!(matches!(
            remove("1.0.0-not-installed"),
            Err(AppError::PhpVersionNotFound(_))
        ));
    }

    #[test]
    fn adding_a_folder_with_no_php_exe_is_a_clear_error() {
        let empty = std::env::temp_dir().join(format!("rezure-test-nophp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&empty);
        let err = add_from_folder_blocking(&empty).unwrap_err();
        assert!(
            format!("{err}").contains("php.exe"),
            "the message must name what was missing, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&empty);
    }

    /// What the Switch page will actually show, against real disk state.
    /// Run with:
    /// `cargo test --lib services::php::tests::print_installed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_installed() {
        for version in list() {
            println!(
                "{:8} active={:5} managed={:5} {}",
                version.version, version.active, version.managed, version.path
            );
        }
    }

    /// Reads the version out of a really-installed PHP. Run with:
    /// `cargo test --lib services::php::tests::reads_the_version_from_a_real_binary -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_the_version_from_a_real_binary() {
        let Some(runtime) = installed().into_iter().next() else {
            println!("no PHP installed — nothing to check");
            return;
        };
        let reported = read_php_version(&runtime.exe).unwrap();
        println!("{} reports {reported}", runtime.exe.display());
        assert_eq!(
            reported, runtime.version,
            "the folder name and the binary must agree"
        );
    }
}
