//! The Node.js versions Rezure can install, read from nodejs.org's own
//! release index at runtime — same reasoning as [`super::php_catalog`].
//!
//! This is catalog-and-install only: it makes `bin/node/<version>/node.exe`
//! appear on disk, checksum-verified, the same way installing any other
//! runtime does. It deliberately does **not** wire Node onto `PATH`, track
//! an "active" version, or do anything a running project would need to
//! actually use it — that's a separate, later feature (Node.js version
//! switching), and this is its foundation, not its finish.
//!
//! # Why only LTS lines are listed
//!
//! `nodejs.org/dist/index.json` lists every release back to 0.x — hundreds
//! of entries. Only the newest patch of each LTS line, plus the single
//! newest "Current" release, are offered — the same "one relevant entry,
//! not the whole history" choice `php_catalog` makes per branch.
//!
//! # Why the checksum is fetched lazily
//!
//! `index.json` carries no checksums at all; each version publishes its own
//! `SHASUMS256.txt`. Fetching that for every entry just to render the list
//! would mean one request per LTS line before the modal can even open, so
//! it's fetched once, at install time, for the one version being installed
//! — the same lazy-checksum shape [`super::composer_catalog`] uses for its
//! sidecar file.

use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

use super::binaries::{self, ArchiveInstall};
use crate::utils::error::AppError;

const INDEX_URL: &str = "https://nodejs.org/dist/index.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRelease {
    /// Full version including the `v` prefix, e.g. `v22.11.0` — nodejs.org's
    /// own folder naming, kept as-is rather than stripped so download URLs
    /// stay a straight substitution.
    pub version: String,
    /// The LTS codename (`"Krypton"`), or `null` for a Current release.
    pub lts: Option<String>,
    /// `YYYY-MM-DD`.
    pub released: String,
    pub latest: bool,
    pub installed: bool,
}

fn cache() -> &'static Mutex<Option<Vec<NodeRelease>>> {
    static CACHE: OnceLock<Mutex<Option<Vec<NodeRelease>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn has_windows_zip(entry: &Value) -> bool {
    entry
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| files.iter().any(|f| f.as_str() == Some("win-x64-zip")))
}

/// The LTS codename this entry ships under, if any. nodejs.org spells a
/// non-LTS ("Current") release's `lts` field as the boolean `false`, so it
/// has to be checked before assuming the field is always a string.
fn lts_name(entry: &Value) -> Option<String> {
    entry
        .get("lts")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Parses `nodejs.org/dist/index.json` into the newest patch of every LTS
/// line plus the newest Current release.
///
/// Kept separate from the fetch so it can be tested against a captured
/// response without touching the network — same split as `php_catalog::parse`.
fn parse(body: &str) -> Result<Vec<NodeRelease>, AppError> {
    let root: Value = serde_json::from_str(body)
        .map_err(|e| AppError::Download(format!("could not parse nodejs.org's index: {e}")))?;
    let entries = root
        .as_array()
        .ok_or_else(|| AppError::Download("nodejs.org's index isn't an array".to_string()))?;

    // Newest first, so "first entry per LTS line" and "first entry overall"
    // are both just "first seen" — nodejs.org's index is already sorted
    // this way, but sorting again here doesn't depend on that holding.
    let mut sorted: Vec<&Value> = entries.iter().filter(|e| has_windows_zip(e)).collect();
    sorted.sort_by(|a, b| {
        let va = a.get("version").and_then(Value::as_str).unwrap_or("");
        let vb = b.get("version").and_then(Value::as_str).unwrap_or("");
        binaries::compare_versions(vb.trim_start_matches('v'), va.trim_start_matches('v'))
    });

    let mut releases: Vec<NodeRelease> = Vec::new();
    let mut seen_lts: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_current = false;

    for entry in sorted {
        let Some(version) = entry.get("version").and_then(Value::as_str) else {
            continue;
        };
        let lts = lts_name(entry);

        match &lts {
            Some(name) => {
                if !seen_lts.insert(name.clone()) {
                    continue; // already have the newest patch of this line
                }
            }
            None => {
                if seen_current {
                    continue; // already have the newest Current release
                }
                seen_current = true;
            }
        }

        releases.push(NodeRelease {
            version: version.to_string(),
            lts,
            released: entry
                .get("date")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            latest: false,
            installed: false,
        });
    }

    if releases.is_empty() {
        return Err(AppError::Download(
            "nodejs.org's index listed no Windows x64 builds".to_string(),
        ));
    }

    releases.sort_by(|a, b| {
        binaries::compare_versions(
            b.version.trim_start_matches('v'),
            a.version.trim_start_matches('v'),
        )
    });
    releases[0].latest = true;
    Ok(releases)
}

fn mark_installed(releases: &mut [NodeRelease]) {
    let present = binaries::discover("node", "node.exe");
    for release in releases.iter_mut() {
        let version = release.version.trim_start_matches('v');
        release.installed = present.iter().any(|installed| installed.version == version);
    }
}

/// The installable Node.js versions, fetching nodejs.org's index on first
/// use. `refresh` forces a re-fetch; otherwise a previous successful
/// response is reused for the life of the process.
pub async fn list(refresh: bool) -> Result<Vec<NodeRelease>, AppError> {
    if !refresh {
        if let Some(cached) = cache().lock().unwrap().clone() {
            let mut cached = cached;
            mark_installed(&mut cached);
            return Ok(cached);
        }
    }

    let body = reqwest::get(INDEX_URL)
        .await
        .map_err(|e| AppError::Download(format!("{INDEX_URL}: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Download(format!("{INDEX_URL}: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Download(format!("{INDEX_URL}: {e}")))?;

    let releases = parse(&body)?;
    *cache().lock().unwrap() = Some(releases.clone());

    let mut releases = releases;
    mark_installed(&mut releases);
    Ok(releases)
}

async fn find(version: &str) -> Result<NodeRelease, AppError> {
    list(false)
        .await?
        .into_iter()
        .find(|release| release.version == version)
        .ok_or_else(|| AppError::CatalogVersionNotFound {
            runtime: "node".to_string(),
            version: version.to_string(),
        })
}

/// `node-v22.11.0-win-x64.zip`'s checksum, out of that version's own
/// `SHASUMS256.txt` — the `sha256sum`-tool format, one `<hex>  <filename>`
/// line per build.
async fn checksum_for(version: &str, file_name: &str) -> Result<String, AppError> {
    let url = format!("https://nodejs.org/dist/{version}/SHASUMS256.txt");
    let body = reqwest::get(&url)
        .await
        .map_err(|e| AppError::Download(format!("{url}: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Download(format!("{url}: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Download(format!("{url}: {e}")))?;

    body.lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            (name == file_name).then(|| hash.to_string())
        })
        .ok_or_else(|| AppError::Download(format!("{url} did not list {file_name}")))
}

/// Downloads, verifies and installs one Node.js version into
/// `bin/node/<version>/` (without the `v` prefix, matching every other
/// runtime's folder naming — `binaries::discover` reads the version back out
/// of the folder name either way).
pub async fn install(app: &AppHandle, version: &str) -> Result<(), AppError> {
    let bare_version = version.trim_start_matches('v');
    if binaries::discover("node", "node.exe")
        .iter()
        .any(|installed| installed.version == bare_version)
    {
        return Ok(());
    }

    // Confirms the version is really in the catalog before anything is
    // downloaded — the fields aren't needed beyond that, since the file name
    // and download URL both follow a fixed, predictable shape.
    find(version).await?;
    let file_name = format!("node-{version}-win-x64.zip");
    let sha256 = checksum_for(version, &file_name).await?;
    let download_url = format!("https://nodejs.org/dist/{version}/{file_name}");

    let dest_dir = binaries::install_root()?.join("node").join(bare_version);
    let exe_relative = format!("node-{version}-win-x64/node.exe");

    binaries::install_archive(
        app,
        &ArchiveInstall {
            id: &format!("node-{version}"),
            label: "Node.js",
            download_url: &download_url,
            sha256: &sha256,
            dest_dir,
            exe_relative_path: &exe_relative,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed copy of `nodejs.org/dist/index.json`'s real shape.
    const SAMPLE: &str = r#"[
      { "version": "v23.1.0", "date": "2024-10-18", "lts": false,
        "files": ["win-x64-zip", "linux-x64"] },
      { "version": "v22.11.0", "date": "2024-10-29", "lts": "Krypton",
        "files": ["win-x64-zip", "linux-x64"] },
      { "version": "v22.10.0", "date": "2024-10-16", "lts": false,
        "files": ["win-x64-zip"] },
      { "version": "v20.18.1", "date": "2024-11-20", "lts": "Iron",
        "files": ["win-x64-zip"] },
      { "version": "v20.18.0", "date": "2024-10-24", "lts": "Iron",
        "files": ["win-x64-zip"] },
      { "version": "v18.20.5", "date": "2024-11-12", "lts": "Hydrogen",
        "files": ["win-x64-zip"] },
      { "version": "v0.10.0", "date": "2013-03-11", "lts": false,
        "files": [] }
    ]"#;

    #[test]
    fn keeps_only_the_newest_patch_per_lts_line_plus_current() {
        let releases = parse(SAMPLE).unwrap();
        let versions: Vec<&str> = releases.iter().map(|r| r.version.as_str()).collect();
        // v22.10.0 (non-LTS patch of the 22 line) is superseded by v23.1.0
        // as "Current", and v22.11.0 is the 22 line's LTS entry — both are
        // kept because they're different things; v20.18.0 is dropped since
        // v20.18.1 is newer within the same LTS line.
        assert_eq!(versions, ["v23.1.0", "v22.11.0", "v20.18.1", "v18.20.5"]);
    }

    #[test]
    fn only_the_newest_release_overall_is_badged_latest() {
        let releases = parse(SAMPLE).unwrap();
        assert!(releases[0].latest);
        assert_eq!(releases.iter().filter(|r| r.latest).count(), 1);
    }

    #[test]
    fn lts_codenames_are_carried_through() {
        let releases = parse(SAMPLE).unwrap();
        let node22 = releases.iter().find(|r| r.version == "v22.11.0").unwrap();
        assert_eq!(node22.lts.as_deref(), Some("Krypton"));

        let current = releases.iter().find(|r| r.version == "v23.1.0").unwrap();
        assert_eq!(current.lts, None);
    }

    #[test]
    fn a_release_with_no_windows_zip_is_skipped() {
        let releases = parse(SAMPLE).unwrap();
        assert!(!releases.iter().any(|r| r.version == "v0.10.0"));
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse("not json").is_err());
        assert!(parse("{}").is_err());
        assert!(parse("[]").is_err());
    }

    #[test]
    fn a_shasums_line_resolves_to_just_the_hash() {
        let body = "aaaa000000000000000000000000000000000000000000000000000000000a  node-v22.11.0-win-x64.zip\nbbbb  node-v22.11.0-linux-x64.tar.gz\n";
        let found = body.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            (name == "node-v22.11.0-win-x64.zip").then(|| hash.to_string())
        });
        assert_eq!(
            found.as_deref(),
            Some("aaaa000000000000000000000000000000000000000000000000000000000a")
        );
    }

    /// Hits nodejs.org for real — run with:
    /// `cargo test --lib services::node_catalog::tests::fetches_the_real_index -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn fetches_the_real_index() {
        let releases = list(true).await.unwrap();
        assert!(!releases.is_empty());
        for release in &releases {
            let file_name = format!("node-{}-win-x64.zip", release.version);
            let sha256 = checksum_for(&release.version, &file_name).await.unwrap();
            println!(
                "{} lts={:?} installed={} latest={} sha256={sha256}",
                release.version, release.lts, release.installed, release.latest
            );
            assert_eq!(sha256.len(), 64);
        }
    }
}
