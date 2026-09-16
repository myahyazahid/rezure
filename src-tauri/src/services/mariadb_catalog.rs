//! The MariaDB versions Rezure can install, read from MariaDB's own REST API
//! at runtime — the same "don't hand-maintain what a real index already
//! publishes" reasoning as [`super::php_catalog`].
//!
//! # Why branches are still a hard-coded list
//!
//! Unlike php.net's `releases.json`, which is one document covering every
//! branch, MariaDB's API is queried **per branch**
//! (`downloads.mariadb.org/rest-api/mariadb/<branch>/`) — there's no single
//! "everything" endpoint worth depending on for this. [`BRANCHES`] is the
//! list of branches Rezure asks about, the same trade-off `php_ext.rs`'s
//! PECL table already made: a branch MariaDB retires from this list simply
//! stops being offered, which is a wrong-but-honest answer rather than a
//! crash.
//!
//! Each release's checksum comes from MariaDB's server at request time, so
//! there is nothing pinned or hand-verified here to go stale — unlike the
//! PECL table, this list needs a bump only when a *new branch* becomes worth
//! offering, not on every patch release.
//!
//! MariaDB additionally publishes `sha256sum` inline per file, which is what
//! makes this catalog possible at all — see the open question about Nginx
//! in `docs/v3/rezure-app-v3-phases-tasks.md` for what a runtime *without*
//! that looks like.

use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

use super::binaries::{self, ArchiveInstall};
use crate::utils::error::AppError;
use crate::utils::paths;

/// Branches Rezure offers. Update this when MariaDB ships a new stable
/// major/minor line worth installing — the REST API itself doesn't expose
/// "list every branch that exists" in a way worth depending on here.
const BRANCHES: &[&str] = &["10.6", "10.11", "11.4", "11.8"];

fn branch_url(branch: &str) -> String {
    format!("https://downloads.mariadb.org/rest-api/mariadb/{branch}/")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MariaDbRelease {
    /// Full version, e.g. `11.4.5` — also the id used to install it.
    pub version: String,
    pub branch: String,
    pub download_url: String,
    pub sha256: String,
    /// `YYYY-MM-DD`, as MariaDB reports the release date.
    pub released: String,
    pub latest: bool,
    pub installed: bool,
}

fn cache() -> &'static Mutex<Option<Vec<MariaDbRelease>>> {
    static CACHE: OnceLock<Mutex<Option<Vec<MariaDbRelease>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Whether a file entry is the plain Windows ZIP — not `-debugsymbols`, not
/// an MSI, not a non-Windows build. MariaDB ships several packagings per
/// release, and only this one is a portable archive Rezure can extract.
fn is_wanted_file(file: &Value) -> bool {
    let os = file.get("os").and_then(Value::as_str).unwrap_or("");
    let package_type = file
        .get("package_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let file_name = file.get("file_name").and_then(Value::as_str).unwrap_or("");

    os.eq_ignore_ascii_case("Windows")
        && package_type.eq_ignore_ascii_case("ZIP file")
        && !file_name.to_lowercase().contains("debugsymbols")
}

/// Parses one branch's `GET .../mariadb/<branch>/` response into releases.
///
/// Kept separate from the fetch so it can be tested against a captured
/// response without touching the network — same split as `php_catalog::parse`.
fn parse_branch(branch: &str, body: &str) -> Result<Vec<MariaDbRelease>, AppError> {
    let root: Value = serde_json::from_str(body).map_err(|e| {
        AppError::Download(format!(
            "could not parse MariaDB's release index for {branch}: {e}"
        ))
    })?;

    let releases = root
        .get("releases")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::Download(format!(
                "MariaDB's release index for {branch} had no \"releases\" object"
            ))
        })?;

    let mut found = Vec::new();
    for (version, entry) in releases {
        let Some(files) = entry.get("files").and_then(Value::as_array) else {
            continue;
        };
        let Some(file) = files.iter().find(|f| is_wanted_file(f)) else {
            continue;
        };
        let Some(download_url) = file.get("file_download_url").and_then(Value::as_str) else {
            continue;
        };
        let Some(sha256) = file
            .get("checksum")
            .and_then(|c| c.get("sha256sum"))
            .and_then(Value::as_str)
        else {
            continue;
        };

        found.push(MariaDbRelease {
            version: version.clone(),
            branch: branch.to_string(),
            download_url: download_url.to_string(),
            sha256: sha256.to_string(),
            released: entry
                .get("release_date")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            latest: false,
            installed: false,
        });
    }

    Ok(found)
}

fn mark_installed(releases: &mut [MariaDbRelease]) {
    let present = binaries::discover("mariadb", "mysqld.exe");
    for release in releases.iter_mut() {
        release.installed = present
            .iter()
            .any(|installed| installed.version == release.version);
    }
}

/// Every installable version across [`BRANCHES`], newest first. A branch
/// whose request fails (network, or MariaDB retiring the branch) is skipped
/// rather than failing the whole list — the same "a partial answer beats no
/// answer" choice `php_catalog::parse` makes for one bad entry.
async fn fetch_all() -> Result<Vec<MariaDbRelease>, AppError> {
    let mut releases = Vec::new();

    for branch in BRANCHES {
        let url = branch_url(branch);
        let body = match reqwest::get(&url).await {
            Ok(resp) => match resp.error_for_status() {
                Ok(resp) => resp.text().await.ok(),
                Err(_) => None,
            },
            Err(_) => None,
        };
        let Some(body) = body else {
            log::warn!("could not reach MariaDB's release index for branch {branch}");
            continue;
        };

        match parse_branch(branch, &body) {
            Ok(parsed) => releases.extend(parsed),
            Err(err) => log::warn!("skipping MariaDB branch {branch}: {err}"),
        }
    }

    if releases.is_empty() {
        return Err(AppError::Download(
            "could not reach MariaDB's release index for any known branch".to_string(),
        ));
    }

    releases.sort_by(|a, b| binaries::compare_versions(&b.version, &a.version));
    releases[0].latest = true;
    Ok(releases)
}

/// The installable MariaDB versions, fetching MariaDB's index on first use.
/// `refresh` forces a re-fetch; otherwise a previous successful response is
/// reused for the life of the process.
pub async fn list(refresh: bool) -> Result<Vec<MariaDbRelease>, AppError> {
    if !refresh {
        if let Some(cached) = cache().lock().unwrap().clone() {
            let mut cached = cached;
            mark_installed(&mut cached);
            return Ok(cached);
        }
    }

    let releases = fetch_all().await?;
    *cache().lock().unwrap() = Some(releases.clone());

    let mut releases = releases;
    mark_installed(&mut releases);
    Ok(releases)
}

async fn find(version: &str) -> Result<MariaDbRelease, AppError> {
    list(false)
        .await?
        .into_iter()
        .find(|release| release.version == version)
        .ok_or_else(|| AppError::CatalogVersionNotFound {
            runtime: "mariadb".to_string(),
            version: version.to_string(),
        })
}

/// Downloads, verifies and installs one MariaDB version into
/// `bin/mariadb/<version>/`, where `db_profiles::installed_binaries`
/// (itself `binaries::discover("mariadb", "mysqld.exe")`) already looks —
/// no separate registration step, same as every other runtime here.
///
/// Idempotent: a version already on disk returns without touching the
/// network.
pub async fn install(app: &AppHandle, version: &str) -> Result<(), AppError> {
    if binaries::discover("mariadb", "mysqld.exe")
        .iter()
        .any(|installed| installed.version == version)
    {
        return Ok(());
    }

    let release = find(version).await?;
    let dest_dir = paths::bin()?.join("mariadb").join(&release.version);

    // MariaDB's zip nests everything under its own folder
    // (`mariadb-11.4.5-winx64/bin/mysqld.exe`) — `install_archive` only
    // checks that this one path exists after extracting, and `discover`'s
    // own nested-`bin`-folder search (see `binaries.rs`) is what finds it
    // afterwards regardless of the exact folder name inside the zip.
    let exe_relative = format!("mariadb-{}-winx64/bin/mysqld.exe", release.version);

    binaries::install_archive(
        app,
        &ArchiveInstall {
            id: &format!("mariadb-{}", release.version),
            label: "MariaDB",
            download_url: &release.download_url,
            sha256: &release.sha256,
            dest_dir,
            exe_relative_path: &exe_relative,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape reconstructed from MariaDB's public REST API docs
    /// (`downloads.mariadb.org/rest-api/mariadb/<branch>/`) — **not**
    /// verified against a live response, since this environment has no
    /// network access. `install`'s real-world behavior should be confirmed
    /// against the actual API before this ships.
    const SAMPLE: &str = r#"{
      "releases": {
        "11.4.5": {
          "release_id": "11.4.5",
          "release_status": "Stable",
          "release_date": "2025-02-11",
          "files": [
            {
              "os": "Windows",
              "package_type": "ZIP file",
              "file_name": "mariadb-11.4.5-winx64.zip",
              "file_download_url": "https://archive.mariadb.org/mariadb-11.4.5/winx64-packages/mariadb-11.4.5-winx64.zip",
              "checksum": { "sha256sum": "aaaa000000000000000000000000000000000000000000000000000000000a" }
            },
            {
              "os": "Windows",
              "package_type": "ZIP file",
              "file_name": "mariadb-11.4.5-winx64-debugsymbols.zip",
              "file_download_url": "https://archive.mariadb.org/mariadb-11.4.5/winx64-packages/mariadb-11.4.5-winx64-debugsymbols.zip",
              "checksum": { "sha256sum": "bbbb" }
            },
            {
              "os": "Linux",
              "package_type": "Tar file",
              "file_name": "mariadb-11.4.5-linux.tar.gz",
              "file_download_url": "https://archive.mariadb.org/mariadb-11.4.5/linux.tar.gz",
              "checksum": { "sha256sum": "cccc" }
            }
          ]
        },
        "11.4.4": {
          "release_id": "11.4.4",
          "release_status": "Stable",
          "release_date": "2024-11-11",
          "files": [
            {
              "os": "Windows",
              "package_type": "ZIP file",
              "file_name": "mariadb-11.4.4-winx64.zip",
              "file_download_url": "https://archive.mariadb.org/mariadb-11.4.4/winx64-packages/mariadb-11.4.4-winx64.zip",
              "checksum": { "sha256sum": "dddd000000000000000000000000000000000000000000000000000000000d" }
            }
          ]
        }
      }
    }"#;

    #[test]
    fn parses_the_windows_zip_and_skips_debugsymbols_and_other_os() {
        let releases = parse_branch("11.4", SAMPLE).unwrap();
        assert_eq!(releases.len(), 2);

        let newest = releases.iter().find(|r| r.version == "11.4.5").unwrap();
        assert!(newest.download_url.ends_with("mariadb-11.4.5-winx64.zip"));
        assert!(!newest.download_url.contains("debugsymbols"));
        assert_eq!(
            newest.sha256,
            "aaaa000000000000000000000000000000000000000000000000000000000a"
        );
        assert_eq!(newest.released, "2025-02-11");
    }

    #[test]
    fn a_release_with_no_windows_zip_is_skipped() {
        let body = r#"{
          "releases": {
            "11.4.5": {
              "release_date": "2025-02-11",
              "files": [
                { "os": "Linux", "package_type": "Tar file", "file_name": "x.tar.gz",
                  "file_download_url": "https://x", "checksum": { "sha256sum": "aa" } }
              ]
            }
          }
        }"#;
        assert_eq!(parse_branch("11.4", body).unwrap().len(), 0);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse_branch("11.4", "not json").is_err());
        assert!(parse_branch("11.4", "{}").is_err());
    }

    #[test]
    fn branches_list_is_well_formed() {
        assert!(!BRANCHES.is_empty());
        for branch in BRANCHES {
            assert!(branch.contains('.'), "{branch} should look like a branch");
        }
    }

    /// Hits MariaDB's real API — run with:
    /// `cargo test --lib services::mariadb_catalog::tests::fetches_the_real_index -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn fetches_the_real_index() {
        let releases = list(true).await.unwrap();
        assert!(!releases.is_empty());
        for release in &releases {
            println!(
                "{} ({}) {} installed={} latest={}",
                release.version,
                release.branch,
                release.released,
                release.installed,
                release.latest
            );
            assert_eq!(release.sha256.len(), 64, "{} sha256", release.version);
        }
    }
}
