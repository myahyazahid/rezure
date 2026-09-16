use tauri::AppHandle;

use crate::services::binaries::{self, BinaryStatus, InstalledVersionStatus};
use crate::services::mariadb_catalog::{self, MariaDbRelease};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_binaries() -> Vec<BinaryStatus> {
    binaries::list_status()
}

#[tauri::command]
pub async fn install_binary(app: AppHandle, id: String) -> Result<BinaryStatus, AppError> {
    binaries::install(&app, &id).await
}

/// Every MariaDB version currently on disk — the same scan
/// `db_profiles::installed_binaries` uses to resolve a profile's server
/// binary, exposed here for the Switch page's version dropdown.
#[tauri::command]
pub fn list_mariadb_versions() -> Vec<InstalledVersionStatus> {
    binaries::discover_status("mariadb", "mysqld.exe")
}

/// The MariaDB versions Rezure can install, across every branch it knows
/// about. Hits the network on first use, then serves a cached copy until
/// `refresh` is set — same caching contract as `list_php_catalog`.
#[tauri::command]
pub async fn list_mariadb_catalog(refresh: bool) -> Result<Vec<MariaDbRelease>, AppError> {
    mariadb_catalog::list(refresh).await
}

/// Downloads a MariaDB version, verifies it against the checksum MariaDB's
/// own release index reports, and installs it into `bin/mariadb/<version>/`
/// — where `db_profiles::installed_binaries` already looks, so a profile
/// can be pointed at it immediately.
#[tauri::command]
pub async fn install_mariadb_version(app: AppHandle, version: String) -> Result<(), AppError> {
    mariadb_catalog::install(&app, &version).await
}
