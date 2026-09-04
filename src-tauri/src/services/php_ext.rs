//! Installs PECL extensions that the official Windows PHP zip doesn't ship.
//!
//! `redis` is the one that keeps costing people an afternoon: a Laravel
//! project with `"ext-redis": "*"` in `composer.json` fails Composer's
//! platform check outright, and queues, cache and Horizon all assume it. It
//! is not in php.net's zip and never will be — PECL extensions are published
//! separately, built per PHP branch.
//!
//! # Why the checksums are pinned by hand
//!
//! php.net publishes a machine-readable `releases.json` for PHP itself, which
//! is what lets [`super::php_catalog`] stay current on its own *and* verify
//! what it downloads. The PECL area publishes no such index and no `.sha256`
//! files — only the archives. So the choice was between dropping checksum
//! verification for these downloads or pinning hashes here, and this codebase
//! already decided that question: `binaries::MANIFEST` pins its own.
//!
//! The cost is honest and visible: when a new PHP branch appears, the table
//! below needs one line per extension before Rezure will offer it there. Until
//! that line exists the UI says the extension isn't available for that version
//! yet, which is a wrong answer that explains itself, rather than an unverified
//! download nobody was told about.
//!
//! Only the **NTS x64** build is ever fetched, matching what
//! [`super::php_catalog`] installs and what `php-cgi` runs behind nginx.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use super::binaries::{self, ArchiveInstall};
use super::{php, php_ini};
use crate::utils::error::AppError;
use crate::utils::paths;

/// Where the archives live. The rest of the URL is derived, not stored, so a
/// new build only ever costs a hash.
const PECL_BASE: &str = "https://downloads.php.net/~windows/pecl/releases";

/// One prebuilt DLL: the PHP branch it targets, the toolchain php.net built
/// it with (it is part of the filename), and the archive's SHA-256.
pub struct PeclBuild {
    pub branch: &'static str,
    pub compiler: &'static str,
    pub sha256: &'static str,
}

/// An extension Rezure can install, and every PHP branch it has a verified
/// build for.
pub struct PeclExtension {
    /// Both the PECL package name and the `extension=` value, e.g. `redis`.
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    /// One line on why a project would want it, shown in the UI.
    pub summary: &'static str,
    pub builds: &'static [PeclBuild],
}

/// Hashes verified against the archives on 2026-09-05.
pub const CATALOG: &[PeclExtension] = &[PeclExtension {
    id: "redis",
    name: "Redis",
    version: "6.3.0",
    summary: "Queues, cache and Horizon in Laravel projects that require ext-redis.",
    builds: &[
        PeclBuild {
            branch: "7.4",
            compiler: "vc15",
            sha256: "c74f1fb5500b493050839330b4cbfb84dafc07da99b898939ac4b690fe74de1a",
        },
        PeclBuild {
            branch: "8.0",
            compiler: "vs16",
            sha256: "9d3143049d3c27e715ea92a023dfab92389df985aec224d6ed9d092b4ba5ea9a",
        },
        PeclBuild {
            branch: "8.1",
            compiler: "vs16",
            sha256: "952ec845408e343f273eab109e62a9c7732178d93985120736da4a171d41b9b0",
        },
        PeclBuild {
            branch: "8.2",
            compiler: "vs16",
            sha256: "b2b730b99b97352212b338c01f7dde577857e31c5f8e1d011ec2871e63f8f87c",
        },
        PeclBuild {
            branch: "8.3",
            compiler: "vs16",
            sha256: "519fc1bdf54323d3ab08443c66f77e783d314c13b532e27cc8b390def7f81b60",
        },
        PeclBuild {
            branch: "8.4",
            compiler: "vs17",
            sha256: "6db881ed172703962002d7e02dacc8ddb3f789904c648fee3e6ceec1319679cd",
        },
        PeclBuild {
            branch: "8.5",
            compiler: "vs17",
            sha256: "481d6d1af45060ab41af6abe250faa270276fc47a493badc2e178024cdf6e255",
        },
    ],
}];

/// What the UI needs to decide between "install it" and "not available".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatus {
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    /// The DLL is already in this version's `ext/`.
    pub installed: bool,
    /// A verified build exists for this PHP branch.
    pub available: bool,
}

/// `8.5.10` → `8.5`. A hand-dropped folder named something else simply has no
/// branch, and so is offered nothing rather than guessed at.
fn branch_of(php_version: &str) -> Option<String> {
    let mut parts = php_version.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    (major.chars().all(|c| c.is_ascii_digit()) && minor.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("{major}.{minor}"))
}

pub fn find(id: &str) -> Result<&'static PeclExtension, AppError> {
    CATALOG
        .iter()
        .find(|extension| extension.id == id)
        .ok_or_else(|| AppError::UnknownBinary(id.to_string()))
}

fn build_for(extension: &'static PeclExtension, php_version: &str) -> Option<&'static PeclBuild> {
    let branch = branch_of(php_version)?;
    extension.builds.iter().find(|build| build.branch == branch)
}

/// `php_redis-6.3.0-8.5-nts-vs17-x64.zip` — the shape php.net's PECL area uses.
fn archive_name(extension: &PeclExtension, build: &PeclBuild) -> String {
    format!(
        "php_{id}-{version}-{branch}-nts-{compiler}-x64.zip",
        id = extension.id,
        version = extension.version,
        branch = build.branch,
        compiler = build.compiler
    )
}

fn download_url(extension: &PeclExtension, build: &PeclBuild) -> String {
    format!(
        "{PECL_BASE}/{id}/{version}/{archive}",
        id = extension.id,
        version = extension.version,
        archive = archive_name(extension, build)
    )
}

/// The DLL's name inside a PHP install's `ext/` folder.
fn dll_name(extension: &PeclExtension) -> String {
    format!("php_{}.dll", extension.id)
}

fn is_installed(php_dir: &Path, extension: &PeclExtension) -> bool {
    php_dir.join("ext").join(dll_name(extension)).is_file()
}

/// The folder of an installed PHP version, by the id the Switch page uses.
fn php_dir_for(php_version: &str) -> Result<PathBuf, AppError> {
    php::installed()
        .into_iter()
        .find(|runtime| runtime.version == php_version)
        .map(|runtime| runtime.dir)
        .ok_or_else(|| AppError::PhpVersionNotFound(php_version.to_string()))
}

/// Every catalog entry, answered for one PHP version.
pub fn status_for(php_version: &str) -> Result<Vec<ExtensionStatus>, AppError> {
    let php_dir = php_dir_for(php_version)?;

    Ok(CATALOG
        .iter()
        .map(|extension| ExtensionStatus {
            id: extension.id.to_string(),
            name: extension.name.to_string(),
            version: extension.version.to_string(),
            summary: extension.summary.to_string(),
            installed: is_installed(&php_dir, extension),
            available: build_for(extension, php_version).is_some(),
        })
        .collect())
}

/// Downloads, verifies and installs one extension into a PHP version.
///
/// Idempotent: an extension already in that version's `ext/` returns without
/// touching the network.
///
/// The archive carries documentation, a licence and a `.pdb` alongside the
/// DLL, so it is unpacked into a staging folder and only the DLL is moved
/// across. Extracting it straight into `ext/` would leave `README.md` and
/// `liblzf/` sitting among the extension binaries forever.
pub async fn install(app: &AppHandle, id: &str, php_version: &str) -> Result<(), AppError> {
    let extension = find(id)?;
    let php_dir = php_dir_for(php_version)?;

    if is_installed(&php_dir, extension) {
        return Ok(());
    }

    let build =
        build_for(extension, php_version).ok_or_else(|| AppError::ExtensionUnavailable {
            id: extension.id.to_string(),
            php_version: php_version.to_string(),
        })?;

    let dll = dll_name(extension);
    let staging = paths::data()?.join("ext-downloads").join(format!(
        "{}-{}-{}",
        extension.id, extension.version, build.branch
    ));
    // A previous run that died mid-way would otherwise be mistaken for a
    // finished extraction.
    let _ = std::fs::remove_dir_all(&staging);

    binaries::install_archive(
        app,
        &ArchiveInstall {
            id: extension.id,
            label: extension.name,
            download_url: &download_url(extension, build),
            sha256: build.sha256,
            dest_dir: staging.clone(),
            exe_relative_path: &dll,
        },
    )
    .await?;

    let ext_dir = php_dir.join("ext");
    std::fs::create_dir_all(&ext_dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", ext_dir.display())))?;
    std::fs::copy(staging.join(&dll), ext_dir.join(&dll))
        .map_err(|e| AppError::Io(format!("could not install {dll}: {e}")))?;
    let _ = std::fs::remove_dir_all(&staging);

    // The generated ini picks this up on its own — it enables whatever is
    // really in `ext/` — but the ini inside the version folder was written
    // once and is never rewritten, so the `php` in a terminal would keep
    // reporting the extension missing.
    if let Err(err) = php_ini::ensure_extension_enabled(&php_dir, extension.id) {
        log::warn!("installed {dll} but could not enable it for the CLI: {err}");
    }

    log::info!("installed {dll} into {}", ext_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_maps_to_its_branch_and_anything_else_maps_to_nothing() {
        assert_eq!(branch_of("8.5.10").as_deref(), Some("8.5"));
        assert_eq!(branch_of("7.4.33").as_deref(), Some("7.4"));
        // A hand-dropped folder can be named anything; guessing a branch for
        // it would mean downloading a DLL built for another PHP.
        assert_eq!(branch_of("my-php"), None);
        assert_eq!(branch_of("8"), None);
    }

    /// The URL is derived from the table, so a wrong shape here would mean
    /// every download 404s.
    #[test]
    fn the_download_url_matches_php_nets_pecl_layout() {
        let redis = find("redis").unwrap();
        let build = build_for(redis, "8.5.10").expect("8.5 must be covered");

        assert_eq!(
            download_url(redis, build),
            "https://downloads.php.net/~windows/pecl/releases/redis/6.3.0/\
             php_redis-6.3.0-8.5-nts-vs17-x64.zip"
        );
        assert_eq!(dll_name(redis), "php_redis.dll");
    }

    /// Every branch php.net currently publishes PHP for has to be covered, or
    /// the feature silently stops existing for whoever installs the newest
    /// version.
    #[test]
    fn every_supported_branch_has_a_pinned_build() {
        let redis = find("redis").unwrap();
        for branch in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"] {
            let version = format!("{branch}.0");
            assert!(
                build_for(redis, &version).is_some(),
                "no pinned build for PHP {branch}"
            );
        }
    }

    /// Every pin has to be a real SHA-256, since a malformed one would only
    /// surface at the end of a download.
    #[test]
    fn every_pinned_checksum_is_a_sha256() {
        for extension in CATALOG {
            for build in extension.builds {
                assert_eq!(
                    build.sha256.len(),
                    64,
                    "{} {} checksum is not 64 hex chars",
                    extension.id,
                    build.branch
                );
                assert!(build.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            }
        }
    }

    #[test]
    fn an_unknown_extension_is_refused() {
        assert!(find("mongodb").is_err());
    }
}
