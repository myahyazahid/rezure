//! Fetches the published changelog from `rezure-dashboard` and keeps a local
//! cache so the page still shows something when the API can't be reached —
//! see `GET /changelog` in `api-documentation/telemetry-api.md` (sibling
//! repo).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::api;
use crate::utils::error::AppError;
use crate::utils::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogEntry {
    pub version: String,
    pub title: String,
    pub body: String,
    pub released_at: String,
}

/// Mirrors the wire shape of `GET /api/v1/changelog` — `rezure-dashboard` is
/// a Laravel app and returns snake_case keys, unlike `ChangelogEntry`'s
/// camelCase (used for the cache file and the Tauri IPC boundary to the
/// Vue frontend). Deserializing straight into `ChangelogEntry` silently
/// failed here (missing `releasedAt`), which fell back to an empty cache.
#[derive(Debug, Deserialize)]
struct ApiChangelogEntry {
    version: String,
    title: String,
    body: String,
    released_at: String,
}

impl From<ApiChangelogEntry> for ChangelogEntry {
    fn from(entry: ApiChangelogEntry) -> Self {
        Self {
            version: entry.version,
            title: entry.title,
            body: entry.body,
            released_at: entry.released_at,
        }
    }
}

fn cache_path() -> Result<std::path::PathBuf, AppError> {
    Ok(paths::etc()?.join("changelog_cache.json"))
}

fn read_cache() -> Vec<ChangelogEntry> {
    let Ok(path) = cache_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn write_cache(entries: &[ChangelogEntry]) {
    let Ok(path) = cache_path() else { return };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(path, json);
    }
}

/// Live fetch from the API — 10s timeout, no auth, matching the doc's "small,
/// non-sensitive public read" framing.
async fn fetch_live() -> Result<Vec<ChangelogEntry>, AppError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Io(format!("could not set up the request: {e}")))?;
    let url = format!("{}/api/v1/changelog", api::base_url());
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Io(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Io(format!(
            "server returned {}",
            response.status()
        )));
    }
    let entries = response
        .json::<Vec<ApiChangelogEntry>>()
        .await
        .map_err(|e| AppError::Io(format!("unexpected response: {e}")))?;

    Ok(entries.into_iter().map(ChangelogEntry::from).collect())
}

/// Fetches the changelog, refreshing the local cache on success and falling
/// back to it on failure — infallible on purpose, since a stale/empty list
/// beats an error banner for a page this low-stakes.
pub async fn fetch() -> Vec<ChangelogEntry> {
    match fetch_live().await {
        Ok(entries) => {
            write_cache(&entries);
            entries
        }
        Err(err) => {
            log::warn!("could not fetch the changelog, using cache: {err}");
            read_cache()
        }
    }
}
