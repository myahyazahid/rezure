//! Which Node.js version is active — globally, and per-project — and where
//! its `node.exe` lives on disk.
//!
//! Installing is [`super::node_catalog`]'s job; this module is what makes an
//! installed version *usable*: which one is the global default, and the
//! folder that has to go on `PATH` for a spawned process to resolve `node`/
//! `npm`/`npx` as that version.
//!
//! # Why this isn't a pool like `services::php_pool`
//!
//! PHP runs as a persistent FastCGI responder that nginx proxies every
//! request through, so two projects on different PHP versions need two
//! concurrently-running processes on two different ports — hence the pool.
//!
//! Node isn't served through nginx here. It's invoked ephemerally — a
//! terminal opened from a project card, `npm install`, a dev server the user
//! starts themselves — so "using a specific version for a project" only
//! means resolving the right `node.exe`/`npm` at the moment something is
//! spawned, by putting that version's folder first on the *child process's*
//! `PATH`. No port, no long-running service, no `ServiceManager` entry.
//! See `services::launcher::open_terminal`.
//!
//! # Global active vs. per-project pin
//!
//! Same relationship as PHP: the global active version (this module) is what
//! a project uses when it hasn't pinned its own (`ProjectInfo::node_version`,
//! `NULL` in SQLite). Unlike PHP's PATH junction, there's no "Node
//! Everywhere" system-wide link yet — the active version only reaches
//! processes Rezure itself spawns for a project.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use super::binaries::{self, InstalledRuntime};
use crate::utils::error::AppError;

const FAMILY: &str = "node";
const EXE: &str = "node.exe";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeVersionStatus {
    /// The version string doubles as the id — same reasoning as
    /// `services::php::PhpVersionStatus`.
    pub id: String,
    pub version: String,
    pub installed: bool,
    pub active: bool,
    /// False for versions dropped into the user's own `bin` folder.
    pub managed: bool,
    pub path: String,
}

/// Every Node.js version currently on disk, newest first.
pub fn installed() -> Vec<InstalledRuntime> {
    binaries::discover(FAMILY, EXE)
}

fn active_cell() -> &'static Mutex<String> {
    static ACTIVE: OnceLock<Mutex<String>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(String::new()))
}

/// The active version, resolved against an *already-taken* snapshot — same
/// self-healing shape as `services::php::active_from`: falls back to the
/// newest installed version when the stored choice is stale (deleted, or
/// nothing was ever installed when the app started).
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

/// Empty when nothing is installed — callers that need "is there an active
/// version at all" check for that rather than treating it as an error, same
/// as `services::php::active_id`.
pub fn active_id() -> String {
    active_from(&installed())
}

/// Switches the global active version. Rejects anything not on disk.
pub fn set_active(version: &str) -> Result<Vec<NodeVersionStatus>, AppError> {
    if !installed().iter().any(|runtime| runtime.version == version) {
        return Err(AppError::NodeVersionNotFound(version.to_string()));
    }
    *active_cell().lock().unwrap() = version.to_string();
    Ok(list())
}

pub fn list() -> Vec<NodeVersionStatus> {
    let present = installed();
    let active = active_from(&present);
    present
        .into_iter()
        .map(|runtime| NodeVersionStatus {
            active: runtime.version == active,
            id: runtime.version.clone(),
            version: runtime.version,
            installed: true,
            managed: runtime.managed,
            path: runtime.dir.display().to_string(),
        })
        .collect()
}

/// The folder holding a specific version's `node.exe` — and, alongside it in
/// the same Windows distribution, `npm`/`npm.cmd`/`npx`/`npx.cmd`. This is
/// what `services::launcher::open_terminal` prepends to a spawned terminal's
/// `PATH`, not [`installed`]'s `dir` (which, for Node's nested archive
/// layout, is one level above where the executables actually sit).
pub fn bin_dir_for(version: &str) -> Result<PathBuf, AppError> {
    installed()
        .into_iter()
        .find(|runtime| runtime.version == version)
        .and_then(|runtime| runtime.exe.parent().map(Path::to_path_buf))
        .ok_or_else(|| AppError::NodeVersionNotFound(version.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-empty list always has exactly one active entry — the flag and
    /// the list have to come from the same scan for that to hold. Mirrors
    /// `services::php`'s equivalent test.
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

    #[test]
    fn every_listed_version_is_installed_by_definition() {
        assert!(list().iter().all(|v| v.installed));
    }

    #[test]
    fn set_active_rejects_a_version_that_isnt_on_disk() {
        let err = set_active("0.0.0-not-installed").unwrap_err();
        assert!(matches!(err, AppError::NodeVersionNotFound(_)));
    }

    #[test]
    fn bin_dir_for_rejects_a_version_that_isnt_on_disk() {
        assert!(matches!(
            bin_dir_for("0.0.0-not-installed"),
            Err(AppError::NodeVersionNotFound(_))
        ));
    }

    /// What the Switch page will actually show, against real disk state, and
    /// where a terminal's `PATH` would point for each version. Run with:
    /// `cargo test --lib services::node::tests::print_installed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_installed() {
        for version in list() {
            let bin_dir = bin_dir_for(&version.version).unwrap();
            println!(
                "{:10} active={:5} managed={:5} bin_dir={}",
                version.version,
                version.active,
                version.managed,
                bin_dir.display()
            );
        }
    }
}
