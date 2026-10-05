//! Fetches donation/support links from `rezure-dashboard` and keeps a local
//! cache, same shape as `services::changelog` — a maintainer changes these
//! from the dashboard without shipping a new Rezure build, and a stale cache
//! beats an error banner on a page this low-stakes.
//!
//! Unlike `changelog`, this cache also remembers the `Last-Modified` header
//! from the last successful fetch and sends it back as `If-Modified-Since`.
//! The backend answers with a bodyless `304` when nothing changed (see
//! `Api\V1\DonateController` in `laravel-api`), so the Donate page's normal
//! case — maintainer hasn't touched the config since last launch — is a cheap
//! round trip that confirms the cache is still good, not a full re-download.
//! On any failure (offline, `304`, anything else not a fresh `200`) the page
//! renders straight from the cache, so it never depends on this endpoint being
//! reachable on every open.
//!
//! # Pictures
//!
//! The config may describe pictures: one QRIS, and an optional logo on each
//! link and wallet. The descriptions travel with the config (cached beside
//! it); the pictures are separate downloads, verified and kept on disk by
//! `services::donate_images`, which is also what keeps them on the page when
//! the endpoint is down. Where each is kept:
//!
//! * the QRIS: `etc\donate_qris.<ext>`
//! * a logo: `etc\donate_icons\<method id>.<ext>`
//!
//! Addresses are built here from known paths and ids — never followed from a
//! `url` in a response.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use futures_util::future::join_all;
use reqwest::header::{HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED};
use serde::{Deserialize, Serialize};

use super::donate_images::{
    resolve, ApiImage, DataImage, ImageDescriptor, ImageFormat, ImageOutcome, Slot, MAX_ICON_BYTES,
    MAX_QRIS_BYTES,
};
use crate::config::api;
use crate::utils::error::AppError;
use crate::utils::paths;

/// Longest wallet network shown. The server caps it too; this keeps a
/// misbehaving one from stretching the page.
const MAX_NETWORK_CHARS: usize = 60;

/// A logo's description plus the donate method it belongs to (the server
/// serves it per method, by that method's `id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconDescriptor {
    pub method: u64,
    #[serde(flatten)]
    pub image: ImageDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonateLink {
    /// The donate method's id; `None` from a server that predates ids.
    #[serde(default)]
    pub id: Option<u64>,
    pub label: String,
    pub url: String,
    /// A cache written before logos existed has none.
    #[serde(default)]
    pub icon: Option<IconDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoWallet {
    #[serde(default)]
    pub id: Option<u64>,
    pub symbol: String,
    /// The blockchain the address is on (`Tron (TRC-20)`). `None` for a wallet
    /// the maintainer saved before the field existed.
    #[serde(default)]
    pub network: Option<String>,
    pub label: String,
    pub address: String,
    #[serde(default)]
    pub icon: Option<IconDescriptor>,
}

/// What's cached: the config as the server described it, pictures included
/// only as descriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonateConfig {
    pub message: String,
    pub local: Vec<DonateLink>,
    pub global: Vec<DonateLink>,
    pub crypto: Vec<CryptoWallet>,
    /// What the server says about the QRIS image, if it has one. A cache
    /// written before this existed has none, and reads as "no QRIS".
    #[serde(default)]
    pub qris: Option<ImageDescriptor>,
}

impl Default for DonateConfig {
    fn default() -> Self {
        Self {
            message: "Rezure is free & open-source — support keeps it going.".to_string(),
            local: Vec::new(),
            global: Vec::new(),
            crypto: Vec::new(),
            qris: None,
        }
    }
}

/// Mirrors `GET /api/v1/support/donate`'s snake_case wire shape — see
/// `services::changelog`'s `ApiChangelogEntry` for why this isn't deserialized
/// straight into `DonateConfig`.
///
/// Every picture description is kept as raw JSON until it is validated, so an
/// odd-looking one costs the picture, never the page.
#[derive(Debug, Deserialize)]
struct ApiDonateLink {
    /// The donate method's id. Absent from servers that predate logos.
    #[serde(default)]
    id: Option<u64>,
    label: String,
    url: String,
    #[serde(default)]
    icon: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ApiCryptoWallet {
    #[serde(default)]
    id: Option<u64>,
    symbol: String,
    #[serde(default)]
    network: Option<String>,
    label: String,
    address: String,
    #[serde(default)]
    icon: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ApiDonateConfig {
    message: String,
    local: Vec<ApiDonateLink>,
    global: Vec<ApiDonateLink>,
    crypto: Vec<ApiCryptoWallet>,
    /// Absent from servers that predate it, `null` until one is uploaded.
    #[serde(default)]
    qris: Option<serde_json::Value>,
}

/// A logo's description, tied to the method it was listed under. The `url`
/// beside it in the response is not read: the address fetched is built from
/// the method's `id`. No `id` (an older server) means no logo.
fn icon_from_wire(id: Option<u64>, value: Option<serde_json::Value>) -> Option<IconDescriptor> {
    let image: ApiImage = serde_json::from_value(value?).ok()?;
    Some(IconDescriptor {
        method: id?,
        image: image.validated(&ImageFormat::ICON, MAX_ICON_BYTES)?,
    })
}

/// A network name worth showing: trimmed, bounded, and `None` when blank.
fn network_from_wire(network: Option<String>) -> Option<String> {
    let network: String = network?.trim().chars().take(MAX_NETWORK_CHARS).collect();
    (!network.is_empty()).then_some(network)
}

impl From<ApiDonateConfig> for DonateConfig {
    fn from(config: ApiDonateConfig) -> Self {
        let link = |l: ApiDonateLink| DonateLink {
            id: l.id,
            label: l.label,
            url: l.url,
            icon: icon_from_wire(l.id, l.icon),
        };
        Self {
            message: config.message,
            local: config.local.into_iter().map(link).collect(),
            global: config.global.into_iter().map(link).collect(),
            crypto: config
                .crypto
                .into_iter()
                .map(|w| CryptoWallet {
                    id: w.id,
                    symbol: w.symbol,
                    network: network_from_wire(w.network),
                    label: w.label,
                    address: w.address,
                    icon: icon_from_wire(w.id, w.icon),
                })
                .collect(),
            qris: config
                .qris
                .and_then(|value| serde_json::from_value::<ApiImage>(value).ok())
                .and_then(|image| image.validated(&ImageFormat::QRIS, MAX_QRIS_BYTES)),
        }
    }
}

/// What's actually persisted to disk — the config plus the `Last-Modified`
/// it came with, so the next launch can ask the server "anything newer than
/// this?" instead of unconditionally re-downloading.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDonateConfig {
    /// Which Rezure's idea of the config this was written with. A cache from
    /// before pictures existed has none (`0`): its `Last-Modified` would get a
    /// `304` from a server that has had pictures since, and they would never
    /// arrive. See [`CACHE_VERSION`].
    #[serde(default)]
    version: u32,
    last_modified: Option<String>,
    config: DonateConfig,
}

/// Bumped whenever the config grows something an older cache can't hold:
/// `1` added the QRIS, `2` added logos on links and wallets and the network
/// of a wallet. A cache with a
/// lower version is still *shown* (a stale page beats an empty one) but is
/// never used to ask "anything newer?" — the server is asked for the whole
/// thing once, and the cache is rewritten at the current version.
const CACHE_VERSION: u32 = 2;

/// The `Last-Modified` worth sending back, if the cache is current enough to
/// vouch for it.
fn conditional_since(cached: Option<&CachedDonateConfig>) -> Option<&str> {
    cached
        .filter(|cache| cache.version >= CACHE_VERSION)
        .and_then(|cache| cache.last_modified.as_deref())
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

    Ok(FetchOutcome::Fresh(
        DonateConfig::from(config),
        last_modified,
    ))
}

/// Fetches the donation config, refreshing the local cache on a fresh `200`
/// and falling back to it on `304` or any failure. Returns
/// `DonateConfig::default()` — an empty, harmless config — only when
/// there's neither a live response nor a cache yet, so the page renders its
/// "nothing configured" state instead of an error.
///
/// The flag says the server couldn't be asked: what came back is the cache.
async fn fetch_config() -> (DonateConfig, bool) {
    let cached = read_cache();

    match fetch_live(conditional_since(cached.as_ref())).await {
        Ok(FetchOutcome::NotModified) => (cached.map(|c| c.config).unwrap_or_default(), false),
        Ok(FetchOutcome::Fresh(config, last_modified)) => {
            write_cache(&CachedDonateConfig {
                version: CACHE_VERSION,
                last_modified,
                config: config.clone(),
            });
            (config, false)
        }
        Err(err) => {
            log::warn!("could not fetch donation links, using cache: {err}");
            (cached.map(|c| c.config).unwrap_or_default(), true)
        }
    }
}

/// A donate link, as the page renders it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonateLinkView {
    /// What the page keys the entry by. `None` only from a server that
    /// predates ids.
    pub id: Option<u64>,
    pub label: String,
    pub url: String,
    pub icon: Option<DataImage>,
}

/// A crypto wallet, as the page renders it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoWalletView {
    /// What the page keys a wallet — and its QR code — by. A coin on several
    /// networks is several wallets with the same `symbol`, so the symbol
    /// alone can't tell them apart.
    pub id: Option<u64>,
    pub symbol: String,
    pub network: Option<String>,
    pub label: String,
    pub address: String,
    pub icon: Option<DataImage>,
}

/// What the Donate page renders: the config, plus its pictures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonatePage {
    pub message: String,
    pub local: Vec<DonateLinkView>,
    pub global: Vec<DonateLinkView>,
    pub crypto: Vec<CryptoWalletView>,
    pub qris: Option<DataImage>,
    /// The server has a QRIS but there's no picture of it to show at all —
    /// never downloaded, and the download failed or didn't verify. The page
    /// says so instead of silently showing nothing.
    pub qris_unavailable: bool,
    /// The picture shown is the one saved earlier, because the one the server
    /// now describes couldn't be got. It may have been replaced since; the
    /// page says so.
    pub qris_stale: bool,
    /// The server couldn't be asked, so everything above is the last it said.
    pub offline: bool,
}

/// `GET /api/v1/support/donate/qris` — built here, not read from the response.
fn qris_url(base_url: &str) -> String {
    format!(
        "{}/api/v1/support/donate/qris",
        base_url.trim_end_matches('/')
    )
}

/// `GET /api/v1/support/donate/methods/{id}/icon`.
fn icon_url(base_url: &str, method: u64) -> String {
    format!(
        "{}/api/v1/support/donate/methods/{method}/icon",
        base_url.trim_end_matches('/')
    )
}

async fn resolve_qris(
    etc: &Path,
    base_url: &str,
    descriptor: Option<&ImageDescriptor>,
    offline: bool,
) -> ImageOutcome {
    let slot = Slot::new(etc, "donate_qris", MAX_QRIS_BYTES);
    resolve(&slot, &qris_url(base_url), descriptor, offline).await
}

/// Every logo the config describes, once each.
fn icon_descriptors(config: &DonateConfig) -> Vec<&IconDescriptor> {
    let mut seen = HashSet::new();
    config
        .local
        .iter()
        .chain(&config.global)
        .filter_map(|link| link.icon.as_ref())
        .chain(config.crypto.iter().filter_map(|w| w.icon.as_ref()))
        .filter(|icon| seen.insert(icon.method))
        .collect()
}

/// Gets every logo, by donate method. A logo that can't be got is simply
/// missing from the result: it is decoration, and the link or wallet beside it
/// works without.
async fn resolve_icons(
    etc: &Path,
    base_url: &str,
    config: &DonateConfig,
    offline: bool,
) -> HashMap<u64, DataImage> {
    let dir = etc.join("donate_icons");
    let wanted = icon_descriptors(config);

    let outcomes = join_all(wanted.iter().map(|icon| async {
        let slot = Slot::new(&dir, icon.method.to_string(), MAX_ICON_BYTES);
        let outcome = resolve(
            &slot,
            &icon_url(base_url, icon.method),
            Some(&icon.image),
            offline,
        )
        .await;
        (icon.method, outcome)
    }))
    .await;

    // Only when the server really answered: a logo for a method that no
    // longer has one, or no longer exists, would otherwise stay on disk.
    if !offline {
        let in_use: HashSet<u64> = wanted.iter().map(|icon| icon.method).collect();
        forget_unused_icons(&dir, &in_use);
    }

    outcomes
        .into_iter()
        .filter_map(|(method, outcome)| Some((method, outcome.image?)))
        .collect()
}

/// Deletes saved logos of methods that are no longer in use, and any
/// half-written leftovers.
fn forget_unused_icons(dir: &Path, in_use: &HashSet<u64>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let wanted = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u64>().ok())
            .is_some_and(|method| in_use.contains(&method));
        if !wanted && path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Everything the Donate page needs: the config (live or cached) and its
/// pictures.
pub async fn fetch() -> DonatePage {
    let (config, offline) = fetch_config().await;

    let (qris, icons) = match paths::etc() {
        Ok(etc) => {
            let base_url = api::base_url();
            let qris = resolve_qris(&etc, &base_url, config.qris.as_ref(), offline).await;
            let icons = resolve_icons(&etc, &base_url, &config, offline).await;
            (qris, icons)
        }
        // Nowhere to keep a picture means nowhere to verify one into.
        Err(_) if config.qris.is_some() => (ImageOutcome::unavailable(), HashMap::new()),
        Err(_) => (ImageOutcome::default(), HashMap::new()),
    };

    page(config, qris, &icons, offline)
}

/// Puts the pictures beside the config they belong to.
fn page(
    config: DonateConfig,
    qris: ImageOutcome,
    icons: &HashMap<u64, DataImage>,
    offline: bool,
) -> DonatePage {
    let icon_of = |icon: &Option<IconDescriptor>| {
        icon.as_ref()
            .and_then(|icon| icons.get(&icon.method).cloned())
    };
    let link = |l: DonateLink| DonateLinkView {
        id: l.id,
        icon: icon_of(&l.icon),
        label: l.label,
        url: l.url,
    };

    DonatePage {
        message: config.message,
        local: config.local.into_iter().map(link).collect(),
        global: config.global.into_iter().map(link).collect(),
        crypto: config
            .crypto
            .into_iter()
            .map(|w| CryptoWalletView {
                id: w.id,
                icon: icon_of(&w.icon),
                symbol: w.symbol,
                network: w.network,
                label: w.label,
                address: w.address,
            })
            .collect(),
        qris: qris.image,
        qris_unavailable: qris.unavailable,
        qris_stale: qris.stale,
        offline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_http::{serve, Reply};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR some pixels";
    const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 some pixels";
    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>"##;

    /// A scratch folder standing in for `etc\`, removed when the test is done.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("rezure-donate-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn icons(&self, name: &str) -> std::path::PathBuf {
            self.0.join("donate_icons").join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sha(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(bytes))
    }

    /// A logo as the server describes it on the wire.
    fn wire_icon(method: u64, format: &str, bytes: &[u8]) -> String {
        format!(
            r#"{{"format":"{format}","size":{},"sha256":"{}","url":"https://api.example/api/v1/support/donate/methods/{method}/icon"}}"#,
            bytes.len(),
            sha(bytes)
        )
    }

    fn config_json(qris: &str, global_icon: &str, crypto_icon: &str) -> String {
        format!(
            r#"{{"message":"hi","local":[],
            "global":[{{"id":7,"label":"GitHub","url":"https://x.test","icon":{global_icon}}}],
            "crypto":[{{"id":12,"symbol":"BTC","network":"Bitcoin","label":"Bitcoin","address":"bc1","icon":{crypto_icon}}}],
            "qris":{qris}}}"#
        )
    }

    fn parse(json: &str) -> DonateConfig {
        DonateConfig::from(serde_json::from_str::<ApiDonateConfig>(json).unwrap())
    }

    // ---- reading the description -------------------------------------------

    #[test]
    fn a_qris_description_is_read_and_its_url_is_ignored() {
        let sha = "a".repeat(64);
        let config = parse(&config_json(
            &format!(
                r#"{{"format":"jpg","size":159658,"sha256":"{}","url":"https://evil.example/q"}}"#,
                sha.to_ascii_uppercase()
            ),
            "null",
            "null",
        ));

        assert_eq!(
            config.qris,
            Some(ImageDescriptor {
                format: ImageFormat::Jpg,
                size: 159_658,
                sha256: sha,
            })
        );
    }

    #[test]
    fn a_server_with_no_pictures_reads_as_none() {
        let config = parse(&config_json("null", "null", "null"));
        assert_eq!(config.qris, None);
        assert_eq!(config.global[0].icon, None);
        assert_eq!(config.crypto[0].icon, None);

        // A server from before any of it existed doesn't send the keys at all.
        let older = r#"{"message":"hi","local":[],"global":[{"label":"G","url":"https://x.test"}],"crypto":[{"symbol":"BTC","label":"B","address":"a"}]}"#;
        let config = parse(older);
        assert_eq!(config.qris, None);
        assert_eq!(config.global[0].icon, None);
        assert_eq!(config.crypto[0].icon, None);
    }

    /// The pictures are optional; a description that can't be trusted costs the
    /// picture, never the links and wallets beside it.
    #[test]
    fn an_unusable_description_costs_the_picture_not_the_page() {
        let sha = "b".repeat(64);
        for bad in [
            format!(r#"{{"format":"gif","size":10,"sha256":"{sha}"}}"#),
            format!(r#"{{"format":"png","size":0,"sha256":"{sha}"}}"#),
            format!(r#"{{"format":"png","size":99999999,"sha256":"{sha}"}}"#),
            r#"{"format":"png","size":10,"sha256":"short"}"#.to_string(),
            format!(r#"{{"format":"png","size":"ten","sha256":"{sha}"}}"#),
            r#""just a string""#.to_string(),
        ] {
            let config = parse(&config_json(&bad, &bad, &bad));
            assert_eq!(config.qris, None, "{bad}");
            assert_eq!(config.global.len(), 1, "{bad}");
            assert_eq!(config.global[0].icon, None, "{bad}");
            assert_eq!(config.crypto[0].icon, None, "{bad}");
        }
    }

    #[test]
    fn a_logo_is_read_with_the_method_it_belongs_to() {
        let config = parse(&config_json(
            "null",
            &wire_icon(7, "svg", SVG),
            &wire_icon(12, "webp", WEBP),
        ));

        let github = config.global[0].icon.as_ref().unwrap();
        assert_eq!(github.method, 7);
        assert_eq!(github.image.format, ImageFormat::Svg);
        assert_eq!(config.crypto[0].icon.as_ref().unwrap().method, 12);
    }

    /// The logo's address comes from the entry's `id`, so without one — a
    /// server from before logos — there is nothing to ask for.
    #[test]
    fn a_logo_needs_its_methods_id_and_a_format_a_logo_can_be() {
        let icon = wire_icon(3, "png", PNG);
        let with_id = r#"{"id":3,"label":"G","url":"https://x.test","icon":ICON}"#;
        let without_id = r#"{"label":"G","url":"https://x.test","icon":ICON}"#;
        let read = |entry: &str, icon: &str| {
            let json = format!(
                r#"{{"message":"hi","local":[{}],"global":[],"crypto":[]}}"#,
                entry.replace("ICON", icon)
            );
            parse(&json).local[0].icon.clone()
        };

        assert_eq!(read(with_id, &icon).map(|i| i.method), Some(3));
        assert_eq!(read(without_id, &icon), None);
        // A logo may be a JPEG (a coin logo saved from a web page often is)...
        let jpg = read(with_id, &wire_icon(3, "jpg", b"\xFF\xD8\xFFjpeg")).unwrap();
        assert_eq!(jpg.image.format, ImageFormat::Jpg);
        // ...but not just anything.
        assert_eq!(read(with_id, &wire_icon(3, "gif", PNG)), None);
    }

    /// The same coin on two networks is two wallets with the same symbol. They
    /// must stay apart — each with its own address, network and id — or the
    /// page could show one wallet's QR code beside the other's address.
    #[test]
    fn one_coin_on_two_networks_stays_two_wallets() {
        let json = r#"{"message":"hi","local":[],"global":[],"crypto":[
            {"id":5,"symbol":"USDT","network":"Tron (TRC-20)","label":"Tether","address":"TRON1"},
            {"id":6,"symbol":"USDT","network":"Ethereum (ERC-20)","label":"Tether","address":"0xETH"}
        ]}"#;
        let page = page(parse(json), ImageOutcome::default(), &HashMap::new(), false);

        let key = |w: &CryptoWalletView| (w.id, w.network.clone(), w.address.clone());
        assert_eq!(
            page.crypto.iter().map(key).collect::<Vec<_>>(),
            vec![
                (
                    Some(5),
                    Some("Tron (TRC-20)".to_string()),
                    "TRON1".to_string()
                ),
                (
                    Some(6),
                    Some("Ethereum (ERC-20)".to_string()),
                    "0xETH".to_string()
                ),
            ]
        );
    }

    #[test]
    fn the_network_of_a_wallet_is_kept_tidied_and_optional() {
        let wallet = |network: &str| {
            let json = format!(
                r#"{{"message":"hi","local":[],"global":[],"crypto":[{{"id":1,"symbol":"USDT","network":{network},"label":"Tether","address":"T1"}}]}}"#
            );
            parse(&json).crypto[0].network.clone()
        };

        assert_eq!(
            wallet(r#""Tron (TRC-20)""#).as_deref(),
            Some("Tron (TRC-20)")
        );
        assert_eq!(wallet(r#"  "  Solana  ""#).as_deref(), Some("Solana"));
        // Saved before the field existed, or blank.
        assert_eq!(wallet("null"), None);
        assert_eq!(wallet(r#""   ""#), None);
        let long = format!("\"{}\"", "n".repeat(500));
        assert_eq!(wallet(&long).unwrap().chars().count(), MAX_NETWORK_CHARS);

        // A server from before the field existed doesn't send the key at all.
        let older = r#"{"message":"hi","local":[],"global":[],"crypto":[{"symbol":"BTC","label":"B","address":"a"}]}"#;
        assert_eq!(parse(older).crypto[0].network, None);
    }

    // ---- the cache ---------------------------------------------------------

    #[test]
    fn a_cache_from_before_pictures_existed_still_reads_but_cannot_vouch_for_a_304() {
        let old = r#"{"last_modified":"Mon, 05 Oct 2026 10:00:00 GMT","config":{"message":"hi","local":[{"label":"G","url":"https://x.test"}],"global":[],"crypto":[]}}"#;
        let cached: CachedDonateConfig = serde_json::from_str(old).unwrap();

        // Still shown...
        assert_eq!(cached.config.local[0].label, "G");
        assert_eq!(cached.config.local[0].icon, None);
        assert_eq!(cached.config.qris, None);
        // ...but never used to ask "anything newer?": a server that has had
        // pictures since would answer 304, and they would never arrive.
        assert_eq!(conditional_since(Some(&cached)), None);
    }

    /// Version 1 knew the QRIS but not the logos: it must not vouch either.
    #[test]
    fn a_cache_that_knew_the_qris_but_not_logos_cannot_vouch_for_a_304_either() {
        let cached = CachedDonateConfig {
            version: 1,
            last_modified: Some("Mon, 05 Oct 2026 10:00:00 GMT".to_string()),
            config: DonateConfig::default(),
        };

        assert_eq!(conditional_since(Some(&cached)), None);
    }

    #[test]
    fn a_current_cache_sends_its_last_modified_back() {
        let cached = CachedDonateConfig {
            version: CACHE_VERSION,
            last_modified: Some("Mon, 05 Oct 2026 10:00:00 GMT".to_string()),
            config: DonateConfig::default(),
        };

        assert_eq!(
            conditional_since(Some(&cached)),
            Some("Mon, 05 Oct 2026 10:00:00 GMT")
        );
        assert_eq!(conditional_since(None), None);
    }

    #[test]
    fn descriptions_survive_a_round_trip_through_the_cache() {
        let config = parse(&config_json(
            &format!(
                r#"{{"format":"png","size":{},"sha256":"{}","url":"u"}}"#,
                PNG.len(),
                sha(PNG)
            ),
            &wire_icon(7, "svg", SVG),
            &wire_icon(12, "webp", WEBP),
        ));
        let cached = CachedDonateConfig {
            version: CACHE_VERSION,
            last_modified: None,
            config: config.clone(),
        };

        let json = serde_json::to_string(&cached).unwrap();
        let back: CachedDonateConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(back.config.qris, config.qris);
        assert_eq!(back.config.global, config.global);
        assert_eq!(back.config.crypto, config.crypto);
    }

    // ---- logos -------------------------------------------------------------

    /// The test server's route for one logo. (The server wants `&'static str`
    /// paths; a handful of leaked strings in a test process is harmless.)
    fn route(method: u64, reply: Reply) -> (&'static str, Reply) {
        let path = format!("/api/v1/support/donate/methods/{method}/icon");
        (Box::leak(path.into_boxed_str()), reply)
    }

    #[tokio::test]
    async fn logos_are_downloaded_by_method_from_addresses_built_here() {
        let scratch = Scratch::new();
        // `url` in the response names a host that isn't the server: it must
        // not be followed.
        let config = parse(&config_json(
            "null",
            &wire_icon(7, "svg", SVG),
            &wire_icon(12, "webp", WEBP),
        ));
        let server = serve(vec![
            route(7, Reply::bytes(SVG)),
            route(12, Reply::bytes(WEBP)),
        ]);

        let icons = resolve_icons(&scratch.0, &server.base_url, &config, false).await;

        assert_eq!(icons.len(), 2);
        assert_eq!(icons[&7].format, ImageFormat::Svg);
        assert_eq!(icons[&12].format, ImageFormat::Webp);
        assert_eq!(std::fs::read(scratch.icons("7.svg")).unwrap(), SVG);
        assert_eq!(std::fs::read(scratch.icons("12.webp")).unwrap(), WEBP);
    }

    #[tokio::test]
    async fn saved_logos_are_not_fetched_again() {
        let scratch = Scratch::new();
        let config = parse(&config_json("null", &wire_icon(7, "svg", SVG), "null"));
        let server = serve(vec![route(7, Reply::bytes(SVG))]);

        resolve_icons(&scratch.0, &server.base_url, &config, false).await;
        let again = resolve_icons(&scratch.0, &server.base_url, &config, false).await;

        assert_eq!(again.len(), 1);
        assert_eq!(server.requests().len(), 1);
    }

    /// The endpoint is dead and the page still has its logos.
    #[tokio::test]
    async fn offline_the_saved_logos_are_still_there() {
        let scratch = Scratch::new();
        let config = parse(&config_json("null", &wire_icon(7, "svg", SVG), "null"));
        let server = serve(vec![route(7, Reply::bytes(SVG))]);
        resolve_icons(&scratch.0, &server.base_url, &config, false).await;

        let offline = resolve_icons(&scratch.0, "http://127.0.0.1:1", &config, true).await;

        assert_eq!(offline.len(), 1);
        assert_eq!(offline[&7].format, ImageFormat::Svg);
    }

    #[tokio::test]
    async fn a_logo_that_cannot_be_got_costs_only_that_logo() {
        let scratch = Scratch::new();
        let config = parse(&config_json(
            "null",
            &wire_icon(7, "svg", SVG),
            &wire_icon(12, "webp", WEBP),
        ));
        // 7 is gone from the server; 12 is fine.
        let server = serve(vec![
            route(7, Reply::status(404)),
            route(12, Reply::bytes(WEBP)),
        ]);

        let icons = resolve_icons(&scratch.0, &server.base_url, &config, false).await;

        assert!(!icons.contains_key(&7));
        assert!(icons.contains_key(&12));
    }

    #[tokio::test]
    async fn a_replaced_logo_that_cannot_be_downloaded_keeps_the_earlier_one() {
        let scratch = Scratch::new();
        let before = parse(&config_json("null", &wire_icon(7, "png", PNG), "null"));
        let server = serve(vec![route(7, Reply::bytes(PNG))]);
        resolve_icons(&scratch.0, &server.base_url, &before, false).await;

        // Replaced on the server; the new file can't be fetched.
        let after = parse(&config_json("null", &wire_icon(7, "webp", WEBP), "null"));
        let broken = serve(vec![route(7, Reply::status(500))]);
        let icons = resolve_icons(&scratch.0, &broken.base_url, &after, false).await;

        assert_eq!(icons[&7].sha256, sha(PNG), "the earlier logo stays");
    }

    #[tokio::test]
    async fn logos_of_methods_that_are_gone_are_cleaned_up_but_only_when_the_server_answered() {
        let scratch = Scratch::new();
        let both = parse(&config_json(
            "null",
            &wire_icon(7, "svg", SVG),
            &wire_icon(12, "webp", WEBP),
        ));
        let server = serve(vec![
            route(7, Reply::bytes(SVG)),
            route(12, Reply::bytes(WEBP)),
        ]);
        resolve_icons(&scratch.0, &server.base_url, &both, false).await;
        // A leftover from an interrupted write.
        std::fs::write(scratch.icons("7.svg.part"), b"half").unwrap();

        // Method 12 was deleted on the server.
        let only_github = parse(&config_json("null", &wire_icon(7, "svg", SVG), "null"));

        // Offline, nothing is deleted: "no logo" may only be a stale cache.
        resolve_icons(&scratch.0, "http://127.0.0.1:1", &only_github, true).await;
        assert!(scratch.icons("12.webp").is_file());

        resolve_icons(&scratch.0, &server.base_url, &only_github, false).await;
        assert!(scratch.icons("7.svg").is_file());
        assert!(
            !scratch.icons("12.webp").exists(),
            "no logo left for a deleted method"
        );
        assert!(
            !scratch.icons("7.svg.part").exists(),
            "no half-written leftovers"
        );
    }

    #[tokio::test]
    async fn a_method_listed_twice_has_its_logo_fetched_once() {
        let scratch = Scratch::new();
        let icon = icon_from_wire(
            Some(7),
            serde_json::from_str(&wire_icon(7, "svg", SVG)).ok(),
        );
        let link = |label: &str| DonateLink {
            id: Some(7),
            label: label.to_string(),
            url: "https://x.test".to_string(),
            icon: icon.clone(),
        };
        let config = DonateConfig {
            local: vec![link("a")],
            global: vec![link("b")],
            ..DonateConfig::default()
        };
        let server = serve(vec![route(7, Reply::bytes(SVG))]);

        let icons = resolve_icons(&scratch.0, &server.base_url, &config, false).await;

        assert_eq!(icons.len(), 1);
        assert_eq!(server.requests().len(), 1);
    }

    // ---- the page ----------------------------------------------------------

    #[tokio::test]
    async fn the_page_puts_each_logo_beside_its_own_link_and_wallet() {
        let scratch = Scratch::new();
        let config = parse(&config_json(
            "null",
            &wire_icon(7, "svg", SVG),
            &wire_icon(12, "webp", WEBP),
        ));
        let server = serve(vec![
            route(7, Reply::bytes(SVG)),
            route(12, Reply::bytes(WEBP)),
        ]);
        let icons = resolve_icons(&scratch.0, &server.base_url, &config, false).await;

        let page = page(config, ImageOutcome::default(), &icons, false);

        assert_eq!(page.global[0].label, "GitHub");
        assert_eq!(
            page.global[0].icon.as_ref().unwrap().format,
            ImageFormat::Svg
        );
        assert!(page.global[0]
            .icon
            .as_ref()
            .unwrap()
            .data_url
            .starts_with("data:image/svg+xml;base64,"));
        assert_eq!(
            page.crypto[0].icon.as_ref().unwrap().format,
            ImageFormat::Webp
        );
        assert!(!page.offline && page.qris.is_none());
    }

    #[test]
    fn a_link_without_a_logo_is_still_a_link() {
        let config = parse(&config_json("null", "null", "null"));

        let page = page(config, ImageOutcome::default(), &HashMap::new(), true);

        assert_eq!(page.global[0].label, "GitHub");
        assert_eq!(page.global[0].icon, None);
        assert!(page.offline);
    }

    // ---- the real server ---------------------------------------------------

    /// Reads the real donate endpoint (`config::api::base_url`) and everything
    /// it describes into a scratch folder: the live config parses, each real
    /// picture passes verification, and once saved they still show with the
    /// server gone. Only public GETs. Run with
    /// `cargo test --lib against_the_live_api -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "talks to the live API"]
    async fn against_the_live_api() {
        let base = api::base_url();
        let scratch = Scratch::new();

        let FetchOutcome::Fresh(config, last_modified) = fetch_live(None).await.unwrap() else {
            panic!("a first fetch can't be a 304");
        };
        println!(
            "message: {:?}\nlast-modified: {last_modified:?}",
            config.message
        );
        println!(
            "links: {} local, {} global, {} crypto",
            config.local.len(),
            config.global.len(),
            config.crypto.len()
        );

        let qris = resolve_qris(&scratch.0, &base, config.qris.as_ref(), false).await;
        println!(
            "qris: described {:?}, shown {}",
            config.qris,
            qris.image.is_some()
        );
        let icons = resolve_icons(&scratch.0, &base, &config, false).await;
        println!("logos described: {:?}", icon_descriptors(&config));
        println!("logos downloaded and verified: {}", icons.len());
        assert_eq!(
            icons.len(),
            icon_descriptors(&config).len(),
            "every described logo verifies"
        );

        // The server goes away: everything saved is still there.
        let gone = "http://127.0.0.1:1";
        let qris = resolve_qris(&scratch.0, gone, config.qris.as_ref(), true).await;
        let icons = resolve_icons(&scratch.0, gone, &config, true).await;
        assert_eq!(qris.image.is_some(), config.qris.is_some());
        assert_eq!(icons.len(), icon_descriptors(&config).len());
        println!("offline: {} logos and the QRIS still shown", icons.len());
    }
}
