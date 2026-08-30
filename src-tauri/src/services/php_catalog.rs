//! The list of PHP versions Rezure can install, read from php.net's own
//! Windows release index at runtime.
//!
//! Not a hard-coded table: every entry needs a real SHA-256, and hand-maintaining
//! those means the list silently rots the moment php.net publishes a patch
//! release. `releases.json` is the same file the official download page is
//! built from, and it already carries the checksum, archive size and release
//! date for the current release of every supported branch — so the installer
//! stays current on its own *and* keeps the checksum verification that a
//! pinned manifest entry gets.
//!
//! What it deliberately does not do is list every historical patch release;
//! php.net only indexes the current one per branch. Older builds are what the
//! drop-in folder (`binaries::user_bin_root`) is for.

use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;

use super::binaries;
use crate::utils::error::AppError;

const RELEASES_URL: &str = "https://downloads.php.net/~windows/releases/releases.json";
const ARCHIVE_BASE: &str = "https://downloads.php.net/~windows/releases/";

/// Rezure runs `php-cgi.exe` behind nginx and never inside a threaded SAPI,
/// so the non-thread-safe x64 build is the right one — same choice the
/// pinned manifest entries already made.
fn is_wanted_build(key: &str) -> bool {
    key.starts_with("nts-") && key.ends_with("-x64")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpRelease {
    /// Full version, e.g. `8.4.25` — also the id used to install it and the
    /// folder it lands in.
    pub version: String,
    /// Release branch, e.g. `8.4`.
    pub branch: String,
    pub download_url: String,
    pub sha256: String,
    /// Archive size as php.net reports it, e.g. `33.46MB`.
    pub size: String,
    /// `YYYY-MM-DD`, from the archive's mtime.
    pub released: String,
    /// True for the newest branch only — what the UI badges as "LATEST".
    pub latest: bool,
    pub installed: bool,
}

fn cache() -> &'static Mutex<Option<Vec<PhpRelease>>> {
    static CACHE: OnceLock<Mutex<Option<Vec<PhpRelease>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Parses php.net's index into the releases Rezure can actually install.
///
/// Kept separate from the fetch so it can be tested against a real captured
/// response without touching the network.
fn parse(body: &str) -> Result<Vec<PhpRelease>, AppError> {
    let root: Value = serde_json::from_str(body)
        .map_err(|e| AppError::Download(format!("could not parse php.net's release index: {e}")))?;
    let branches = root
        .as_object()
        .ok_or_else(|| AppError::Download("php.net's release index isn't an object".to_string()))?;

    let mut releases: Vec<PhpRelease> = Vec::new();
    for (branch, entry) in branches {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some(version) = entry.get("version").and_then(Value::as_str) else {
            continue;
        };

        // The build key carries the compiler version (`nts-vs17-x64`,
        // `nts-vc15-x64`), which changes between branches — so it's matched
        // by shape rather than looked up by name.
        let Some((_, build)) = entry.iter().find(|(key, _)| is_wanted_build(key)) else {
            continue;
        };

        let Some(zip) = build.get("zip") else {
            continue;
        };
        let (Some(path), Some(sha256)) = (
            zip.get("path").and_then(Value::as_str),
            zip.get("sha256").and_then(Value::as_str),
        ) else {
            continue;
        };

        releases.push(PhpRelease {
            version: version.to_string(),
            branch: branch.to_string(),
            download_url: format!("{ARCHIVE_BASE}{path}"),
            sha256: sha256.to_string(),
            size: zip
                .get("size")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            // The mtime is an RFC 3339 timestamp; only the date is shown.
            released: build
                .get("mtime")
                .and_then(Value::as_str)
                .and_then(|stamp| stamp.split('T').next())
                .unwrap_or("")
                .to_string(),
            latest: false,
            installed: false,
        });
    }

    if releases.is_empty() {
        return Err(AppError::Download(
            "php.net's release index listed no usable Windows builds".to_string(),
        ));
    }

    releases.sort_by(|a, b| binaries::compare_versions(&b.version, &a.version));
    releases[0].latest = true;
    Ok(releases)
}

/// Marks which releases are already on disk, so the UI can show "Installed"
/// instead of offering the download again.
fn mark_installed(releases: &mut [PhpRelease]) {
    let present = binaries::discover("php", "php.exe");
    for release in releases.iter_mut() {
        release.installed = present
            .iter()
            .any(|installed| installed.version == release.version);
    }
}

/// The installable PHP versions, fetching php.net's index on first use.
///
/// `refresh` forces a re-fetch; otherwise a previous successful response is
/// reused for the life of the process — the index changes a few times a
/// month, and the install dialog gets opened far more often than that.
pub async fn list(refresh: bool) -> Result<Vec<PhpRelease>, AppError> {
    if !refresh {
        if let Some(cached) = cache().lock().unwrap().clone() {
            let mut cached = cached;
            mark_installed(&mut cached);
            return Ok(cached);
        }
    }

    let body = reqwest::get(RELEASES_URL)
        .await
        .map_err(|e| AppError::Download(format!("{RELEASES_URL}: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Download(format!("{RELEASES_URL}: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Download(format!("{RELEASES_URL}: {e}")))?;

    let releases = parse(&body)?;
    *cache().lock().unwrap() = Some(releases.clone());

    let mut releases = releases;
    mark_installed(&mut releases);
    Ok(releases)
}

/// Looks up one release by version — what `services::php` installs from.
pub async fn find(version: &str) -> Result<PhpRelease, AppError> {
    list(false)
        .await?
        .into_iter()
        .find(|release| release.version == version)
        .ok_or_else(|| AppError::PhpVersionNotFound(version.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed copy of a real `releases.json`, including the shape
    /// details that matter: differing compiler keys per branch, thread-safe
    /// builds alongside the non-thread-safe ones, and x86 next to x64.
    const SAMPLE: &str = r#"{
      "8.3": {
        "version": "8.3.33",
        "nts-vs16-x64": {
          "mtime": "2026-07-28T19:05:03+00:00",
          "zip": { "path": "php-8.3.33-nts-Win32-vs16-x64.zip", "size": "32.28MB",
                   "sha256": "534399107056313246f424adbbb7937337e40fbbf6aa7bc26287ba9cfd2e4a2a" }
        },
        "ts-vs16-x64": {
          "mtime": "2026-07-28T19:05:03+00:00",
          "zip": { "path": "php-8.3.33-Win32-vs16-x64.zip", "size": "32.5MB", "sha256": "aaaa" }
        },
        "nts-vs16-x86": {
          "mtime": "2026-07-28T19:05:03+00:00",
          "zip": { "path": "php-8.3.33-nts-Win32-vs16-x86.zip", "size": "29MB", "sha256": "bbbb" }
        }
      },
      "8.4": {
        "version": "8.4.25",
        "nts-vs17-x64": {
          "mtime": "2026-08-25T19:20:07+00:00",
          "zip": { "path": "php-8.4.25-nts-Win32-vs17-x64.zip", "size": "33.46MB",
                   "sha256": "43a8f67ed2e5223fafb21293c85976361808855405278cef2cf3037c3ae2529c" }
        }
      },
      "7.4": {
        "version": "7.4.33",
        "nts-vc15-x64": {
          "mtime": "2024-07-10T21:09:01+00:00",
          "zip": { "path": "php-7.4.33-nts-Win32-vc15-x64.zip", "size": "24.92MB",
                   "sha256": "14ae3250d4447c8ccfc4c45a70d90adfbcd61e728d85f0be56a7ddf8f9c8aace" }
        }
      }
    }"#;

    #[test]
    fn parses_one_release_per_branch_newest_first() {
        let releases = parse(SAMPLE).unwrap();
        let versions: Vec<&str> = releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["8.4.25", "8.3.33", "7.4.33"]);
    }

    #[test]
    fn only_the_newest_release_is_badged_latest() {
        let releases = parse(SAMPLE).unwrap();
        assert!(releases[0].latest);
        assert_eq!(releases.iter().filter(|r| r.latest).count(), 1);
    }

    /// The whole point of matching the build key by shape: `nts` and `x64`
    /// have to win over the thread-safe and 32-bit archives sitting next to
    /// them, whatever compiler version the branch was built with.
    #[test]
    fn picks_the_non_thread_safe_x64_build() {
        let releases = parse(SAMPLE).unwrap();
        let php83 = releases.iter().find(|r| r.branch == "8.3").unwrap();
        assert!(php83
            .download_url
            .ends_with("php-8.3.33-nts-Win32-vs16-x64.zip"));
        assert_eq!(
            php83.sha256,
            "534399107056313246f424adbbb7937337e40fbbf6aa7bc26287ba9cfd2e4a2a"
        );

        // A different branch, a different compiler key — same rule.
        let php74 = releases.iter().find(|r| r.branch == "7.4").unwrap();
        assert!(php74
            .download_url
            .ends_with("php-7.4.33-nts-Win32-vc15-x64.zip"));
    }

    #[test]
    fn carries_the_size_and_release_date_the_dialog_shows() {
        let releases = parse(SAMPLE).unwrap();
        let latest = &releases[0];
        assert_eq!(latest.size, "33.46MB");
        assert_eq!(latest.released, "2026-08-25");
    }

    #[test]
    fn download_urls_are_absolute_https() {
        for release in parse(SAMPLE).unwrap() {
            assert!(
                release.download_url.starts_with("https://"),
                "{}",
                release.download_url
            );
        }
    }

    #[test]
    fn a_branch_with_no_usable_build_is_skipped_not_fatal() {
        let body = r#"{
          "8.4": { "version": "8.4.25", "nts-vs17-x86": { "mtime": "2026-08-25T00:00:00+00:00",
                   "zip": { "path": "x86.zip", "size": "1MB", "sha256": "cc" } } },
          "8.3": { "version": "8.3.33", "nts-vs16-x64": { "mtime": "2026-07-28T00:00:00+00:00",
                   "zip": { "path": "ok.zip", "size": "1MB", "sha256": "dd" } } }
        }"#;
        let releases = parse(body).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, "8.3.33");
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse("not json at all").is_err());
        assert!(parse("{}").is_err());
    }

    /// Hits php.net for real — run with:
    /// `cargo test --lib services::php_catalog::tests::fetches_the_real_index -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn fetches_the_real_index() {
        let releases = list(true).await.unwrap();
        assert!(releases.len() >= 3, "expected several branches");
        for release in &releases {
            println!(
                "{} ({}) {} {} installed={} latest={}",
                release.version,
                release.branch,
                release.size,
                release.released,
                release.installed,
                release.latest
            );
            assert_eq!(release.sha256.len(), 64, "{} sha256", release.version);
        }
    }
}
