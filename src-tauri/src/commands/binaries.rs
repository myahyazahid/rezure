use tauri::AppHandle;

use crate::services::binaries::{self, BinaryStatus, InstalledVersionStatus};
use crate::services::mariadb_catalog::{self, MariaDbRelease};
use crate::services::{connections, localdb_bridge, msi, postgres, postgres_catalog};
use crate::utils::error::AppError;

#[tauri::command]
pub fn list_binaries() -> Vec<BinaryStatus> {
    binaries::list_status()
}

/// Installs a package by id — a portable zip from `binaries::MANIFEST`, one
/// of the Windows installers in `services::msi`, or `postgres`: the newest
/// PostgreSQL, for the Install button on its service card.
///
/// `accept_license` only matters for the latter, and is only ever sent by
/// the frontend after the user ticked the box beside the license link.
#[tauri::command]
pub async fn install_binary(
    app: AppHandle,
    id: String,
    accept_license: Option<bool>,
) -> Result<BinaryStatus, AppError> {
    if id == postgres::SERVICE_ID {
        let version = postgres_catalog::install_latest(&app, &id).await?;
        // Like LocalDB below: the server is only useful on the Databases page
        // through the connection that points at it.
        connections::ensure_postgres();
        return Ok(BinaryStatus {
            id,
            name: postgres::SERVICE_NAME.to_string(),
            version,
            installed: true,
            license_url: None,
        });
    }
    let Some(package) = msi::find(&id) else {
        return binaries::install(&app, &id).await;
    };
    msi::install(&app, &id, accept_license.unwrap_or(false)).await?;
    // LocalDB is only useful on the Databases page through the connection
    // that points at it; adding it here means it's there the moment the
    // install finishes, not after the next restart.
    if id == "sqllocaldb" {
        connections::ensure_localdb();
        localdb_bridge::start();
    }
    Ok(msi::status_of(package))
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
