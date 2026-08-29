//! Tracks which installed PHP version is active — the one `services::process`
//! spawns for the FastCGI service and `services::scaffold` runs Composer
//! through.
//!
//! Global, process-wide state (not per-request): a `OnceLock<Mutex<...>>`
//! rather than threading a manager object through every command and
//! service. In-memory only for now — a restart resets it to the newest
//! *installed* version, since Phase 4's settings persistence hasn't landed
//! yet to remember an explicit choice across restarts.

use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use super::binaries::{self, BinaryPackage};
use crate::utils::error::AppError;

const FAMILY: &str = "php";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpVersionStatus {
    pub id: String,
    pub version: String,
    pub installed: bool,
    pub active: bool,
}

fn active_cell() -> &'static Mutex<String> {
    static ACTIVE: OnceLock<Mutex<String>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(default_active_id()))
}

/// The newest *installed* version, or just the newest known version if
/// nothing's installed yet (nothing works until something is, regardless
/// of which id this picks).
fn default_active_id() -> String {
    let versions = binaries::family_packages(FAMILY);
    versions
        .iter()
        .find(|pkg| binaries::is_installed(pkg))
        .or_else(|| versions.first())
        .map(|pkg| pkg.id.to_string())
        .unwrap_or_default()
}

pub fn active_id() -> String {
    active_cell().lock().unwrap().clone()
}

/// The active version's package — resolves which `php.exe`/`php-cgi.exe`
/// `services::process` and `services::scaffold` should actually run.
pub fn active_package() -> Result<&'static BinaryPackage, AppError> {
    binaries::find(&active_id())
}

/// Switches the active version. Rejects anything not installed yet — the
/// Switch UI's "Install version" is a separate, explicit step.
pub fn set_active(id: &str) -> Result<Vec<PhpVersionStatus>, AppError> {
    let pkg = binaries::find(id)?;
    if pkg.family != FAMILY {
        return Err(AppError::PhpVersionNotFound(id.to_string()));
    }
    if !binaries::is_installed(pkg) {
        return Err(AppError::BinaryNotInstalled(format!(
            "{} {}",
            pkg.name, pkg.version
        )));
    }

    *active_cell().lock().unwrap() = id.to_string();
    Ok(list())
}

pub fn list() -> Vec<PhpVersionStatus> {
    let active = active_id();
    binaries::family_packages(FAMILY)
        .iter()
        .map(|pkg| PhpVersionStatus {
            id: pkg.id.to_string(),
            version: pkg.version.to_string(),
            installed: binaries::is_installed(pkg),
            active: pkg.id == active,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_reports_every_php_version_with_exactly_one_active() {
        let versions = list();
        assert_eq!(versions.len(), binaries::family_packages(FAMILY).len());
        assert_eq!(versions.iter().filter(|v| v.active).count(), 1);
    }

    #[test]
    fn set_active_rejects_a_non_php_id() {
        assert!(set_active("nginx").is_err());
    }

    #[test]
    fn set_active_rejects_an_unknown_id() {
        assert!(set_active("php-1.0.0").is_err());
    }

    #[test]
    fn set_active_rejects_a_version_that_isnt_installed() {
        // None of the fixed PHP versions are guaranteed installed on a
        // fresh checkout/CI box, so this just needs *a* not-installed one.
        let not_installed = binaries::family_packages(FAMILY)
            .into_iter()
            .find(|pkg| !binaries::is_installed(pkg));

        if let Some(pkg) = not_installed {
            let err = set_active(pkg.id).unwrap_err();
            assert!(matches!(err, AppError::BinaryNotInstalled(_)));
        }
    }
}
