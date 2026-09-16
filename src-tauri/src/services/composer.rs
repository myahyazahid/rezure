//! Which Composer versions are installed, which one is active, and how new
//! ones get there — the same shape as [`super::php`], minus PHP's PATH-link
//! and pooled-instance concerns, since nothing spawns Composer as a running
//! service: it's a `.phar` invoked once per scaffold.
//!
//! Before this module existed, `services::scaffold::ensure_composer` cached
//! exactly one `composer.phar` at a fixed path, always the current stable
//! build, with no checksum — "whatever's newest" was the only version that
//! existed. This makes Composer a version like any other runtime: several
//! can sit on disk under `bin/composer/<version>/`, one is active, and
//! installing one is checksum-verified through [`super::composer_catalog`].

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::AppHandle;

use super::binaries::{self, InstalledRuntime};
use super::composer_catalog;
use crate::utils::error::AppError;

const FAMILY: &str = "composer";
const PHAR: &str = "composer.phar";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerVersionStatus {
    pub id: String,
    pub version: String,
    pub installed: bool,
    pub active: bool,
}

/// Every Composer version currently on disk, newest first.
pub fn installed() -> Vec<InstalledRuntime> {
    binaries::discover(FAMILY, PHAR)
}

fn active_cell() -> &'static Mutex<String> {
    static ACTIVE: OnceLock<Mutex<String>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(String::new()))
}

/// Self-healing against a snapshot, same reasoning as `php::active_from`:
/// resolving the active version and building the list from two separate
/// scans could disagree if a folder appears or disappears in between.
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

/// The active version's `composer.phar`. Errors when nothing is installed
/// yet — callers that scaffold on demand (`services::scaffold`) install the
/// newest catalog version first rather than surfacing this to the user.
pub fn active_exe() -> Result<PathBuf, AppError> {
    let present = installed();
    let active = active_from(&present);
    if active.is_empty() {
        return Err(AppError::BinaryNotInstalled("Composer".to_string()));
    }
    present
        .into_iter()
        .find(|runtime| runtime.version == active)
        .map(|runtime| runtime.exe)
        .ok_or_else(|| AppError::CatalogVersionNotFound {
            runtime: "composer".to_string(),
            version: active,
        })
}

pub fn set_active(version: &str) -> Result<Vec<ComposerVersionStatus>, AppError> {
    if !installed().iter().any(|runtime| runtime.version == version) {
        return Err(AppError::CatalogVersionNotFound {
            runtime: "composer".to_string(),
            version: version.to_string(),
        });
    }
    *active_cell().lock().unwrap() = version.to_string();
    Ok(list())
}

pub fn list() -> Vec<ComposerVersionStatus> {
    let present = installed();
    let active = active_from(&present);
    present
        .into_iter()
        .map(|runtime| ComposerVersionStatus {
            active: runtime.version == active,
            id: runtime.version.clone(),
            version: runtime.version,
            installed: true,
        })
        .collect()
}

fn version_dir(version: &str) -> Result<PathBuf, AppError> {
    Ok(binaries::install_root()?.join(FAMILY).join(version))
}

fn write_version(version: &str, bytes: &[u8]) -> Result<PathBuf, AppError> {
    let dir = version_dir(version)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    let phar_path = dir.join(PHAR);
    std::fs::write(&phar_path, bytes)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", phar_path.display())))?;
    Ok(phar_path)
}

/// Downloads, verifies and installs one Composer version, reporting
/// progress through [`binaries::PROGRESS_EVENT`] — what the Switch page's
/// install modal drives.
///
/// Not a zip — `composer.phar` is downloaded as-is and written straight
/// into its version folder, so this can't reuse `binaries::install_archive`
/// (which always extracts). There's no extraction stage to report because
/// there's nothing to extract.
pub async fn install(
    app: &AppHandle,
    version: &str,
) -> Result<Vec<ComposerVersionStatus>, AppError> {
    if installed().iter().any(|runtime| runtime.version == version) {
        return Ok(list());
    }

    let release = composer_catalog::find(version).await?;
    let sha256 = composer_catalog::checksum_for(&release).await?;

    let id = format!("composer-{version}");
    let bytes = binaries::download(app, &id, &release.download_url).await?;
    binaries::verify_checksum(&id, &sha256, &bytes)?;
    write_version(version, &bytes)?;

    Ok(list())
}

/// Same download-verify-write as [`install`], but with no [`AppHandle`] and
/// no progress events — for `services::scaffold::ensure_composer`'s lazy
/// self-heal, which never reported progress for this even before this
/// module existed (there was no per-version download to report on) and has
/// no `AppHandle` in scope to report through.
pub async fn install_silent(version: &str) -> Result<PathBuf, AppError> {
    if let Some(existing) = installed().into_iter().find(|r| r.version == version) {
        return Ok(existing.exe);
    }

    let release = composer_catalog::find(version).await?;
    let sha256 = composer_catalog::checksum_for(&release).await?;

    let bytes = reqwest::get(&release.download_url)
        .await
        .map_err(|e| AppError::Download(format!("{}: {e}", release.download_url)))?
        .error_for_status()
        .map_err(|e| AppError::Download(format!("{}: {e}", release.download_url)))?
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| AppError::Download(format!("{}: {e}", release.download_url)))?;

    binaries::verify_checksum(&format!("composer-{version}"), &sha256, &bytes)?;
    write_version(version, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_empty_list_always_has_exactly_one_active_entry() {
        let versions = list();
        let active = versions.iter().filter(|v| v.active).count();
        if versions.is_empty() {
            assert_eq!(active, 0);
        } else {
            assert_eq!(active, 1, "installed versions: {versions:?}");
        }
    }

    #[test]
    fn set_active_rejects_a_version_that_isnt_on_disk() {
        assert!(matches!(
            set_active("0.0.0-not-installed"),
            Err(AppError::CatalogVersionNotFound { .. })
        ));
    }
}
