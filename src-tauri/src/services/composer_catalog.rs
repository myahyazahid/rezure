//! The Composer versions Rezure can install, read from Composer's own
//! version index at runtime — same reasoning as [`super::php_catalog`].
//!
//! Before this existed, `services::scaffold::ensure_composer` always fetched
//! whatever `composer.phar` was current at Composer's stable download URL,
//! with **no checksum verification at all** — there was no stable per-version
//! artifact to pin a hash against. This catalog is what makes picking (and
//! verifying) a specific version possible.
//!
//! # Where the checksum comes from
//!
//! `getcomposer.org/versions` is not documented as a stable public API. Two
//! plausible shapes exist in the wild: the JSON entry carrying its own
//! `sha256` field, or the checksum living in a sidecar file at
//! `<download url>.sha256sum`. [`checksum_for`] tries the first and falls
//! back to the second, so this works either way without needing to guess
//! which one is current before shipping.
//!
//! # `path` is root-relative, not a bare filename
//!
//! Confirmed against a live response (a user's failed install produced
//! `.../download/2.10.3//download/2.10.3/composer.phar.sha256sum` — the
//! doubled segment is what a bare-filename assumption looks like once the
//! real value turns out to already be `/download/2.10.3/composer.phar`).
//! [`resolve_download_url`] normalizes every shape the field is known to
//! take — root-relative, absolute, or a plain filename — so it can't
//! silently double up again if the shape changes.

use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;

use super::binaries;
use crate::utils::error::AppError;

const VERSIONS_URL: &str = "https://getcomposer.org/versions";
const SITE_ROOT: &str = "https://getcomposer.org";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerRelease {
    pub version: String,
    pub download_url: String,
    /// `None` until [`checksum_for`] resolves it — the catalog list doesn't
    /// need it, only [`super::composer::install`] does.
    #[serde(skip)]
    pub sha256: Option<String>,
    pub latest: bool,
    pub installed: bool,
}

fn cache() -> &'static Mutex<Option<Vec<ComposerRelease>>> {
    static CACHE: OnceLock<Mutex<Option<Vec<ComposerRelease>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Turns a `getcomposer.org/versions` entry's `path` field into a full
/// download URL, whatever shape that field takes: already absolute
/// (`https://…`), root-relative (`/download/2.10.3/composer.phar` — the
/// shape confirmed live, see the module doc), or a bare filename
/// (`composer.phar`, this module's original guess, kept as a fallback).
fn resolve_download_url(version: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else if let Some(rest) = path.strip_prefix('/') {
        format!("{SITE_ROOT}/{rest}")
    } else {
        format!("{SITE_ROOT}/download/{version}/{path}")
    }
}

/// Parses `getcomposer.org/versions` into the releases Rezure offers.
///
/// Only the `"stable"` array is used — `"preview"`/`"snapshot"` are
/// pre-release builds, and the legacy `"1"` branch is Composer 1.x, which
/// Rezure has no reason to offer alongside 2.x.
fn parse(body: &str) -> Result<Vec<ComposerRelease>, AppError> {
    let root: Value = serde_json::from_str(body).map_err(|e| {
        AppError::Download(format!("could not parse Composer's version index: {e}"))
    })?;

    let stable = root
        .get("stable")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AppError::Download("Composer's version index had no \"stable\" array".to_string())
        })?;

    let mut releases = Vec::new();
    for entry in stable {
        let Some(version) = entry.get("version").and_then(Value::as_str) else {
            continue;
        };
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("composer.phar");

        releases.push(ComposerRelease {
            version: version.to_string(),
            download_url: resolve_download_url(version, path),
            sha256: entry
                .get("sha256")
                .and_then(Value::as_str)
                .map(str::to_string),
            latest: false,
            installed: false,
        });
    }

    if releases.is_empty() {
        return Err(AppError::Download(
            "Composer's version index listed no stable releases".to_string(),
        ));
    }

    releases.sort_by(|a, b| binaries::compare_versions(&b.version, &a.version));
    releases[0].latest = true;
    Ok(releases)
}

fn mark_installed(releases: &mut [ComposerRelease]) {
    let present = binaries::discover("composer", "composer.phar");
    for release in releases.iter_mut() {
        release.installed = present
            .iter()
            .any(|installed| installed.version == release.version);
    }
}

/// The installable Composer versions, fetching the index on first use.
/// `refresh` forces a re-fetch; otherwise a previous successful response is
/// reused for the life of the process.
pub async fn list(refresh: bool) -> Result<Vec<ComposerRelease>, AppError> {
    if !refresh {
        if let Some(cached) = cache().lock().unwrap().clone() {
            let mut cached = cached;
            mark_installed(&mut cached);
            return Ok(cached);
        }
    }

    let body = reqwest::get(VERSIONS_URL)
        .await
        .map_err(|e| AppError::Download(format!("{VERSIONS_URL}: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Download(format!("{VERSIONS_URL}: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Download(format!("{VERSIONS_URL}: {e}")))?;

    let releases = parse(&body)?;
    *cache().lock().unwrap() = Some(releases.clone());

    let mut releases = releases;
    mark_installed(&mut releases);
    Ok(releases)
}

pub async fn find(version: &str) -> Result<ComposerRelease, AppError> {
    list(false)
        .await?
        .into_iter()
        .find(|release| release.version == version)
        .ok_or_else(|| AppError::CatalogVersionNotFound {
            runtime: "composer".to_string(),
            version: version.to_string(),
        })
}

/// The checksum to verify a release's download against — the entry's own
/// `sha256` if the index carried one, otherwise the sidecar file Composer
/// publishes next to every `.phar`. See the module doc for why both paths
/// exist.
pub async fn checksum_for(release: &ComposerRelease) -> Result<String, AppError> {
    if let Some(sha256) = &release.sha256 {
        return Ok(sha256.clone());
    }

    let sidecar_url = format!("{}.sha256sum", release.download_url);
    let body = reqwest::get(&sidecar_url)
        .await
        .map_err(|e| AppError::Download(format!("{sidecar_url}: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Download(format!("{sidecar_url}: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Download(format!("{sidecar_url}: {e}")))?;

    // The sidecar is just the hex digest, sometimes followed by whitespace
    // or a filename (the same `sha256sum`-tool format `SHASUMS256.txt` uses).
    let sha256 = body
        .split_whitespace()
        .next()
        .filter(|token| token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| {
            AppError::Download(format!(
                "{sidecar_url} did not contain a usable SHA-256 digest"
            ))
        })?;

    Ok(sha256.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mixed on purpose: `2.10.3`'s root-relative `path` is the shape
    /// confirmed against a live response (see the module doc); the
    /// `2.8.x`/`2.9.x`/`1.10.27` entries keep the bare-filename shape this
    /// module originally guessed, so both branches of
    /// [`resolve_download_url`] stay covered.
    const SAMPLE: &str = r#"{
      "stable": [
        { "path": "composer.phar", "version": "2.8.1", "sha256": "aaaa000000000000000000000000000000000000000000000000000000000a", "min-php": 70205 },
        { "path": "composer.phar", "version": "2.8.0", "min-php": 70205 },
        { "path": "/download/2.10.3/composer.phar", "version": "2.10.3", "min-php": 70205 }
      ],
      "preview": [
        { "path": "composer.phar", "version": "2.9.0-RC1", "min-php": 70205 }
      ],
      "1": [
        { "path": "1composer.phar", "version": "1.10.27", "min-php": 50302 }
      ]
    }"#;

    #[test]
    fn only_stable_releases_are_offered() {
        let releases = parse(SAMPLE).unwrap();
        let versions: Vec<&str> = releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["2.10.3", "2.8.1", "2.8.0"]);
    }

    #[test]
    fn a_root_relative_path_resolves_without_duplicating_the_version_segment() {
        let releases = parse(SAMPLE).unwrap();
        let newest = releases.iter().find(|r| r.version == "2.10.3").unwrap();
        assert_eq!(
            newest.download_url,
            "https://getcomposer.org/download/2.10.3/composer.phar"
        );
    }

    #[test]
    fn only_the_newest_release_is_badged_latest() {
        let releases = parse(SAMPLE).unwrap();
        assert!(releases[0].latest);
        assert_eq!(releases.iter().filter(|r| r.latest).count(), 1);
    }

    #[test]
    fn a_release_with_its_own_checksum_field_carries_it_through() {
        let releases = parse(SAMPLE).unwrap();
        let latest = releases.iter().find(|r| r.version == "2.8.1").unwrap();
        assert_eq!(
            latest.sha256.as_deref(),
            Some("aaaa000000000000000000000000000000000000000000000000000000000a")
        );
        assert_eq!(
            latest.download_url,
            "https://getcomposer.org/download/2.8.1/composer.phar"
        );
    }

    #[test]
    fn a_release_with_no_checksum_field_leaves_it_none_for_the_sidecar_fallback() {
        let releases = parse(SAMPLE).unwrap();
        let older = releases.iter().find(|r| r.version == "2.8.0").unwrap();
        assert_eq!(older.sha256, None);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse("not json").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn a_sha256sum_sidecar_line_is_parsed_to_just_the_digest() {
        // The `sha256sum` CLI format: digest, two spaces, filename.
        let line =
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  composer.phar\n";
        let token = line.split_whitespace().next().unwrap();
        assert_eq!(token.len(), 64);
    }

    /// Hits getcomposer.org for real — run with:
    /// `cargo test --lib services::composer_catalog::tests::fetches_the_real_index -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn fetches_the_real_index() {
        let releases = list(true).await.unwrap();
        assert!(!releases.is_empty());
        for release in releases.iter().take(3) {
            let sha256 = checksum_for(release).await.unwrap();
            println!(
                "{} installed={} latest={} sha256={sha256}",
                release.version, release.installed, release.latest
            );
            assert_eq!(sha256.len(), 64);
        }
    }
}
