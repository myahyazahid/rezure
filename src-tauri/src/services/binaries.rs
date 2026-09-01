//! On-demand portable binary downloader.
//!
//! Per `CLAUDE.md`, portable service binaries (Nginx, PHP, MariaDB) are never
//! committed to git — they're fetched from each project's official
//! distribution on first use, checksum-verified, and cached under the user's
//! local app data directory. Every entry in [`MANIFEST`] is a real, currently
//! published Windows portable/zip release.
//!
//! **Exception:** Nginx and the default PHP version (8.3.33) can additionally
//! arrive pre-staged inside the installer, so a fresh install serves PHP
//! immediately with no first-run download. `scripts/stage-bundled-binaries.ps1`
//! fetches those two into a gitignored `src-tauri/bundled-bin/` before a
//! release build — never committed, same as everything else here — and
//! `tauri.conf.json`'s `bundle.resources` embeds that folder in the `.msi`/
//! `.exe`. [`seed_bundled`] is what copies it into the real install root on
//! first launch. MariaDB and every other PHP version stay purely on-demand.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::utils::error::AppError;
use crate::utils::paths;

/// A downloadable portable binary package.
#[derive(Debug, Clone, Copy)]
pub struct BinaryPackage {
    /// Unique across the whole manifest (`"php-8.3.33"`, not just `"php"`)
    /// — a family can have more than one installable version.
    pub id: &'static str,
    /// Groups every installable version of the same runtime together
    /// (`"php"` for every PHP entry) — see [`family_packages`]. Also the
    /// on-disk cache folder shared by every version in the family:
    /// `bin/<family>/<version>/`.
    pub family: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub download_url: &'static str,
    pub sha256: &'static str,
    /// Path to the main executable inside the extracted archive — doubles as
    /// the install-completion check and the path a `Service` spawns.
    pub exe_relative_path: &'static str,
}

pub const MANIFEST: &[BinaryPackage] = &[
    BinaryPackage {
        id: "nginx",
        family: "nginx",
        name: "Nginx",
        version: "1.25.3",
        download_url: "https://nginx.org/download/nginx-1.25.3.zip",
        sha256: "58df6e5865a922aaa477ac89b79c13739347a37ccc4b3de58de91f1487710cc4",
        exe_relative_path: "nginx-1.25.3/nginx.exe",
    },
    // Every PHP entry is a separate downloadable version, in newest-first
    // order — `services::php` picks `.first()` of `family_packages("php")`
    // as the default active version, and the Switch UI lists them in this
    // order too.
    BinaryPackage {
        id: "php-8.3.33",
        family: "php",
        name: "PHP",
        version: "8.3.33",
        download_url:
            "https://downloads.php.net/~windows/releases/php-8.3.33-nts-Win32-vs16-x64.zip",
        sha256: "534399107056313246f424adbbb7937337e40fbbf6aa7bc26287ba9cfd2e4a2a",
        exe_relative_path: "php.exe",
    },
    BinaryPackage {
        id: "php-8.2.33",
        family: "php",
        name: "PHP",
        version: "8.2.33",
        download_url:
            "https://downloads.php.net/~windows/releases/php-8.2.33-nts-Win32-vs16-x64.zip",
        sha256: "d0bd189522fa50255ee94ed4b340ed4330f5ae33a90a74205275b0f0b221d388",
        exe_relative_path: "php.exe",
    },
    BinaryPackage {
        id: "php-8.1.34",
        family: "php",
        name: "PHP",
        version: "8.1.34",
        download_url:
            "https://downloads.php.net/~windows/releases/php-8.1.34-nts-Win32-vs16-x64.zip",
        sha256: "9cfe246cb144076c16f5913a3ef88a474c3dd7e60f0f0c8bb95faf68674016cc",
        exe_relative_path: "php.exe",
    },
    BinaryPackage {
        id: "mariadb",
        family: "mariadb",
        name: "MariaDB",
        version: "11.2.2",
        download_url:
            "https://archive.mariadb.org/mariadb-11.2.2/winx64-packages/mariadb-11.2.2-winx64.zip",
        sha256: "7d40de0c468cf33b5e8283e6f67d315aeae6ffa8df052b8d20a9b5943598d35e",
        exe_relative_path: "mariadb-11.2.2-winx64/bin/mysqld.exe",
    },
];

pub fn find(id: &str) -> Result<&'static BinaryPackage, AppError> {
    MANIFEST
        .iter()
        .find(|pkg| pkg.id == id)
        .ok_or_else(|| AppError::UnknownBinary(id.to_string()))
}

/// Every package sharing a `family`, in manifest order (newest version
/// first, by convention — see the PHP entries above).
pub fn family_packages(family: &str) -> Vec<&'static BinaryPackage> {
    MANIFEST.iter().filter(|pkg| pkg.family == family).collect()
}

/// `C:\rezure\bin` — where Rezure's own downloads land, created lazily on
/// first install.
pub fn install_root() -> Result<PathBuf, AppError> {
    paths::bin()
}

/// `C:\rezure\custom` — the drop-in folder, for runtimes the user downloaded
/// themselves.
///
/// Sits beside `www` and `dumps` in the one visible Rezure folder rather than
/// buried in `AppData`: this one is meant to be opened in Explorer and dropped
/// into, the way Laragon's `bin\` is. The filesystem is the whole registry —
/// there is no list of custom paths to persist and keep in sync, so a folder
/// appearing here *is* an installed version, and deleting it uninstalls one.
pub fn user_bin_root() -> Result<PathBuf, AppError> {
    paths::custom_bin()
}

/// One runtime version found on disk, from either root.
#[derive(Debug, Clone)]
pub struct InstalledRuntime {
    /// Version as shown in the UI — pulled out of the folder name, or the
    /// folder name itself when it carries no version number.
    pub version: String,
    pub dir: PathBuf,
    pub exe: PathBuf,
    /// `false` for anything under [`user_bin_root`] — the UI marks those as
    /// added by hand, since Rezure never checksum-verified them.
    pub managed: bool,
}

/// The first `1.2.3`-shaped (or `1.2`-shaped) token in `name`.
///
/// Official archives unpack to folders like `php-8.4.25-nts-Win32-vs17-x64`,
/// and Rezure's own installs to a bare `8.4.25`; both should read as
/// "8.4.25" in the Switch dropdown.
pub fn version_from_folder_name(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut start = 0;

    while start < chars.len() {
        if !chars[start].is_ascii_digit() {
            start += 1;
            continue;
        }

        let mut end = start;
        let mut dots = 0;
        while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.') {
            if chars[end] == '.' {
                // A trailing dot is not part of the version ("8.4." -> "8.4").
                if end + 1 >= chars.len() || !chars[end + 1].is_ascii_digit() {
                    break;
                }
                dots += 1;
            }
            end += 1;
        }

        if dots >= 1 {
            return Some(chars[start..end].iter().collect());
        }
        start = end.max(start + 1);
    }
    None
}

/// Every place an archive might have left the executable, in the order
/// they're tried.
///
/// Four layouts, because the runtimes Rezure supports genuinely differ:
/// PHP's zip puts `php.exe` at the top (`<dir>/php.exe`), and a
/// hand-extracted one nests it in the archive's own folder
/// (`<dir>/php-8.4.25-.../php.exe`). Database servers add a `bin`:
/// MariaDB's zip lands `mysqld.exe` at
/// `<dir>/mariadb-11.2.2-winx64/bin/mysqld.exe` — two levels down, which
/// is why looking only one deep found no installed server at all.
fn exe_candidates(dir: &Path, exe_name: &str) -> Vec<PathBuf> {
    let mut candidates = vec![dir.join(exe_name), dir.join("bin").join(exe_name)];

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let nested = entry.path();
            candidates.push(nested.join(exe_name));
            candidates.push(nested.join("bin").join(exe_name));
        }
    }
    candidates
}

/// Numeric-segment comparison, so `8.10.0` sorts above `8.9.0` the way a
/// plain string compare would not.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let ordering = a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0));
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

/// Every version of `family` present on disk, newest first.
///
/// Scans both roots, so a version Rezure downloaded and one the user
/// dropped in by hand are equally "installed" — that is what makes the
/// drop-in folder work with no registration step. Rezure's own installs
/// win a version collision, since those are the checksum-verified copies.
pub fn discover(family: &str, exe_name: &str) -> Vec<InstalledRuntime> {
    let roots = [
        install_root().map(|root| (root, true)),
        user_bin_root().map(|root| (root, false)),
    ];

    let mut found: Vec<InstalledRuntime> = Vec::new();
    for (root, managed) in roots.into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(root.join(family)) else {
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(folder_name) = dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            let exe = exe_candidates(&dir, exe_name)
                .into_iter()
                .find(|candidate| candidate.is_file());
            let Some(exe) = exe else { continue };

            // Walks back up from the executable when the folder it was found
            // under carries no version — for a `bin/` layout the useful name
            // is the grandparent (`mariadb-11.2.2-winx64`), not `bin`.
            let version = version_from_folder_name(folder_name)
                .or_else(|| {
                    exe.ancestors()
                        .skip(1)
                        .take(3)
                        .filter_map(|dir| dir.file_name().and_then(|n| n.to_str()))
                        .find_map(version_from_folder_name)
                })
                .unwrap_or_else(|| folder_name.to_string());

            if found.iter().any(|existing| existing.version == version) {
                continue;
            }
            found.push(InstalledRuntime {
                version,
                dir,
                exe,
                managed,
            });
        }
    }

    found.sort_by(|a, b| compare_versions(&b.version, &a.version));
    found
}

fn package_dir(pkg: &BinaryPackage) -> Result<PathBuf, AppError> {
    Ok(install_root()?.join(pkg.family).join(pkg.version))
}

/// Where `pkg`'s main executable lives once installed, whether or not it
/// actually is yet.
pub fn exe_path(pkg: &BinaryPackage) -> Result<PathBuf, AppError> {
    Ok(package_dir(pkg)?.join(pkg.exe_relative_path))
}

pub fn is_installed(pkg: &BinaryPackage) -> bool {
    exe_path(pkg).map(|path| path.is_file()).unwrap_or(false)
}

/// Copies whatever `scripts/stage-bundled-binaries.ps1` staged into the
/// installer (see this module's doc comment) into the real `install_root()`,
/// for Nginx and the default PHP version only — a no-op the moment either is
/// already installed, so this never overwrites a real download or a version
/// the user switched away from.
///
/// A no-op on a dev run too, or any install that predates this feature:
/// `resource_dir()` either fails to resolve or simply has no `bundled-bin`
/// folder under it, and both cases are treated the same as "nothing to seed"
/// rather than an error — this must never block startup.
pub fn seed_bundled(app: &AppHandle) {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return;
    };
    let bundled_bin = resource_dir.join("bundled-bin");
    if !bundled_bin.is_dir() {
        return;
    }

    for pkg in MANIFEST
        .iter()
        .filter(|pkg| pkg.family == "nginx" || pkg.id == "php-8.3.33")
    {
        if is_installed(pkg) {
            continue;
        }
        let src = bundled_bin.join(pkg.family).join(pkg.version);
        if !src.is_dir() {
            continue;
        }
        let Ok(dest) = package_dir(pkg) else { continue };
        if let Err(err) = copy_dir_recursive(&src, &dest) {
            log::warn!("failed to seed bundled {}: {err}", pkg.id);
        }
    }
}

/// Recursively copies `src` into `dest`, creating `dest` and any nested
/// directories as needed. `std::fs` has no built-in equivalent of `cp -r`.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dest)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dest.display())))?;

    for entry in std::fs::read_dir(src)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", src.display())))?
    {
        let entry = entry.map_err(|e| AppError::Io(e.to_string()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).map_err(|e| {
                AppError::Io(format!(
                    "could not copy {} to {}: {e}",
                    src_path.display(),
                    dest_path.display()
                ))
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryStatus {
    pub id: String,
    pub name: String,
    pub version: String,
    pub installed: bool,
}

fn status_of(pkg: &BinaryPackage) -> BinaryStatus {
    BinaryStatus {
        id: pkg.id.to_string(),
        name: pkg.name.to_string(),
        version: pkg.version.to_string(),
        installed: is_installed(pkg),
    }
}

pub fn list_status() -> Vec<BinaryStatus> {
    MANIFEST.iter().map(status_of).collect()
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallStage {
    Downloading,
    Verifying,
    Extracting,
    Done,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub id: String,
    pub stage: InstallStage,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

/// Event name the frontend subscribes to via `listen()` for install progress.
pub const PROGRESS_EVENT: &str = "binary://install-progress";

fn emit_progress(app: &AppHandle, progress: &InstallProgress) {
    // Best-effort — a dropped event shouldn't abort an otherwise-fine install.
    if let Err(err) = app.emit(PROGRESS_EVENT, progress) {
        log::warn!("failed to emit binary install progress: {err}");
    }
}

/// What an install needs to know, whether it came from [`MANIFEST`] or from
/// a catalog fetched at runtime (see `services::php_catalog`).
///
/// Extracted so the live PHP catalog gets the *same* download, checksum and
/// zip-slip handling as the pinned manifest entries — a version discovered
/// over the network is not a reason to verify it any less.
pub struct ArchiveInstall<'a> {
    /// Identifies this install in [`PROGRESS_EVENT`] payloads.
    pub id: &'a str,
    pub label: &'a str,
    pub download_url: &'a str,
    pub sha256: &'a str,
    pub dest_dir: PathBuf,
    /// Checked after extraction, relative to `dest_dir`.
    pub exe_relative_path: &'a str,
}

/// Downloads, checksum-verifies, and extracts a portable binary package,
/// emitting [`PROGRESS_EVENT`] as it goes.
pub async fn install_archive(app: &AppHandle, spec: &ArchiveInstall<'_>) -> Result<(), AppError> {
    std::fs::create_dir_all(&spec.dest_dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", spec.dest_dir.display())))?;

    let archive_bytes = download(app, spec.id, spec.download_url).await?;

    emit_progress(
        app,
        &InstallProgress {
            id: spec.id.to_string(),
            stage: InstallStage::Verifying,
            downloaded_bytes: archive_bytes.len() as u64,
            total_bytes: Some(archive_bytes.len() as u64),
        },
    );
    verify_checksum(spec.id, spec.sha256, &archive_bytes)?;

    emit_progress(
        app,
        &InstallProgress {
            id: spec.id.to_string(),
            stage: InstallStage::Extracting,
            downloaded_bytes: archive_bytes.len() as u64,
            total_bytes: Some(archive_bytes.len() as u64),
        },
    );

    let extract_dir = spec.dest_dir.clone();
    tokio::task::spawn_blocking(move || extract(&archive_bytes, &extract_dir))
        .await
        .map_err(|e| AppError::Extract(format!("extraction task panicked: {e}")))??;

    if !spec.dest_dir.join(spec.exe_relative_path).is_file() {
        // A half-extracted folder would otherwise read as an installed
        // version on the next scan, so it goes rather than lingering.
        let _ = std::fs::remove_dir_all(&spec.dest_dir);
        return Err(AppError::Extract(format!(
            "expected {} after extracting {}, but it was not found",
            spec.exe_relative_path, spec.label
        )));
    }

    emit_progress(
        app,
        &InstallProgress {
            id: spec.id.to_string(),
            stage: InstallStage::Done,
            downloaded_bytes: 0,
            total_bytes: None,
        },
    );

    Ok(())
}

/// Installs a pinned [`MANIFEST`] entry. Idempotent — if the executable is
/// already present this returns immediately without hitting the network.
pub async fn install(app: &AppHandle, id: &str) -> Result<BinaryStatus, AppError> {
    let pkg = find(id)?;

    if is_installed(pkg) {
        return Ok(status_of(pkg));
    }

    install_archive(
        app,
        &ArchiveInstall {
            id: pkg.id,
            label: pkg.name,
            download_url: pkg.download_url,
            sha256: pkg.sha256,
            dest_dir: package_dir(pkg)?,
            exe_relative_path: pkg.exe_relative_path,
        },
    )
    .await?;

    Ok(status_of(pkg))
}

async fn download(app: &AppHandle, id: &str, url: &str) -> Result<Vec<u8>, AppError> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| AppError::Download(format!("{url}: {e}")))?;

    if !response.status().is_success() {
        return Err(AppError::Download(format!(
            "{url} responded with {}",
            response.status()
        )));
    }

    let total_bytes = response.content_length();
    let mut downloaded = Vec::with_capacity(total_bytes.unwrap_or(0) as usize);
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Download(format!("{url}: {e}")))?;
        downloaded.extend_from_slice(&chunk);
        emit_progress(
            app,
            &InstallProgress {
                id: id.to_string(),
                stage: InstallStage::Downloading,
                downloaded_bytes: downloaded.len() as u64,
                total_bytes,
            },
        );
    }

    Ok(downloaded)
}

fn verify_checksum(id: &str, expected: &str, bytes: &[u8]) -> Result<(), AppError> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = hex::encode(hasher.finalize());

    if actual != expected {
        return Err(AppError::ChecksumMismatch {
            id: id.to_string(),
            expected: expected.to_string(),
            actual,
        });
    }

    Ok(())
}

/// Extracts a zip archive into `dest_dir`, rejecting any entry whose path
/// would escape it (zip-slip).
fn extract(bytes: &[u8], dest_dir: &Path) -> Result<(), AppError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| AppError::Extract(e.to_string()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| AppError::Extract(e.to_string()))?;

        let relative_path = match entry.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => {
                return Err(AppError::Extract(format!(
                    "archive entry {} has an unsafe path",
                    entry.name()
                )));
            }
        };

        let out_path = dest_dir.join(relative_path);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| AppError::Io(e.to_string()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
        }

        let mut out_file =
            std::fs::File::create(&out_path).map_err(|e| AppError::Io(e.to_string()))?;
        std::io::copy(&mut entry, &mut out_file).map_err(|e| AppError::Io(e.to_string()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_dir_recursive_reproduces_nested_files_and_folders() {
        let src = std::env::temp_dir().join(format!("rezure-test-copy-src-{}", std::process::id()));
        let dest =
            std::env::temp_dir().join(format!("rezure-test-copy-dest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dest);

        std::fs::create_dir_all(src.join("bin")).unwrap();
        std::fs::write(src.join("top.txt"), b"top").unwrap();
        std::fs::write(src.join("bin").join("nested.exe"), b"nested").unwrap();

        copy_dir_recursive(&src, &dest).unwrap();

        assert_eq!(std::fs::read(dest.join("top.txt")).unwrap(), b"top");
        assert_eq!(
            std::fs::read(dest.join("bin").join("nested.exe")).unwrap(),
            b"nested"
        );

        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn version_is_read_out_of_an_official_archive_folder_name() {
        // The shape a hand-extracted php.net zip actually leaves behind.
        assert_eq!(
            version_from_folder_name("php-8.4.25-nts-Win32-vs17-x64").as_deref(),
            Some("8.4.25")
        );
        assert_eq!(
            version_from_folder_name("php-7.4.33-nts-Win32-vc15-x64").as_deref(),
            Some("7.4.33")
        );
        // Rezure's own installs are named by the bare version.
        assert_eq!(
            version_from_folder_name("8.3.33").as_deref(),
            Some("8.3.33")
        );
        assert_eq!(version_from_folder_name("8.1").as_deref(), Some("8.1"));
    }

    #[test]
    fn a_folder_name_with_no_version_reads_as_none() {
        assert_eq!(version_from_folder_name("php"), None);
        assert_eq!(version_from_folder_name("my-build"), None);
        // A lone number is not a version — "8" tells us nothing to sort by.
        assert_eq!(version_from_folder_name("php8"), None);
    }

    /// A trailing dot belongs to the surrounding name, not the version.
    #[test]
    fn a_trailing_dot_is_not_part_of_the_version() {
        assert_eq!(version_from_folder_name("php-8.4.").as_deref(), Some("8.4"));
    }

    #[test]
    fn versions_compare_by_number_not_by_string() {
        use std::cmp::Ordering;
        // The case a plain string compare gets wrong.
        assert_eq!(compare_versions("8.10.0", "8.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("8.4.25", "8.4.3"), Ordering::Greater);
        assert_eq!(compare_versions("8.3.33", "8.3.33"), Ordering::Equal);
        assert_eq!(compare_versions("7.4.33", "8.0.30"), Ordering::Less);
        // A missing segment reads as zero, so "8.4" sits below "8.4.1".
        assert_eq!(compare_versions("8.4", "8.4.1"), Ordering::Less);
    }

    #[test]
    fn discover_finds_a_dropped_in_folder_and_marks_it_unmanaged() {
        // Runs against the real drop-in root — the point is to prove that a
        // folder appearing there is all it takes, with no registration step.
        let root = match user_bin_root() {
            Ok(root) => root.join("php"),
            Err(_) => return,
        };
        let dir = root.join("php-9.9.9-test-nts-Win32-x64");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("php.exe"), b"not a real binary").unwrap();

        let found = discover("php", "php.exe");
        let dropped = found.iter().find(|runtime| runtime.version == "9.9.9");

        // Clean up before asserting, so a failure doesn't leave the fake
        // version behind in the user's real folder.
        let _ = std::fs::remove_dir_all(&dir);

        let dropped = dropped.expect("a folder in the drop-in root must be discovered");
        assert!(
            !dropped.managed,
            "anything under the user's own bin folder is unmanaged"
        );
        assert!(dropped.exe.ends_with("php.exe"));
    }

    #[test]
    fn discover_looks_one_level_into_a_nested_extraction() {
        let root = match user_bin_root() {
            Ok(root) => root.join("php"),
            Err(_) => return,
        };
        // Unzipping without "extract here" leaves the archive's own folder
        // in the middle — that shouldn't hide the install.
        let outer = root.join("php-9.9.8-test");
        let inner = outer.join("php-9.9.8-nts-Win32-vs17-x64");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("php.exe"), b"not a real binary").unwrap();

        let found = discover("php", "php.exe");
        let nested = found.iter().find(|runtime| runtime.version == "9.9.8");
        let exe = nested.map(|runtime| runtime.exe.clone());

        let _ = std::fs::remove_dir_all(&outer);

        assert!(nested.is_some(), "a nested extraction must still be found");
        assert!(exe.unwrap().starts_with(&inner));
    }

    /// The layout MariaDB's (and MySQL's) zip actually produces:
    /// `<version>/<archive-folder>/bin/mysqld.exe` — two levels down plus a
    /// `bin`. Looking only one level deep found no installed database
    /// server at all, which is the regression this guards.
    #[test]
    fn discover_finds_a_server_nested_under_a_bin_folder() {
        let root = match user_bin_root() {
            Ok(root) => root.join("mariadb"),
            Err(_) => return,
        };
        let bin = root.join("9.9.7").join("mariadb-9.9.7-winx64").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("mysqld.exe"), b"not a real binary").unwrap();

        let found = discover("mariadb", "mysqld.exe");
        let nested = found.iter().find(|runtime| runtime.version == "9.9.7");
        let exe = nested.map(|runtime| runtime.exe.clone());

        let _ = std::fs::remove_dir_all(root.join("9.9.7"));

        assert!(
            nested.is_some(),
            "a server under <version>/<archive>/bin must be discovered"
        );
        assert!(exe.unwrap().ends_with("mysqld.exe"));
    }

    /// A `bin` folder contributes no version of its own — the number has to
    /// come from a real folder name further up.
    #[test]
    fn a_bin_folder_never_becomes_the_version() {
        let root = match user_bin_root() {
            Ok(root) => root.join("mariadb"),
            Err(_) => return,
        };
        let bin = root.join("mariadb-9.9.6-winx64").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("mysqld.exe"), b"not a real binary").unwrap();

        let found = discover("mariadb", "mysqld.exe");
        let version = found
            .iter()
            .find(|runtime| runtime.exe.starts_with(&bin))
            .map(|runtime| runtime.version.clone());

        let _ = std::fs::remove_dir_all(root.join("mariadb-9.9.6-winx64"));

        assert_eq!(version.as_deref(), Some("9.9.6"));
    }

    #[test]
    fn manifest_entries_have_well_formed_checksums_and_urls() {
        for pkg in MANIFEST {
            assert_eq!(
                pkg.sha256.len(),
                64,
                "{} sha256 must be 64 hex chars",
                pkg.id
            );
            assert!(
                pkg.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} sha256 must be hex",
                pkg.id
            );
            assert!(pkg.download_url.starts_with("https://"), "{} url", pkg.id);
        }
    }

    #[test]
    fn find_returns_the_matching_package() {
        assert_eq!(find("nginx").unwrap().id, "nginx");
        assert!(find("does-not-exist").is_err());
    }

    #[test]
    fn list_status_reports_every_manifest_entry() {
        // `installed` reflects real local disk state, not something this
        // test controls, so only structure/ordering is asserted here.
        let statuses = list_status();
        assert_eq!(statuses.len(), MANIFEST.len());
        for (status, pkg) in statuses.iter().zip(MANIFEST) {
            assert_eq!(status.id, pkg.id);
            assert_eq!(status.version, pkg.version);
        }
    }

    #[test]
    fn exe_path_is_nested_under_the_package_family_and_version() {
        let pkg = find("php-8.3.33").unwrap();
        let path = exe_path(pkg).unwrap();
        let path_str = path.to_string_lossy();

        assert!(path_str.contains("php"));
        assert!(path_str.contains("8.3.33"));
        assert!(path_str.ends_with("php.exe"));
    }

    #[test]
    fn family_packages_returns_every_php_version_newest_first() {
        let versions = family_packages("php");
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].version, "8.3.33");
        assert!(versions.iter().all(|pkg| pkg.family == "php"));
    }
}
