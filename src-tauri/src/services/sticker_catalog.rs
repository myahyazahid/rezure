//! The sticker catalog behind **Decorations → Browse**: what `rezure-dashboard`
//! offers, fetched from `GET /api/v1/stickers` (contract in
//! `api-documentation/telemetry-api.md`) and cached locally.
//!
//! Same shape as `services::donate`, for the same reason: Browse is a
//! low-stakes page, so a stale list beats an error banner. The last response
//! is kept with its `ETag` and sent back as `If-None-Match`; a catalog that
//! hasn't changed comes back as an empty `304`, and any failure — offline, a
//! `5xx`, a body that doesn't parse — falls back to what was cached, flagged
//! as `offline` so the page can say so.
//!
//! # Nothing in a response is trusted further than it has to be
//!
//! Each catalog entry is validated one at a time and a bad one is dropped
//! rather than failing the page: its id must be a plain slug (it becomes a file
//! name), its checksum 64 hex characters, its size within the cap. The `url` the
//! server sends is **ignored** — the file address is built from the id, so a
//! catalog can't point a download at some other host.

use std::time::Duration;

use reqwest::header::{HeaderValue, ETAG, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};

use crate::config::api;
use crate::config::stickers::is_valid_sticker_id;
use crate::utils::error::AppError;
use crate::utils::paths;

/// Largest sticker the catalog may offer, in bytes. Mirrors
/// `Sticker::MAX_BYTES` in `laravel-api`: the server won't accept a bigger
/// upload, and the client won't download one.
pub const MAX_STICKER_BYTES: u64 = 256 * 1024;

const MAX_NAME_CHARS: usize = 100;
const MAX_CATEGORY_LEN: usize = 40;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// What a sticker file is. The only three formats the catalog serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Svg,
    Png,
    Webp,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
        }
    }

    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "svg" => Some(Self::Svg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// One sticker the catalog offers, after validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSticker {
    /// A plain slug, safe to use in a file name — see
    /// [`is_valid_sticker_id`].
    pub id: String,
    pub name: String,
    pub category: String,
    pub format: Format,
    /// Bytes, 1 to [`MAX_STICKER_BYTES`].
    pub size: u64,
    /// Lowercase hex SHA-256 of the file. What a download is checked against.
    pub sha256: String,
}

/// A catalog entry as the frontend sees it: the sticker plus where to preview
/// it. The address depends on which API this build talks to, so it's worked
/// out per response rather than stored in the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    #[serde(flatten)]
    pub sticker: CatalogSticker,
    pub preview_url: String,
}

/// What `fetch_sticker_catalog` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub stickers: Vec<CatalogEntry>,
    /// The list came from the local cache because the server couldn't be
    /// reached (or answered something unusable). May be empty.
    pub offline: bool,
    /// Why, in a sentence — only when `offline`.
    pub error: Option<String>,
}

/// `GET /api/v1/stickers/{id}/file` — the only place a file address is built.
pub fn file_url(base_url: &str, id: &str) -> String {
    format!(
        "{}/api/v1/stickers/{id}/file",
        base_url.trim_end_matches('/')
    )
}

/// Mirrors the wire shape of one entry. `url` is deliberately not read.
#[derive(Debug, Deserialize)]
struct ApiSticker {
    id: String,
    name: String,
    category: String,
    format: String,
    size: u64,
    sha256: String,
}

/// Validates one entry; `None` drops it. Strict about what becomes a file
/// name or is compared against a download, lenient about what is only shown
/// (an over-long name is cut, not refused).
fn validate(raw: ApiSticker) -> Option<CatalogSticker> {
    let name: String = raw.name.trim().chars().take(MAX_NAME_CHARS).collect();
    let category = if is_category(&raw.category) {
        raw.category
    } else {
        "general".to_string()
    };
    let sha256 = raw.sha256.to_ascii_lowercase();

    let acceptable = is_valid_sticker_id(&raw.id)
        && !name.is_empty()
        && (1..=MAX_STICKER_BYTES).contains(&raw.size)
        && sha256.len() == 64
        && sha256.bytes().all(|b| b.is_ascii_hexdigit());
    if !acceptable {
        return None;
    }

    Some(CatalogSticker {
        format: Format::from_wire(&raw.format)?,
        id: raw.id,
        name,
        category,
        size: raw.size,
        sha256,
    })
}

fn is_category(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CATEGORY_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Reads the response body one entry at a time, so a single malformed row
/// costs one sticker rather than the whole page.
fn parse_catalog(body: &str) -> Result<Vec<CatalogSticker>, String> {
    #[derive(Deserialize)]
    struct Body {
        stickers: Vec<serde_json::Value>,
    }
    let body: Body = serde_json::from_str(body).map_err(|e| {
        log::warn!("could not read the sticker catalog: {e}");
        "The sticker catalog sent a response Rezure could not read.".to_string()
    })?;
    Ok(body
        .stickers
        .into_iter()
        .filter_map(|value| match serde_json::from_value::<ApiSticker>(value) {
            Ok(raw) => validate(raw),
            Err(err) => {
                log::warn!("ignoring a malformed catalog entry: {err}");
                None
            }
        })
        .collect())
}

/// What's kept on disk between launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedCatalog {
    etag: Option<String>,
    stickers: Vec<CatalogSticker>,
}

fn cache_path() -> Result<std::path::PathBuf, AppError> {
    Ok(paths::etc()?.join("stickers_catalog_cache.json"))
}

fn read_cache() -> Option<CachedCatalog> {
    let content = std::fs::read_to_string(cache_path().ok()?).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(cached: &CachedCatalog) {
    let Ok(path) = cache_path() else { return };
    if let Ok(json) = serde_json::to_string_pretty(cached) {
        let _ = std::fs::write(path, json);
    }
}

/// The cached entry for `id`, or an error that says what to do. Downloads
/// look the sticker up here rather than take its checksum and size from the
/// caller: those are what the file is verified against, so they come from the
/// catalog this module validated, not from the frontend.
pub fn cached_sticker(id: &str) -> Result<CatalogSticker, AppError> {
    read_cache()
        .and_then(|cache| cache.stickers.into_iter().find(|s| s.id == id))
        .ok_or_else(|| {
            AppError::StickerFailed(
                "That sticker isn't in the catalog any more — refresh Browse and try again."
                    .to_string(),
            )
        })
}

pub(crate) enum Fetched {
    /// `304` — the cached copy is still current.
    NotModified,
    Fresh {
        stickers: Vec<CatalogSticker>,
        etag: Option<String>,
    },
}

/// One live request. The error is a sentence for the page, not a transport
/// error to be parsed.
pub(crate) async fn fetch_from(base_url: &str, etag: Option<&str>) -> Result<Fetched, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("Couldn't set up the request: {e}"))?;
    let mut request = client.get(format!(
        "{}/api/v1/stickers",
        base_url.trim_end_matches('/')
    ));
    if let Some(value) = etag.and_then(|etag| HeaderValue::from_str(etag).ok()) {
        request = request.header(IF_NONE_MATCH, value);
    }

    let response = request
        .send()
        .await
        .map_err(|_| "Couldn't reach the sticker catalog".to_string())?;

    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Fetched::NotModified);
    }
    if !response.status().is_success() {
        return Err(format!(
            "The sticker catalog answered {}",
            response.status()
        ));
    }

    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(String::from);
    let body = response
        .text()
        .await
        .map_err(|_| "The sticker catalog's answer was cut off".to_string())?;

    Ok(Fetched::Fresh {
        stickers: parse_catalog(&body)?,
        etag,
    })
}

fn entries(base_url: &str, stickers: Vec<CatalogSticker>) -> Vec<CatalogEntry> {
    stickers
        .into_iter()
        .map(|sticker| CatalogEntry {
            preview_url: file_url(base_url, &sticker.id),
            sticker,
        })
        .collect()
}

/// The catalog for the Browse page: live when the server answers, the cache
/// when it doesn't. Never fails — an unreachable server with nothing cached is
/// an empty, `offline` catalog the page can explain.
pub async fn fetch() -> Catalog {
    let base_url = api::base_url();
    let cached = read_cache();
    let etag = cached.as_ref().and_then(|c| c.etag.as_deref());

    match fetch_from(&base_url, etag).await {
        Ok(Fetched::Fresh { stickers, etag }) => {
            write_cache(&CachedCatalog {
                etag,
                stickers: stickers.clone(),
            });
            Catalog {
                stickers: entries(&base_url, stickers),
                offline: false,
                error: None,
            }
        }
        Ok(Fetched::NotModified) => Catalog {
            stickers: entries(&base_url, cached.map(|c| c.stickers).unwrap_or_default()),
            offline: false,
            error: None,
        },
        Err(reason) => {
            log::warn!("could not fetch the sticker catalog, using the cache: {reason}");
            Catalog {
                stickers: entries(&base_url, cached.map(|c| c.stickers).unwrap_or_default()),
                offline: true,
                error: Some(reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_http::{serve, Reply};

    const SHA: &str = "a3f1c2e4b5d60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

    fn wire(id: &str, extra: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Pink bow","category":"girls","format":"svg","size":1832,"sha256":"{SHA}","url":"https://evil.example/x"{extra}}}"#
        )
    }

    fn body(entries: &[String]) -> String {
        format!(r#"{{"stickers":[{}]}}"#, entries.join(","))
    }

    #[test]
    fn a_valid_entry_is_read_and_the_servers_url_is_ignored() {
        let stickers = parse_catalog(&body(&[wire("pink-bow", "")])).unwrap();
        assert_eq!(
            stickers,
            vec![CatalogSticker {
                id: "pink-bow".into(),
                name: "Pink bow".into(),
                category: "girls".into(),
                format: Format::Svg,
                size: 1832,
                sha256: SHA.into(),
            }]
        );
        assert_eq!(
            file_url("https://api.redscale.my.id/", "pink-bow"),
            "https://api.redscale.my.id/api/v1/stickers/pink-bow/file"
        );
    }

    #[test]
    fn an_entry_that_could_do_harm_is_dropped_and_the_rest_kept() {
        let entries = [
            wire("good-one", ""),
            wire("../../evil", ""),
            wire("UPPER", ""),
            wire("", ""),
            wire("bad-format", "").replace(r#""format":"svg""#, r#""format":"exe""#),
            wire("short-hash", "").replace(SHA, "abc"),
            wire("not-hex", "").replace(SHA, &"z".repeat(64)),
            wire("empty", "").replace(r#""size":1832"#, r#""size":0"#),
            wire("huge", "").replace(r#""size":1832"#, r#""size":99999999"#),
            wire("nameless", "").replace("Pink bow", "   "),
            r#"{"id":"missing-fields"}"#.to_string(),
            r#""just a string""#.to_string(),
        ];

        let kept: Vec<_> = parse_catalog(&body(&entries))
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(kept, vec!["good-one"]);
    }

    #[test]
    fn display_fields_are_tidied_not_refused() {
        let long = "n".repeat(300);
        let entry = wire("long-name", "")
            .replace("Pink bow", &format!("  {long}  "))
            .replace(r#""category":"girls""#, r#""category":"Not A Slug!""#)
            .replace(SHA, &SHA.to_ascii_uppercase());
        let sticker = parse_catalog(&body(&[entry])).unwrap().remove(0);

        assert_eq!(sticker.name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(sticker.category, "general");
        assert_eq!(sticker.sha256, SHA, "the checksum is compared in lowercase");
    }

    #[test]
    fn a_body_that_is_not_a_catalog_is_an_error() {
        assert!(parse_catalog("<html>").is_err());
        assert!(parse_catalog(r#"{"nope":[]}"#).is_err());
        assert_eq!(parse_catalog(r#"{"stickers":[]}"#).unwrap(), vec![]);
    }

    #[tokio::test]
    async fn a_fresh_catalog_comes_back_with_its_etag() {
        let server = serve(vec![(
            "/api/v1/stickers",
            Reply::json(&body(&[wire("pink-bow", "")])).with_header("ETag", "\"abc\""),
        )]);

        let Fetched::Fresh { stickers, etag } = fetch_from(&server.base_url, None).await.unwrap()
        else {
            panic!("expected a fresh catalog");
        };
        assert_eq!(stickers.len(), 1);
        assert_eq!(etag.as_deref(), Some("\"abc\""));
    }

    #[tokio::test]
    async fn the_cached_etag_is_sent_and_a_304_means_keep_the_cache() {
        let server = serve(vec![("/api/v1/stickers", Reply::status(304))]);

        let outcome = fetch_from(&server.base_url, Some("\"abc\"")).await.unwrap();
        assert!(matches!(outcome, Fetched::NotModified));
        assert_eq!(
            server.last_request_header("if-none-match").as_deref(),
            Some("\"abc\"")
        );
    }

    #[tokio::test]
    async fn a_server_error_is_a_sentence_for_the_page() {
        let server = serve(vec![("/api/v1/stickers", Reply::status(503))]);

        let Err(reason) = fetch_from(&server.base_url, None).await else {
            panic!("expected an error");
        };
        assert!(reason.contains("503"), "{reason}");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_sentence_for_the_page() {
        // Nothing listens on port 1.
        let Err(reason) = fetch_from("http://127.0.0.1:1", None).await else {
            panic!("expected an error");
        };
        assert!(reason.contains("Couldn't reach"), "{reason}");
    }
}
