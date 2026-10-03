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
//!
//! # Microsoft's SQL Server drivers
//!
//! `sqlsrv` and `pdo_sqlsrv` are published to the same PECL area, but each
//! driver release only targets the PHP branches current when it shipped —
//! 5.13 starts at PHP 8.3, so 8.1 and 8.2 are served by 5.12, and 7.4 and
//! 8.0 by 5.10. A build can therefore name its own version, overriding the
//! extension's.
//!
//! Both also need the Microsoft ODBC Driver installed into Windows
//! (`services::odbc`). The DLL loads without it — PHP reports nothing
//! wrong — and the first connect fails instead; `requires_odbc` is what
//! lets the UI say so up front.

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
    /// The extension version built for this branch, when it isn't the
    /// extension's own `version` — see the module docs.
    pub version: Option<&'static str>,
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
    /// Needs the Microsoft ODBC Driver for SQL Server to actually connect.
    pub requires_odbc: bool,
    pub builds: &'static [PeclBuild],
}

/// Hashes verified against the archives — `redis` on 2026-09-05, the two
/// SQL Server drivers on 2026-10-03.
pub const CATALOG: &[PeclExtension] = &[
    PeclExtension {
    id: "redis",
    name: "Redis",
    version: "6.3.0",
    summary: "Queues, cache and Horizon in Laravel projects that require ext-redis.",
    requires_odbc: false,
    builds: &[
        PeclBuild {
            branch: "7.4",
            compiler: "vc15",
            version: None,
            sha256: "c74f1fb5500b493050839330b4cbfb84dafc07da99b898939ac4b690fe74de1a",
        },
        PeclBuild {
            branch: "8.0",
            compiler: "vs16",
            version: None,
            sha256: "9d3143049d3c27e715ea92a023dfab92389df985aec224d6ed9d092b4ba5ea9a",
        },
        PeclBuild {
            branch: "8.1",
            compiler: "vs16",
            version: None,
            sha256: "952ec845408e343f273eab109e62a9c7732178d93985120736da4a171d41b9b0",
        },
        PeclBuild {
            branch: "8.2",
            compiler: "vs16",
            version: None,
            sha256: "b2b730b99b97352212b338c01f7dde577857e31c5f8e1d011ec2871e63f8f87c",
        },
        PeclBuild {
            branch: "8.3",
            compiler: "vs16",
            version: None,
            sha256: "519fc1bdf54323d3ab08443c66f77e783d314c13b532e27cc8b390def7f81b60",
        },
        PeclBuild {
            branch: "8.4",
            compiler: "vs17",
            version: None,
            sha256: "6db881ed172703962002d7e02dacc8ddb3f789904c648fee3e6ceec1319679cd",
        },
        PeclBuild {
            branch: "8.5",
            compiler: "vs17",
            version: None,
            sha256: "481d6d1af45060ab41af6abe250faa270276fc47a493badc2e178024cdf6e255",
        },
    ],
    },
    PeclExtension {
        id: "pdo_sqlsrv",
        name: "PDO SQL Server",
        version: "5.13.3",
        summary: "Microsoft's SQL Server driver for PDO — what Laravel's sqlsrv connection uses.",
        requires_odbc: true,
        builds: &[
            PeclBuild {
                branch: "7.4",
                compiler: "vc15",
                version: Some("5.10.0"),
                sha256: "66f5d3b25289be7b6e897641617260d5c218e727cd4c68dae14dd3239b69e102",
            },
            PeclBuild {
                branch: "8.0",
                compiler: "vs16",
                version: Some("5.10.0"),
                sha256: "812aa3541e89d6df7f27d0f56c786a73ba4ee56b185f3448ef825d1aa1bda350",
            },
            PeclBuild {
                branch: "8.1",
                compiler: "vs16",
                version: Some("5.12.0"),
                sha256: "2a5b38e19c4bab76644398b5bf9ded660726f59d4e86464991140a48239275fe",
            },
            PeclBuild {
                branch: "8.2",
                compiler: "vs16",
                version: Some("5.12.0"),
                sha256: "d2a81c2d51976f039335a6c0ea10acd90c69390c83bf75253688eb6993acad97",
            },
            PeclBuild {
                branch: "8.3",
                compiler: "vs16",
                version: None,
                sha256: "0561c4fa3dc764899d6c366af4893fb12c1d36353a6c6eb2398016e429d465b4",
            },
            PeclBuild {
                branch: "8.4",
                compiler: "vs17",
                version: None,
                sha256: "fd31b57ea01f30e13564e9f12231a0553d64e303f4dc89856efc0514a2e28159",
            },
            PeclBuild {
                branch: "8.5",
                compiler: "vs17",
                version: None,
                sha256: "63bbda9081266cba12c46071324166e97434aa8366e47d8122122ff5e6cffb36",
            },
        ],
    },
    PeclExtension {
        id: "sqlsrv",
        name: "SQL Server",
        version: "5.13.3",
        summary: "Microsoft's procedural SQL Server driver (sqlsrv_* functions), for code that doesn't go through PDO.",
        requires_odbc: true,
        builds: &[
            PeclBuild {
                branch: "7.4",
                compiler: "vc15",
                version: Some("5.10.0"),
                sha256: "60baa18459e4d1683661b001abc2e4cdd5462a8c2447d9b5e516ee0315eb71ac",
            },
            PeclBuild {
                branch: "8.0",
                compiler: "vs16",
                version: Some("5.10.0"),
                sha256: "13fc71e39f3f7d7c0e07e975f7ce6a9c5b0198cd84ecacdac554f26011f1c57b",
            },
            PeclBuild {
                branch: "8.1",
                compiler: "vs16",
                version: Some("5.12.0"),
                sha256: "00aefd8a92a518086d58fec98f82949a08177ee9f65974be9ebd2e3c6f78b3c2",
            },
            PeclBuild {
                branch: "8.2",
                compiler: "vs16",
                version: Some("5.12.0"),
                sha256: "90861d8a5f19339a16c5a01afb1291e2043fdf6cbaa0efe07d2972c75709e5ab",
            },
            PeclBuild {
                branch: "8.3",
                compiler: "vs16",
                version: None,
                sha256: "1a9d8cc6819e92ccfd19d67893abf643de1093b1ef7106402e2ae668110298db",
            },
            PeclBuild {
                branch: "8.4",
                compiler: "vs17",
                version: None,
                sha256: "e47854a5d84adf2d78911ccdaa8b5bfd1048e11dab90ca64ad168f719fb4701f",
            },
            PeclBuild {
                branch: "8.5",
                compiler: "vs17",
                version: None,
                sha256: "d38dbc0f7e2dcd99237fc9ec2fba07ab547d13dd3fc76b343ceed842f9539a41",
            },
        ],
    },
];

/// What the UI needs to decide between "install it" and "not available".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatus {
    pub id: String,
    pub name: String,
    /// The version that would be installed for *this* PHP — which, for the
    /// SQL Server drivers, depends on the branch.
    pub version: String,
    pub summary: String,
    /// The DLL is already in this version's `ext/`.
    pub installed: bool,
    /// A verified build exists for this PHP branch.
    pub available: bool,
    /// Needs the Microsoft ODBC Driver to connect — see the module docs.
    pub requires_odbc: bool,
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

/// The version a build is of — its own, or the extension's.
fn version_of(extension: &PeclExtension, build: &PeclBuild) -> &'static str {
    build.version.unwrap_or(extension.version)
}

/// `php_redis-6.3.0-8.5-nts-vs17-x64.zip` — the shape php.net's PECL area uses.
fn archive_name(extension: &PeclExtension, build: &PeclBuild) -> String {
    format!(
        "php_{id}-{version}-{branch}-nts-{compiler}-x64.zip",
        id = extension.id,
        version = version_of(extension, build),
        branch = build.branch,
        compiler = build.compiler
    )
}

fn download_url(extension: &PeclExtension, build: &PeclBuild) -> String {
    format!(
        "{PECL_BASE}/{id}/{version}/{archive}",
        id = extension.id,
        version = version_of(extension, build),
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
        .map(|extension| {
            let build = build_for(extension, php_version);
            ExtensionStatus {
                id: extension.id.to_string(),
                name: extension.name.to_string(),
                version: build
                    .map(|build| version_of(extension, build))
                    .unwrap_or(extension.version)
                    .to_string(),
                summary: extension.summary.to_string(),
                installed: is_installed(&php_dir, extension),
                available: build.is_some(),
                requires_odbc: extension.requires_odbc,
            }
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
        extension.id,
        version_of(extension, build),
        build.branch
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
            keep: &[],
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
        for extension in CATALOG {
            for branch in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"] {
                let version = format!("{branch}.0");
                assert!(
                    build_for(extension, &version).is_some(),
                    "no pinned {} build for PHP {branch}",
                    extension.id
                );
            }
        }
    }

    /// Microsoft's newer driver releases dropped the older branches, so the
    /// URL has to carry the version built for *that* branch.
    #[test]
    fn an_older_php_gets_the_driver_release_that_still_targeted_it() {
        let pdo = find("pdo_sqlsrv").unwrap();
        assert_eq!(
            download_url(pdo, build_for(pdo, "7.4.33").unwrap()),
            "https://downloads.php.net/~windows/pecl/releases/pdo_sqlsrv/5.10.0/\
             php_pdo_sqlsrv-5.10.0-7.4-nts-vc15-x64.zip"
        );
        assert_eq!(
            download_url(pdo, build_for(pdo, "8.5.10").unwrap()),
            "https://downloads.php.net/~windows/pecl/releases/pdo_sqlsrv/5.13.3/\
             php_pdo_sqlsrv-5.13.3-8.5-nts-vs17-x64.zip"
        );
        assert!(pdo.requires_odbc);
        assert!(!find("redis").unwrap().requires_odbc);
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

    /// Downloads every pinned build from php.net for real and checks it
    /// against its pin, and that the DLL the installer copies out is really
    /// in the archive — the only way to catch a mistyped hash or a moved
    /// file before a user's Install click does. Run with:
    /// `cargo test --lib services::php_ext::tests::every_pinned_build_matches_its_download -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn every_pinned_build_matches_its_download() {
        use sha2::{Digest, Sha256};

        for extension in CATALOG {
            for build in extension.builds {
                let url = download_url(extension, build);
                let bytes = reqwest::get(&url)
                    .await
                    .and_then(|r| r.error_for_status())
                    .unwrap_or_else(|e| panic!("{url}: {e}"))
                    .bytes()
                    .await
                    .unwrap();
                let actual = format!("{:x}", Sha256::digest(&bytes));

                let archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
                let has_dll = archive.file_names().any(|name| name == dll_name(extension));

                println!(
                    "{} {} {:>8} bytes  sha256 {}  dll {}",
                    extension.id,
                    build.branch,
                    bytes.len(),
                    if actual == build.sha256 {
                        "ok"
                    } else {
                        "MISMATCH"
                    },
                    if has_dll { "ok" } else { "MISSING" },
                );
                assert_eq!(actual, build.sha256, "{url}");
                assert!(has_dll, "{url} has no {}", dll_name(extension));
            }
        }
    }
}
