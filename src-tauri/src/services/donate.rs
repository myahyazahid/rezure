//! Fetches donation/support links from `rezure-dashboard` and keeps a local
//! cache, same shape as `services::changelog` — a maintainer changes these
//! from the dashboard without shipping a new Rezure build, and a stale cache
//! beats an error banner on a page this low-stakes.
//!
//! Unlike `changelog`, this cache also remembers the `Last-Modified` header
//! from the last successful fetch and sends it back as `If-Modified-Since`.
//! The backend answers with a bodyless `304` when nothing changed (see
//! `Api\V1\DonateController` in `laravel-api`), so the Support Developer
//! page's normal case — maintainer hasn't touched the config since last
//! launch — is a cheap round trip that confirms the cache is still good,
//! not a full re-download. On any failure (offline, `304`, anything else
//! not a fresh `200`) the page renders straight from the cache, so it never
//! depends on this endpoint being reachable on every open.

use std::time::Duration;

use reqwest::header::{HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED};
use serde::{Deserialize, Serialize};

use crate::config::api;
use crate::utils::error::AppError;
use crate::utils::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonateLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoWallet {
    pub symbol: String,
    pub label: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonateConfig {
    pub message: String,
    pub local: Vec<DonateLink>,
    pub global: Vec<DonateLink>,
    pub crypto: Vec<CryptoWallet>,
}

impl Default for DonateConfig {
    fn default() -> Self {
        Self {
            message: "Rezure is free & open-source — support keeps it going.".to_string(),
            local: Vec::new(),
            global: Vec::new(),
            crypto: Vec::new(),
        }
    }
}

/// Mirrors `GET /api/v1/support/donate`'s snake_case wire shape — see
/// `services::changelog`'s `ApiChangelogEntry` for why this isn't deserialized
/// straight into `DonateConfig`.
#[derive(Debug, Deserialize)]
struct ApiDonateLink {
    label: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct ApiCryptoWallet {
    symbol: String,
    label: String,
    address: String,
}

#[derive(Debug, Deserialize)]
struct ApiDonateConfig {
    message: String,
    local: Vec<ApiDonateLink>,
    global: Vec<ApiDonateLink>,
    crypto: Vec<ApiCryptoWallet>,
}

impl From<ApiDonateConfig> for DonateConfig {
    fn from(config: ApiDonateConfig) -> Self {
        Self {
            message: config.message,
            local: config
                .local
                .into_iter()
                .map(|l| DonateLink {
                    label: l.label,
                    url: l.url,
                })
                .collect(),
            global: config
                .global
                .into_iter()
                .map(|l| DonateLink {
                    label: l.label,
                    url: l.url,
                })
                .collect(),
            crypto: config
                .crypto
                .into_iter()
                .map(|w| CryptoWallet {
                    symbol: w.symbol,
                    label: w.label,
                    address: w.address,
                })
                .collect(),
        }
    }
}

/// What's actually persisted to disk — the config plus the `Last-Modified`
/// it came with, so the next launch can ask the server "anything newer than
/// this?" instead of unconditionally re-downloading.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDonateConfig {
    last_modified: Option<String>,
    config: DonateConfig,
}

fn cache_path() -> Result<std::path::PathBuf, AppError> {
    Ok(paths::etc()?.join("donate_cache.json"))
}

fn read_cache() -> Option<CachedDonateConfig> {
    let path = cache_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(cached: &CachedDonateConfig) {
    let Ok(path) = cache_path() else { return };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(cached) {
        let _ = std::fs::write(path, json);
    }
}

enum FetchOutcome {
    /// Server confirmed the cached copy is still current — `304`.
    NotModified,
    Fresh(DonateConfig, Option<String>),
}

/// Live fetch from the API — 10s timeout, no auth, matching the changelog
/// endpoint's "small, non-sensitive public read" framing. Sends
/// `If-Modified-Since` when a previous fetch's `Last-Modified` is cached, so
/// the common "nothing changed" case comes back as an empty `304` rather
/// than the full body.
async fn fetch_live(cached_last_modified: Option<&str>) -> Result<FetchOutcome, AppError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Io(format!("could not set up the request: {e}")))?;
    let url = format!("{}/api/v1/support/donate", api::base_url());
    let mut request = client.get(url);
    if let Some(last_modified) = cached_last_modified {
        if let Ok(value) = HeaderValue::from_str(last_modified) {
            request = request.header(IF_MODIFIED_SINCE, value);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::Io(e.to_string()))?;

    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FetchOutcome::NotModified);
    }
    if !response.status().is_success() {
        return Err(AppError::Io(format!(
            "server returned {}",
            response.status()
        )));
    }

    let last_modified = response
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let config = response
        .json::<ApiDonateConfig>()
        .await
        .map_err(|e| AppError::Io(format!("unexpected response: {e}")))?;

    Ok(FetchOutcome::Fresh(DonateConfig::from(config), last_modified))
}

/// Fetches the donation config, refreshing the local cache on a fresh `200`
/// and falling back to it on `304` or any failure. Returns
/// `DonateConfig::default()` — an empty, harmless config — only when
/// there's neither a live response nor a cache yet, so the page renders its
/// "nothing configured" state instead of an error.
pub async fn fetch() -> DonateConfig {
    let cached = read_cache();
    let cached_last_modified = cached.as_ref().and_then(|c| c.last_modified.as_deref());

    match fetch_live(cached_last_modified).await {
        Ok(FetchOutcome::NotModified) => cached.map(|c| c.config).unwrap_or_default(),
        Ok(FetchOutcome::Fresh(config, last_modified)) => {
            write_cache(&CachedDonateConfig {
                last_modified,
                config: config.clone(),
            });
            config
        }
        Err(err) => {
            log::warn!("could not fetch donation links, using cache: {err}");
            cached.map(|c| c.config).unwrap_or_default()
        }
    }
}
