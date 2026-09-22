//! The CA bundle PHP verifies outbound HTTPS against.
//!
//! The Windows PHP zip ships no CA store, and neither `curl.cainfo` nor
//! `openssl.cafile` has a usable built-in default there, so without a bundle
//! every HTTPS call out of PHP — Laravel's Http client, Guzzle, a webhook to
//! an API — fails with "cURL error 60: unable to get local issuer
//! certificate". [`super::php_ini`] points both directives at the file this
//! module keeps in `etc/`; this module is what makes sure there *is* one.
//!
//! It gets there two ways:
//!
//! - [`seed_bundled`] copies the one shipped inside the installer, staged by
//!   `scripts/stage-bundled-binaries.ps1` from a dated, SHA-256-pinned
//!   curl.se URL. Works offline, on first launch, before PHP ever starts.
//! - [`update`] fetches curl.se's current bundle, verified against the
//!   `.sha256` sidecar published beside it — the same trust model as
//!   Composer's sidecar in `services::composer_catalog`. Mozilla's root list
//!   changes a few times a year, and a pinned copy only moves with releases.
//!
//! It lives in `etc/` rather than beside a PHP build because it isn't a
//! property of any one version: a switch must not lose it, and every
//! version's ini names the same file.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use super::{php, php_ini};
use crate::utils::error::AppError;
use crate::utils::paths;

/// The bundle's name inside [`paths::etc`] — and inside the installer's
/// `bundled-bin/ca/`.
pub const FILE_NAME: &str = "cacert.pem";

/// Always the newest bundle curl.se has extracted from Mozilla.
const LATEST_URL: &str = "https://curl.se/ca/cacert.pem";

/// `sha256sum`-format digest of [`LATEST_URL`], updated alongside it.
const LATEST_SHA256_URL: &str = "https://curl.se/ca/cacert.pem.sha256";

/// Every curl.se bundle opens with this line, naming the date of the
/// Mozilla data it was extracted from — the only version marker it has.
const DATE_MARKER: &str = "Certificate data from Mozilla as of:";

/// What the Switch page shows about the bundle.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaBundleStatus {
    pub installed: bool,
    pub path: String,
    /// `YYYY-MM-DD` of the Mozilla data inside, when the file says. `None`
    /// for a bundle that isn't from curl.se (a user's own, say).
    pub mozilla_date: Option<String>,
}

/// Where the bundle lives, whether or not it's there yet.
pub fn path() -> Result<PathBuf, AppError> {
    Ok(paths::etc()?.join(FILE_NAME))
}

/// The bundle's path, only when it is actually on disk.
pub fn installed() -> Option<PathBuf> {
    let path = path().ok()?;
    path.is_file().then_some(path)
}

pub fn status() -> Result<CaBundleStatus, AppError> {
    let path = path()?;
    let content = fs::read_to_string(&path).ok();
    Ok(CaBundleStatus {
        installed: content.is_some(),
        path: path.display().to_string(),
        mozilla_date: content
            .as_deref()
            .and_then(mozilla_date)
            .map(|date| format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)),
    })
}

/// The date a curl.se bundle's Mozilla data is from, ordered oldest first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MozillaDate {
    year: u32,
    month: u32,
    day: u32,
    /// `HH:MM:SS` — zero-padded, so it orders correctly as text.
    time: String,
}

/// Reads the date out of the marker line, e.g.
/// `## Certificate data from Mozilla as of: Thu Aug 13 03:12:01 2026 GMT`.
fn mozilla_date(content: &str) -> Option<MozillaDate> {
    let line = content.lines().take(20).find(|l| l.contains(DATE_MARKER))?;
    let rest = line.split_once(DATE_MARKER)?.1;
    // Weekday, month, day, time, year — the weekday carries nothing.
    let mut parts = rest.split_whitespace().skip(1);
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let day = parts.next()?.parse().ok()?;
    let time = parts.next()?.to_string();
    let year = parts.next()?.parse().ok()?;
    Some(MozillaDate {
        year,
        month,
        day,
        time,
    })
}

/// Enough of a sanity check to refuse an HTML error page or an empty body
/// that happened to download cleanly.
fn looks_like_bundle(content: &str) -> bool {
    content.contains("-----BEGIN CERTIFICATE-----")
}

/// Writes `content` to `dest` via a temporary file, so a PHP process reading
/// the bundle mid-update never sees half of one.
fn write_atomically(dest: &Path, content: &str) -> Result<(), AppError> {
    let tmp = dest.with_extension("pem.tmp");
    fs::write(&tmp, content)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", tmp.display())))?;
    fs::rename(&tmp, dest).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        AppError::Io(format!("could not replace {}: {e}", dest.display()))
    })
}

/// Copies the installer's bundle into `etc/` — when there is none yet, or
/// when the installer's is newer than the one on disk (a Rezure update
/// carrying fresher Mozilla data).
///
/// A file on disk with no curl.se date marker is left alone: that is not a
/// bundle Rezure put there, and replacing someone's own on an update would
/// be a surprise nobody could trace. Best-effort, like the binary seeding
/// beside it — PHP still runs without a bundle, just not over HTTPS.
pub fn seed_bundled(app: &AppHandle) {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return;
    };
    let source = resource_dir.join("bundled-bin").join("ca").join(FILE_NAME);
    if !source.is_file() {
        return;
    }
    let Ok(dest) = path() else {
        return;
    };
    match seed_from(&source, &dest) {
        Ok(true) => log::info!("seeded the CA bundle into {}", dest.display()),
        Ok(false) => {}
        Err(err) => log::warn!("could not seed the CA bundle: {err}"),
    }
}

/// [`seed_bundled`] minus the `AppHandle`, so the replace-or-keep rule is
/// testable. Reports whether `dest` was written.
fn seed_from(source: &Path, dest: &Path) -> Result<bool, AppError> {
    let bundled = fs::read_to_string(source)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", source.display())))?;
    if !looks_like_bundle(&bundled) {
        return Err(AppError::Io(format!(
            "{} is not a PEM bundle",
            source.display()
        )));
    }

    if let Ok(existing) = fs::read_to_string(dest) {
        let newer = match (mozilla_date(&existing), mozilla_date(&bundled)) {
            (Some(on_disk), Some(shipped)) => shipped > on_disk,
            // Not ours, or not dated: keep it.
            _ => false,
        };
        if !newer {
            return Ok(false);
        }
    }

    write_atomically(dest, &bundled)?;
    Ok(true)
}

/// Pulls the first hex digest out of a `sha256sum`-format sidecar.
fn parse_digest(sidecar: &str) -> Option<String> {
    let digest = sidecar.split_whitespace().next()?.to_ascii_lowercase();
    (digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit())).then_some(digest)
}

async fn fetch(url: &str) -> Result<reqwest::Response, AppError> {
    reqwest::get(url)
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| AppError::Download(format!("{url}: {e}")))
}

/// Replaces the bundle with curl.se's current one, then points every
/// installed version's own `php.ini` at it (see
/// [`php_ini::repair_ca_directives`]) so `php` in a terminal verifies too.
///
/// Web requests pick up new *contents* on their own — libcurl and OpenSSL
/// read the file per connection — but a bundle that didn't exist when PHP
/// started only reaches the web once PHP restarts, since the generated ini
/// only names it when it's there.
pub async fn update() -> Result<CaBundleStatus, AppError> {
    let body = fetch(LATEST_URL)
        .await?
        .bytes()
        .await
        .map_err(|e| AppError::Download(format!("{LATEST_URL}: {e}")))?;
    let sidecar = fetch(LATEST_SHA256_URL)
        .await?
        .text()
        .await
        .map_err(|e| AppError::Download(format!("{LATEST_SHA256_URL}: {e}")))?;

    let expected = parse_digest(&sidecar).ok_or_else(|| {
        AppError::Download(format!(
            "{LATEST_SHA256_URL} did not contain a usable SHA-256 digest"
        ))
    })?;
    let actual = hex::encode(Sha256::digest(&body));
    if actual != expected {
        return Err(AppError::ChecksumMismatch {
            id: FILE_NAME.to_string(),
            expected,
            actual,
        });
    }

    let content = String::from_utf8(body.to_vec())
        .map_err(|_| AppError::Download(format!("{LATEST_URL} is not text")))?;
    if !looks_like_bundle(&content) {
        return Err(AppError::Download(format!(
            "{LATEST_URL} did not contain any certificates"
        )));
    }

    write_atomically(&path()?, &content)?;
    log::info!("updated the CA bundle from {LATEST_URL}");

    for runtime in php::installed() {
        if let Err(err) = php_ini::repair_ca_directives(&runtime.dir) {
            log::warn!(
                "could not point {} at the CA bundle: {err}",
                runtime.dir.display()
            );
        }
    }

    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(date_line: &str) -> String {
        format!(
            "##\n## Bundle of CA Root Certificates\n##\n## {date_line}\n##\n\n\
             -----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n"
        )
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rezure-test-ca-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_mozilla_date_is_read_from_the_marker_line() {
        let content = bundle("Certificate data from Mozilla as of: Thu Aug 13 03:12:01 2026 GMT");
        assert_eq!(
            mozilla_date(&content),
            Some(MozillaDate {
                year: 2026,
                month: 8,
                day: 13,
                time: "03:12:01".to_string()
            })
        );
    }

    #[test]
    fn a_bundle_without_the_marker_has_no_date() {
        assert_eq!(mozilla_date(&bundle("my corporate roots")), None);
    }

    #[test]
    fn dates_order_across_years_months_and_days() {
        let older = mozilla_date(&bundle(
            "Certificate data from Mozilla as of: Tue Dec 30 03:12:01 2025 GMT",
        ));
        let newer = mozilla_date(&bundle(
            "Certificate data from Mozilla as of: Mon Jan 5 03:12:01 2026 GMT",
        ));
        assert!(newer > older);
    }

    #[test]
    fn only_a_sha256_digest_is_accepted_from_the_sidecar() {
        let digest = "f66dff1bdf8f96060b8177976f8b7d9254bc89bc4db933d769f7384d28480bc9";
        assert_eq!(
            parse_digest(&format!("{digest}  cacert.pem\n")).as_deref(),
            Some(digest)
        );
        assert_eq!(parse_digest("<html>not found</html>"), None);
        assert_eq!(parse_digest(""), None);
    }

    #[test]
    fn seeding_fills_an_empty_slot() {
        let dir = temp_dir("seed-empty");
        let source = dir.join("shipped.pem");
        let dest = dir.join(FILE_NAME);
        fs::write(
            &source,
            bundle("Certificate data from Mozilla as of: Thu Aug 13 03:12:01 2026 GMT"),
        )
        .unwrap();

        assert!(seed_from(&source, &dest).unwrap());
        assert!(dest.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn seeding_replaces_only_an_older_curl_bundle() {
        let dir = temp_dir("seed-older");
        let source = dir.join("shipped.pem");
        let dest = dir.join(FILE_NAME);
        let shipped = bundle("Certificate data from Mozilla as of: Thu Aug 13 03:12:01 2026 GMT");
        fs::write(&source, &shipped).unwrap();

        let newer_on_disk =
            bundle("Certificate data from Mozilla as of: Tue Nov 3 03:12:01 2026 GMT");
        fs::write(&dest, &newer_on_disk).unwrap();
        assert!(!seed_from(&source, &dest).unwrap(), "a newer bundle stays");
        assert_eq!(fs::read_to_string(&dest).unwrap(), newer_on_disk);

        fs::write(
            &dest,
            bundle("Certificate data from Mozilla as of: Tue Mar 11 03:12:01 2025 GMT"),
        )
        .unwrap();
        assert!(
            seed_from(&source, &dest).unwrap(),
            "an older bundle is replaced"
        );
        assert_eq!(fs::read_to_string(&dest).unwrap(), shipped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn seeding_never_touches_a_bundle_rezure_did_not_put_there() {
        let dir = temp_dir("seed-custom");
        let source = dir.join("shipped.pem");
        let dest = dir.join(FILE_NAME);
        fs::write(
            &source,
            bundle("Certificate data from Mozilla as of: Thu Aug 13 03:12:01 2026 GMT"),
        )
        .unwrap();
        let custom = bundle("my corporate roots");
        fs::write(&dest, &custom).unwrap();

        assert!(!seed_from(&source, &dest).unwrap());
        assert_eq!(fs::read_to_string(&dest).unwrap(), custom);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shipped_file_that_is_not_a_bundle_is_refused() {
        let dir = temp_dir("seed-bogus");
        let source = dir.join("shipped.pem");
        fs::write(&source, "<html>502</html>").unwrap();

        assert!(seed_from(&source, &dir.join(FILE_NAME)).is_err());
        assert!(!dir.join(FILE_NAME).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
