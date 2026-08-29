//! On-demand portable binary downloader.
//!
//! Per `CLAUDE.md`, portable service binaries (Nginx, PHP, MariaDB) are never
//! committed to git — they're fetched from each project's official
//! distribution on first use, checksum-verified, and cached under the user's
//! local app data directory. Every entry in [`MANIFEST`] is a real, currently
//! published Windows portable/zip release.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::utils::error::AppError;

/// A downloadable portable binary package.
#[derive(Debug, Clone, Copy)]
pub struct BinaryPackage {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub download_url: &'static str,
    pub sha256: &'static str,
    /// Path to the main executable inside the extracted archive — doubles as
    /// the install-completion check and, later, the path `Service::start`
    /// will spawn.
    pub exe_relative_path: &'static str,
}

pub const MANIFEST: &[BinaryPackage] = &[
    BinaryPackage {
        id: "nginx",
        name: "Nginx",
        version: "1.25.3",
        download_url: "https://nginx.org/download/nginx-1.25.3.zip",
        sha256: "58df6e5865a922aaa477ac89b79c13739347a37ccc4b3de58de91f1487710cc4",
        exe_relative_path: "nginx-1.25.3/nginx.exe",
    },
    BinaryPackage {
        id: "php",
        name: "PHP",
        version: "8.3.33",
        download_url:
            "https://downloads.php.net/~windows/releases/php-8.3.33-nts-Win32-vs16-x64.zip",
        sha256: "534399107056313246f424adbbb7937337e40fbbf6aa7bc26287ba9cfd2e4a2a",
        exe_relative_path: "php.exe",
    },
    BinaryPackage {
        id: "mariadb",
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

/// `%LOCALAPPDATA%\Rezure\bin`, created lazily on first install.
fn install_root() -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base.join("Rezure").join("bin"))
}

fn package_dir(pkg: &BinaryPackage) -> Result<PathBuf, AppError> {
    Ok(install_root()?.join(pkg.id).join(pkg.version))
}

/// Where `pkg`'s main executable lives once installed, whether or not it
/// actually is yet.
pub fn exe_path(pkg: &BinaryPackage) -> Result<PathBuf, AppError> {
    Ok(package_dir(pkg)?.join(pkg.exe_relative_path))
}

pub fn is_installed(pkg: &BinaryPackage) -> bool {
    exe_path(pkg).map(|path| path.is_file()).unwrap_or(false)
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

/// Downloads, checksum-verifies, and extracts a portable binary package,
/// emitting [`PROGRESS_EVENT`] as it goes. Idempotent — if the executable is
/// already present this returns immediately without hitting the network.
pub async fn install(app: &AppHandle, id: &str) -> Result<BinaryStatus, AppError> {
    let pkg = find(id)?;

    if is_installed(pkg) {
        return Ok(status_of(pkg));
    }

    let dest_dir = package_dir(pkg)?;
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dest_dir.display())))?;

    let archive_bytes = download(app, pkg).await?;

    emit_progress(
        app,
        &InstallProgress {
            id: pkg.id.to_string(),
            stage: InstallStage::Verifying,
            downloaded_bytes: archive_bytes.len() as u64,
            total_bytes: Some(archive_bytes.len() as u64),
        },
    );
    verify_checksum(pkg, &archive_bytes)?;

    emit_progress(
        app,
        &InstallProgress {
            id: pkg.id.to_string(),
            stage: InstallStage::Extracting,
            downloaded_bytes: archive_bytes.len() as u64,
            total_bytes: Some(archive_bytes.len() as u64),
        },
    );

    let extract_dir = dest_dir.clone();
    tokio::task::spawn_blocking(move || extract(&archive_bytes, &extract_dir))
        .await
        .map_err(|e| AppError::Extract(format!("extraction task panicked: {e}")))??;

    if !is_installed(pkg) {
        return Err(AppError::Extract(format!(
            "expected {} after extracting {}, but it was not found",
            pkg.exe_relative_path, pkg.name
        )));
    }

    let status = status_of(pkg);
    emit_progress(
        app,
        &InstallProgress {
            id: pkg.id.to_string(),
            stage: InstallStage::Done,
            downloaded_bytes: 0,
            total_bytes: None,
        },
    );

    Ok(status)
}

async fn download(app: &AppHandle, pkg: &BinaryPackage) -> Result<Vec<u8>, AppError> {
    let response = reqwest::get(pkg.download_url)
        .await
        .map_err(|e| AppError::Download(format!("{}: {e}", pkg.download_url)))?;

    if !response.status().is_success() {
        return Err(AppError::Download(format!(
            "{} responded with {}",
            pkg.download_url,
            response.status()
        )));
    }

    let total_bytes = response.content_length();
    let mut downloaded = Vec::with_capacity(total_bytes.unwrap_or(0) as usize);
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Download(format!("{}: {e}", pkg.download_url)))?;
        downloaded.extend_from_slice(&chunk);
        emit_progress(
            app,
            &InstallProgress {
                id: pkg.id.to_string(),
                stage: InstallStage::Downloading,
                downloaded_bytes: downloaded.len() as u64,
                total_bytes,
            },
        );
    }

    Ok(downloaded)
}

fn verify_checksum(pkg: &BinaryPackage, bytes: &[u8]) -> Result<(), AppError> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = hex::encode(hasher.finalize());

    if actual != pkg.sha256 {
        return Err(AppError::ChecksumMismatch {
            id: pkg.id.to_string(),
            expected: pkg.sha256.to_string(),
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
    fn exe_path_is_nested_under_the_package_id_and_version() {
        let pkg = find("php").unwrap();
        let path = exe_path(pkg).unwrap();
        let path_str = path.to_string_lossy();

        assert!(path_str.contains("php"));
        assert!(path_str.contains("8.3.33"));
        assert!(path_str.ends_with("php.exe"));
    }
}
