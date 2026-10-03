//! The PostgreSQL versions Rezure can install: EDB's Windows zip builds, the
//! same source Laragon uses.
//!
//! # Why a pinned list and not a live index
//!
//! EDB publishes no machine-readable index and no checksums for these
//! archives, so there is nothing to read at runtime the way
//! `mariadb_catalog` reads MariaDB's API. Each entry below is one archive
//! downloaded and hashed by hand — the same trade-off `php_ext.rs` makes for
//! PECL. A new patch release means a new entry (and a new hash); until then
//! the pinned one keeps installing, since EDB keeps old builds online.
//!
//! # What gets extracted
//!
//! The archive is close to 400 MB because it carries pgAdmin 4 (800 MB
//! unpacked) and StackBuilder. Only the server, its libraries, its `share`
//! data and the license files are kept — about 150 MB.

use serde::Serialize;
use tauri::AppHandle;

use super::binaries::{self, ArchiveInstall};
use super::postgres;
use crate::utils::error::AppError;
use crate::utils::paths;

/// One installable build.
struct Pinned {
    version: &'static str,
    /// EDB's own build number for the archive (`postgresql-18.6-5-…`).
    build: u32,
    sha256: &'static str,
}

/// Newest first. Majors offered: the three newest EDB still builds for
/// Windows, chosen by the maintainer.
const RELEASES: &[Pinned] = &[
    Pinned {
        version: "18.6",
        build: 5,
        sha256: "e2246ba91d22345bc3d017586c09ede52d9df180b1eeb480f050445f1cad84e2",
    },
    Pinned {
        version: "17.11",
        build: 5,
        sha256: "80379b2c04d51c30225532e0ae04509899141e9957ed096fe749d7fd9df8f82f",
    },
    Pinned {
        version: "16.15",
        build: 5,
        sha256: "43bb45f173a6f08cf1d29a97a6d8deb119e8e8093a24c00d2d1001a0ccaa8281",
    },
];

/// The parts of the archive worth keeping — see the module docs.
const KEEP: &[&str] = &[
    "pgsql/bin/",
    "pgsql/lib/",
    "pgsql/share/",
    "pgsql/server_license.txt",
    "pgsql/commandlinetools_3rd_party_licenses.txt",
];

/// Where `postgres.exe` lands, relative to the version's folder.
const EXE_RELATIVE: &str = "pgsql/bin/postgres.exe";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostgresRelease {
    pub version: String,
    pub major: String,
    pub download_url: String,
    pub latest: bool,
    pub installed: bool,
}

fn download_url(pinned: &Pinned) -> String {
    format!(
        "https://get.enterprisedb.com/postgresql/postgresql-{}-{}-windows-x64-binaries.zip",
        pinned.version, pinned.build
    )
}

/// Every installable version, newest first, with what's on disk marked.
pub fn list() -> Vec<PostgresRelease> {
    let present = postgres::installed();
    RELEASES
        .iter()
        .enumerate()
        .map(|(index, pinned)| PostgresRelease {
            version: pinned.version.to_string(),
            major: postgres::major_of(pinned.version),
            download_url: download_url(pinned),
            latest: index == 0,
            installed: present
                .iter()
                .any(|runtime| runtime.version == pinned.version),
        })
        .collect()
}

/// Downloads, verifies and installs one version into
/// `bin/postgres/<version>/`, where `postgres::installed` looks. Progress is
/// reported under `postgres-<version>`. Idempotent.
pub async fn install(app: &AppHandle, version: &str) -> Result<(), AppError> {
    install_as(app, version, &format!("postgres-{version}")).await
}

/// Installs the newest version, reporting progress under `progress_id` — the
/// Services card's Install button, which has no version to pick. Returns the
/// version installed.
pub async fn install_latest(app: &AppHandle, progress_id: &str) -> Result<String, AppError> {
    let newest = RELEASES
        .first()
        .ok_or_else(|| AppError::UnknownBinary(postgres::FAMILY.to_string()))?;
    install_as(app, newest.version, progress_id).await?;
    Ok(newest.version.to_string())
}

async fn install_as(app: &AppHandle, version: &str, progress_id: &str) -> Result<(), AppError> {
    if postgres::installed()
        .iter()
        .any(|runtime| runtime.version == version)
    {
        return Ok(());
    }
    let pinned = RELEASES
        .iter()
        .find(|pinned| pinned.version == version)
        .ok_or_else(|| AppError::CatalogVersionNotFound {
            runtime: postgres::FAMILY.to_string(),
            version: version.to_string(),
        })?;

    binaries::install_archive(
        app,
        &ArchiveInstall {
            id: progress_id,
            label: postgres::SERVICE_NAME,
            download_url: &download_url(pinned),
            sha256: pinned.sha256,
            dest_dir: paths::bin()?.join(postgres::FAMILY).join(pinned.version),
            exe_relative_path: EXE_RELATIVE,
            keep: KEEP,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_is_well_formed_and_newest_first() {
        for pinned in RELEASES {
            assert_eq!(pinned.sha256.len(), 64, "{}", pinned.version);
            assert!(pinned.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        }
        for pair in RELEASES.windows(2) {
            assert_eq!(
                binaries::compare_versions(pair[0].version, pair[1].version),
                std::cmp::Ordering::Greater
            );
        }
        let releases = list();
        assert!(releases[0].latest && releases.iter().skip(1).all(|r| !r.latest));
    }

    #[test]
    fn the_server_is_kept_and_pgadmin_is_not() {
        let kept = KEEP
            .iter()
            .map(|prefix| prefix.to_string())
            .collect::<Vec<_>>();
        let wanted = |name: &str| kept.iter().any(|prefix| name.starts_with(prefix.as_str()));
        assert!(wanted(EXE_RELATIVE));
        assert!(wanted("pgsql/bin/psql.exe"));
        assert!(wanted("pgsql/share/timezone/UTC"));
        assert!(!wanted("pgsql/pgAdmin 4/runtime/pgAdmin4.exe"));
        assert!(!wanted("pgsql/StackBuilder/bin/stackbuilder.exe"));
        assert!(!wanted("pgsql/doc/postgresql/html/index.html"));
    }

    /// Re-downloads every pinned archive and checks it against its hash —
    /// the check to run when adding or bumping an entry. ~380 MB each. Run:
    /// `cargo test --lib services::postgres_catalog::tests::every_pin_matches_its_download -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn every_pin_matches_its_download() {
        for pinned in RELEASES {
            let bytes = reqwest::get(download_url(pinned))
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .bytes()
                .await
                .unwrap();
            binaries::verify_checksum(pinned.version, pinned.sha256, &bytes).unwrap();
            println!("{} ok ({} bytes)", pinned.version, bytes.len());
        }
    }

    /// Installs from an archive already on disk, through the same checksum
    /// and extraction an in-app install uses — for testing without another
    /// 380 MB download. Run with `REZURE_TEST_PG_ZIP=<path to the zip>` and
    /// `REZURE_TEST_PG_VERSION=18.6`:
    /// `cargo test --lib services::postgres_catalog::tests::installs_from_a_local_archive -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn installs_from_a_local_archive() {
        let zip = std::env::var("REZURE_TEST_PG_ZIP").expect("REZURE_TEST_PG_ZIP");
        let version = std::env::var("REZURE_TEST_PG_VERSION").expect("REZURE_TEST_PG_VERSION");
        let pinned = RELEASES.iter().find(|p| p.version == version).unwrap();
        let bytes = std::fs::read(&zip).unwrap();
        binaries::verify_checksum(&version, pinned.sha256, &bytes).unwrap();

        let dest = paths::bin().unwrap().join(postgres::FAMILY).join(&version);
        let keep: Vec<String> = KEEP.iter().map(|prefix| prefix.to_string()).collect();
        binaries::extract(std::io::Cursor::new(bytes), &dest, &keep).unwrap();
        assert!(dest.join(EXE_RELATIVE).is_file());
        assert!(!dest.join("pgsql").join("pgAdmin 4").exists());
        assert!(list()
            .iter()
            .any(|release| release.version == version && release.installed));
        println!("installed into {}", dest.display());
    }
}
