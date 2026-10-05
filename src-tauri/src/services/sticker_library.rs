//! Stickers the user downloaded from **Decorations → Browse**, and the checks
//! a download passes before it is kept.
//!
//! # Where they live
//!
//! `<home>\stickers\<id>.<ext>` holds the image; `<home>\etc\stickers.json`
//! holds what the file can't say about itself — its name and category — and
//! the checksum it was verified against. The index is lenient to read: an
//! entry whose file is gone, or whose id isn't a plain slug, is dropped, so
//! deleting an image by hand removes the sticker instead of leaving a broken
//! one in the palette.
//!
//! # Why a download is distrusted
//!
//! A sticker is a file fetched from a server and later drawn on every page of
//! the app. Showing it in an `<img>` already keeps an SVG from running
//! anything; this module adds the checks that don't depend on that:
//!
//! * the URL is built from the sticker's id, never taken from the response;
//! * at most the size the catalog promised is read, and never more than
//!   [`MAX_STICKER_BYTES`];
//! * the bytes must hash to the catalog's checksum, **and** look like the
//!   format the catalog named (PNG/WebP by signature, SVG by being an SVG
//!   with none of the markup a sticker has no use for);
//! * the id is a plain slug, so it can't steer a file name outside the folder.
//!
//! Anything that fails is not saved at all — there is no "kept anyway".

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::sticker_catalog::{self, file_url, CatalogSticker, Format, MAX_STICKER_BYTES};
use crate::config::api;
use crate::config::stickers::is_valid_sticker_id;
use crate::utils::bounded_get::{get_bounded, GetError};
use crate::utils::error::AppError;
use crate::utils::paths;

/// Most downloaded stickers kept at once. Each is at most 256 KB and is held
/// in memory as a data URL while the app runs, so this bounds that.
pub const MAX_SAVED: usize = 100;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15);

fn fail(reason: impl Into<String>) -> AppError {
    AppError::StickerFailed(reason.into())
}

/// A downloaded sticker, as remembered in `stickers.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSticker {
    pub id: String,
    pub name: String,
    pub category: String,
    pub format: Format,
    pub size: u64,
    /// What the file was verified against; compared with the catalog's to tell
    /// when the server has replaced the image.
    pub sha256: String,
}

/// A downloaded sticker with its image inline, which is how the frontend
/// draws it: a data URL needs no file-access permission and no cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedStickerView {
    #[serde(flatten)]
    pub sticker: SavedSticker,
    pub data_url: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    stickers: Vec<SavedSticker>,
}

/// The downloaded stickers on disk. Constructed over explicit paths so tests
/// can use a scratch folder; the app has exactly one, behind [`with_library`].
pub struct Library {
    dir: PathBuf,
    index_path: PathBuf,
    entries: Vec<SavedSticker>,
}

impl Library {
    /// Opens the library at `dir`, keeping only entries that are still valid
    /// and still have their image.
    pub fn open(dir: PathBuf, index_path: PathBuf) -> Self {
        let raw: Vec<serde_json::Value> = std::fs::read_to_string(&index_path)
            .ok()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .and_then(|value| match value.get("stickers")? {
                serde_json::Value::Array(items) => Some(items.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let mut library = Self {
            dir,
            index_path,
            entries: Vec::new(),
        };
        for value in raw {
            let Ok(sticker) = serde_json::from_value::<SavedSticker>(value) else {
                continue;
            };
            let usable = is_valid_sticker_id(&sticker.id)
                && library.get(&sticker.id).is_none()
                && std::fs::metadata(library.file_path(&sticker))
                    .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_STICKER_BYTES);
            if usable {
                library.entries.push(sticker);
            }
        }
        library
    }

    fn open_default() -> Result<Self, AppError> {
        Ok(Self::open(
            paths::stickers()?,
            paths::etc()?.join("stickers.json"),
        ))
    }

    pub fn entries(&self) -> &[SavedSticker] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&SavedSticker> {
        self.entries.iter().find(|s| s.id == id)
    }

    fn file_path(&self, sticker: &SavedSticker) -> PathBuf {
        self.dir
            .join(format!("{}.{}", sticker.id, sticker.format.extension()))
    }

    /// Refuses a *new* sticker once the library is full. Saving over one
    /// that's already there (an updated image) is always fine.
    fn ensure_room_for(&self, id: &str) -> Result<(), AppError> {
        if self.get(id).is_none() && self.entries.len() >= MAX_SAVED {
            return Err(fail(format!(
                "You've saved the maximum of {MAX_SAVED} stickers — remove one first."
            )));
        }
        Ok(())
    }

    /// Writes an already-verified image and records it. The image goes down
    /// under a temporary name and is renamed into place, so a crash half way
    /// never leaves a truncated sticker that the index points at.
    pub fn save(
        &mut self,
        sticker: &CatalogSticker,
        bytes: &[u8],
    ) -> Result<SavedSticker, AppError> {
        self.ensure_room_for(&sticker.id)?;
        let saved = SavedSticker {
            id: sticker.id.clone(),
            name: sticker.name.clone(),
            category: sticker.category.clone(),
            format: sticker.format,
            size: bytes.len() as u64,
            sha256: sticker.sha256.clone(),
        };

        let io = |e: std::io::Error| fail(format!("couldn't save the sticker: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let target = self.file_path(&saved);
        let partial = target.with_extension(format!("{}.part", saved.format.extension()));
        std::fs::write(&partial, bytes).map_err(io)?;
        std::fs::rename(&partial, &target).map_err(io)?;

        // The image was replaced by one of another format (`x.svg` → `x.png`):
        // the old file would otherwise be left behind with nothing pointing
        // at it.
        if let Some(previous) = self.get(&saved.id) {
            if previous.format != saved.format {
                let _ = std::fs::remove_file(self.file_path(previous));
            }
        }

        self.entries.retain(|s| s.id != saved.id);
        self.entries.push(saved.clone());
        self.persist()?;
        Ok(saved)
    }

    /// Deletes a downloaded sticker. Removing one that isn't there is not an
    /// error: the aim — it's gone — is already met.
    pub fn remove(&mut self, id: &str) -> Result<(), AppError> {
        let Some(sticker) = self.get(id).cloned() else {
            return Ok(());
        };
        match std::fs::remove_file(self.file_path(&sticker)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(fail(format!("couldn't remove the sticker: {e}"))),
        }
        self.entries.retain(|s| s.id != id);
        self.persist()
    }

    fn persist(&self) -> Result<(), AppError> {
        let io = |e: std::io::Error| fail(format!("couldn't save your stickers: {e}"));
        let json = serde_json::to_string_pretty(&Index {
            stickers: self.entries.clone(),
        })
        .map_err(|e| fail(format!("couldn't save your stickers: {e}")))?;
        let partial = self.index_path.with_extension("json.part");
        if let Some(parent) = self.index_path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&partial, json).map_err(io)?;
        std::fs::rename(&partial, &self.index_path).map_err(io)
    }

    /// `data:image/svg+xml;base64,…` for one sticker, or `None` when the file
    /// can't be read.
    fn data_url(&self, sticker: &SavedSticker) -> Option<String> {
        let bytes = std::fs::read(self.file_path(sticker)).ok()?;
        if bytes.len() as u64 > MAX_STICKER_BYTES {
            return None;
        }
        Some(data_url_for(sticker.format, &bytes))
    }
}

/// An image as a `data:` URL — how the frontend draws a sticker without any
/// file-access permission.
fn data_url_for(format: Format, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", format.mime(), BASE64.encode(bytes))
}

fn global() -> &'static Mutex<Option<Library>> {
    static LIBRARY: Mutex<Option<Library>> = Mutex::new(None);
    &LIBRARY
}

/// Runs `action` on the app's one library, opening it on first use. A held
/// lock is never carried across an `await`.
fn with_library<T>(
    action: impl FnOnce(&mut Library) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut guard = global().lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(Library::open_default()?);
    }
    match guard.as_mut() {
        Some(library) => action(library),
        None => Err(fail("the sticker library isn't available")),
    }
}

/// Every downloaded sticker, with its image.
pub fn list() -> Result<Vec<SavedStickerView>, AppError> {
    with_library(|library| {
        Ok(library
            .entries()
            .iter()
            .filter_map(|sticker| {
                Some(SavedStickerView {
                    data_url: library.data_url(sticker)?,
                    sticker: sticker.clone(),
                })
            })
            .collect())
    })
}

pub fn remove(id: &str) -> Result<(), AppError> {
    with_library(|library| library.remove(id))
}

/// Downloads `id` from the catalog, verifies it, and saves it.
///
/// The checksum and size come from the cached catalog, not from the caller —
/// they're what the file is held to. Saving over an existing sticker is how an
/// updated image replaces the old one.
///
/// Returns the sticker with its image, so the frontend adds it to the palette
/// without re-reading every other download.
pub async fn download(id: &str) -> Result<SavedStickerView, AppError> {
    if !is_valid_sticker_id(id) {
        return Err(fail("That isn't a sticker id."));
    }
    let sticker = sticker_catalog::cached_sticker(id)?;
    // Before the download, not after it: no point fetching what can't be kept.
    with_library(|library| library.ensure_room_for(&sticker.id))?;

    let bytes = fetch_file(&api::base_url(), &sticker).await?;
    verify(&sticker, &bytes)?;
    let saved = with_library(|library| library.save(&sticker, &bytes))?;
    Ok(SavedStickerView {
        data_url: data_url_for(saved.format, &bytes),
        sticker: saved,
    })
}

/// Downloads a sticker's file, reading no more than the catalog said it has.
///
/// The address is built from the id. Not verified yet — see [`verify`].
async fn fetch_file(base_url: &str, sticker: &CatalogSticker) -> Result<Vec<u8>, AppError> {
    // What the catalog promised; `validate` already kept it within the cap.
    let limit = sticker.size.min(MAX_STICKER_BYTES);

    get_bounded(&file_url(base_url, &sticker.id), limit, DOWNLOAD_TIMEOUT)
        .await
        .map_err(|err| match err {
            GetError::Setup(reason) => fail(format!("Couldn't set up the download: {reason}")),
            GetError::Unreachable => fail("Couldn't reach the sticker server."),
            GetError::Status(status) => fail(format!("The sticker server answered {status}.")),
            GetError::TooBig => fail(
                "The download was bigger than the catalog said, so it wasn't saved. Refresh Browse and try again.",
            ),
            GetError::CutOff => fail("The download was cut off."),
        })
}

/// Whether `bytes` are the file `sticker` promised. Everything must hold; the
/// first thing that doesn't is the reason given.
fn verify(sticker: &CatalogSticker, bytes: &[u8]) -> Result<(), AppError> {
    if bytes.len() as u64 != sticker.size {
        return Err(fail(
            "The download wasn't the size the catalog listed, so it wasn't saved. Refresh Browse and try again.",
        ));
    }
    if hex::encode(Sha256::digest(bytes)) != sticker.sha256 {
        return Err(fail(
            "The download didn't match its checksum, so it wasn't saved.",
        ));
    }
    if sniff(bytes) != Some(sticker.format) {
        return Err(fail(format!(
            "The download isn't a {} image, so it wasn't saved.",
            sticker.format.extension().to_uppercase()
        )));
    }
    if sticker.format == Format::Svg {
        if let Some(markup) = forbidden_svg_markup(bytes) {
            return Err(fail(format!(
                "That SVG contains {markup}, which a sticker can't use, so it wasn't saved."
            )));
        }
    }
    Ok(())
}

/// What a file is, from its bytes. `None` for anything that isn't a PNG, a
/// WebP or an SVG.
pub(crate) fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(Format::Webp);
    }
    let lower = std::str::from_utf8(bytes).ok()?.to_ascii_lowercase();
    let head = lower.trim_start_matches('\u{feff}').trim_start();
    let opens_like_markup = ["<svg", "<?xml", "<!--", "<!doctype"]
        .iter()
        .any(|opening| head.starts_with(opening));
    (opens_like_markup && lower.contains("<svg")).then_some(Format::Svg)
}

/// Markup an SVG sticker has no business containing, found by name — the same
/// list the server refuses at upload (`App\Support\StickerFile`). In an
/// `<img>` none of it would run; this is for the day a sticker is shown some
/// other way.
pub(crate) fn forbidden_svg_markup(bytes: &[u8]) -> Option<&'static str> {
    let lower = std::str::from_utf8(bytes).ok()?.to_ascii_lowercase();
    const FORBIDDEN: [&str; 8] = [
        "<script",
        "<foreignobject",
        "<iframe",
        "<embed",
        "<object",
        "<!entity",
        "<!doctype",
        "javascript:",
    ];
    FORBIDDEN
        .into_iter()
        .find(|needle| lower.contains(needle))
        .or_else(|| has_event_handler(&lower).then_some("an event handler (onload, onclick…)"))
}

/// Whether the text has an attribute like ` onload=` — whitespace, `on`,
/// letters, then `=`.
fn has_event_handler(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    (0..bytes.len().saturating_sub(3)).any(|start| {
        if !bytes[start].is_ascii_whitespace() || &bytes[start + 1..start + 3] != b"on" {
            return false;
        }
        let name_end = bytes[start + 3..]
            .iter()
            .position(|b| !b.is_ascii_lowercase())
            .map(|offset| start + 3 + offset);
        let Some(end) = name_end.filter(|end| *end > start + 3) else {
            return false;
        };
        bytes[end..]
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|b| *b == b'=')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_http::{serve, Reply};

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#f9a8d4"/></svg>"##;
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 ";

    /// A scratch folder, removed when the test is done.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("rezure-stickers-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn library(&self) -> Library {
            Library::open(self.0.join("stickers"), self.0.join("stickers.json"))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entry(id: &str, format: Format, bytes: &[u8]) -> CatalogSticker {
        CatalogSticker {
            id: id.to_string(),
            name: "Pink bow".to_string(),
            category: "girls".to_string(),
            format,
            size: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(bytes)),
        }
    }

    fn svg(id: &str) -> (CatalogSticker, &'static [u8]) {
        (entry(id, Format::Svg, SVG.as_bytes()), SVG.as_bytes())
    }

    // ---- what a file is ----------------------------------------------------

    #[test]
    fn each_format_is_recognised_from_its_bytes() {
        assert_eq!(sniff(SVG.as_bytes()), Some(Format::Svg));
        assert_eq!(sniff(PNG), Some(Format::Png));
        assert_eq!(sniff(WEBP), Some(Format::Webp));
        assert_eq!(
            sniff(format!("\u{feff}<?xml version=\"1.0\"?>{SVG}").as_bytes()),
            Some(Format::Svg)
        );
        assert_eq!(sniff(b"GIF89a"), None);
        assert_eq!(sniff(b"MZ\x90\0"), None);
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"<html><svg/></html>"), None);
    }

    #[test]
    fn markup_a_sticker_cannot_use_is_named() {
        let wrap =
            |inner: &str| format!(r#"<svg xmlns="http://www.w3.org/2000/svg">{inner}</svg>"#);
        for (inner, expected) in [
            ("<script>x()</script>", "<script"),
            ("<SCRIPT>x()</SCRIPT>", "<script"),
            ("<foreignObject/>", "<foreignobject"),
            ("<a href=\"javascript:x()\"/>", "javascript:"),
        ] {
            assert_eq!(
                forbidden_svg_markup(wrap(inner).as_bytes()),
                Some(expected),
                "{inner}"
            );
        }
        assert!(forbidden_svg_markup(SVG.as_bytes()).is_none());
    }

    #[test]
    fn an_event_handler_attribute_is_found_but_ordinary_attributes_are_not() {
        assert!(has_event_handler(r#"<svg xmlns="x" onload="x()">"#));
        assert!(has_event_handler("<circle\nonclick = \"x()\"/>"));
        assert!(!has_event_handler(
            r#"<svg xmlns="x" viewBox="0 0 1 1"><g opacity="1"/></svg>"#
        ));
        assert!(!has_event_handler("<text> on = 5 </text>"));
        assert!(!has_event_handler("<text>ontology</text>"));
        assert!(!has_event_handler("on"));
        assert!(!has_event_handler(""));
    }

    // ---- what a download must be -------------------------------------------

    #[test]
    fn a_file_matching_the_catalog_passes() {
        let (sticker, bytes) = svg("pink-bow");
        assert!(verify(&sticker, bytes).is_ok());
        assert!(verify(&entry("dot", Format::Png, PNG), PNG).is_ok());
        assert!(verify(&entry("dot", Format::Webp, WEBP), WEBP).is_ok());
    }

    #[test]
    fn a_file_that_is_not_what_the_catalog_listed_is_refused_with_the_reason() {
        let (sticker, bytes) = svg("pink-bow");

        let mut longer = bytes.to_vec();
        longer.push(b' ');
        let reason = |sticker: &CatalogSticker, bytes: &[u8]| {
            verify(sticker, bytes).unwrap_err().to_string()
        };

        assert!(reason(&sticker, &longer).contains("size"));

        // Same length, different content.
        let mut tampered = bytes.to_vec();
        tampered[10] = b'X';
        assert!(reason(&sticker, &tampered).contains("checksum"));

        // The catalog says PNG; the checksum is right; the bytes are an SVG.
        let mislabelled = entry("pink-bow", Format::Png, bytes);
        assert!(reason(&mislabelled, bytes).contains("isn't a PNG"));

        // A script, honestly declared and honestly hashed — still refused.
        let hostile = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>x()</script></svg>"#;
        let hostile_entry = entry("hostile", Format::Svg, hostile);
        assert!(reason(&hostile_entry, hostile).contains("<script"));
    }

    // ---- against the real server ---------------------------------------------

    /// Talks to a running `laravel-api` instead of a canned reply, to prove the
    /// two sides agree on the contract: the catalog's shape, its `ETag`
    /// round-trip, and that every file the server lists downloads and passes
    /// [`verify`]. Run with a server up and
    /// `REZURE_TEST_API=http://127.0.0.1:8099 cargo test --lib against_the_real_laravel_api -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "needs a running laravel-api; set REZURE_TEST_API"]
    async fn against_the_real_laravel_api() {
        use super::sticker_catalog::{fetch_from, Fetched};

        let base = std::env::var("REZURE_TEST_API").expect("REZURE_TEST_API is set");

        let Fetched::Fresh { stickers, etag } = fetch_from(&base, None).await.unwrap() else {
            panic!("a first fetch can't be a 304");
        };
        println!("catalog: {} stickers, etag {etag:?}", stickers.len());
        assert!(!stickers.is_empty(), "seed the server with some stickers");

        // The server's own ETag, sent back, must be recognised as current.
        let etag = etag.expect("the catalog carries an ETag");
        assert!(matches!(
            fetch_from(&base, Some(&etag)).await.unwrap(),
            Fetched::NotModified
        ));

        for sticker in &stickers {
            let bytes = fetch_file(&base, sticker).await.unwrap();
            verify(sticker, &bytes).unwrap();
            println!(
                "  ok  {:<14} {:?} {} bytes",
                sticker.id,
                sticker.format,
                bytes.len()
            );
        }
    }

    // ---- fetching ----------------------------------------------------------

    #[tokio::test]
    async fn a_file_is_fetched_from_an_address_built_from_its_id() {
        let (sticker, bytes) = svg("pink-bow");
        let server = serve(vec![(
            "/api/v1/stickers/pink-bow/file",
            Reply::bytes(bytes),
        )]);

        let fetched = fetch_file(&server.base_url, &sticker).await.unwrap();
        assert_eq!(fetched, bytes);
        assert_eq!(server.requests().len(), 1);
    }

    #[tokio::test]
    async fn a_failed_request_is_a_sentence_not_a_saved_sticker() {
        let (sticker, _) = svg("pink-bow");

        let missing = serve(vec![]);
        let err = fetch_file(&missing.base_url, &sticker).await.unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");

        let down = fetch_file("http://127.0.0.1:1", &sticker)
            .await
            .unwrap_err();
        assert!(down.to_string().contains("Couldn't reach"), "{down}");
    }

    #[tokio::test]
    async fn more_than_the_catalog_promised_is_never_read_in_full() {
        let (sticker, bytes) = svg("pink-bow");
        let mut oversized = bytes.to_vec();
        oversized.extend_from_slice(&[b' '; 64]);

        // With a Content-Length, it's refused before reading the body…
        let declared = serve(vec![(
            "/api/v1/stickers/pink-bow/file",
            Reply::bytes(&oversized),
        )]);
        let err = fetch_file(&declared.base_url, &sticker).await.unwrap_err();
        assert!(err.to_string().contains("bigger"), "{err}");

        // …and without one, the bytes are counted as they arrive.
        let streamed = serve(vec![(
            "/api/v1/stickers/pink-bow/file",
            Reply::bytes(&oversized).without_length(),
        )]);
        let err = fetch_file(&streamed.base_url, &sticker).await.unwrap_err();
        assert!(err.to_string().contains("bigger"), "{err}");
    }

    // ---- keeping them ------------------------------------------------------

    #[test]
    fn a_saved_sticker_survives_a_restart() {
        let scratch = Scratch::new();
        let (sticker, bytes) = svg("pink-bow");

        let saved = scratch.library().save(&sticker, bytes).unwrap();
        assert_eq!(saved.id, "pink-bow");
        assert!(scratch.0.join("stickers").join("pink-bow.svg").is_file());

        let reopened = scratch.library();
        assert_eq!(reopened.entries(), &[saved]);
        let url = reopened
            .data_url(reopened.get("pink-bow").unwrap())
            .unwrap();
        assert!(url.starts_with("data:image/svg+xml;base64,"), "{url}");
        assert!(BASE64
            .decode(url.rsplit(',').next().unwrap())
            .is_ok_and(|b| b == bytes));
    }

    #[test]
    fn saving_the_same_sticker_again_replaces_it_and_drops_a_file_of_another_format() {
        let scratch = Scratch::new();
        let mut library = scratch.library();
        let (svg_entry, svg_bytes) = svg("pink-bow");
        library.save(&svg_entry, svg_bytes).unwrap();

        library
            .save(&entry("pink-bow", Format::Png, PNG), PNG)
            .unwrap();

        assert_eq!(library.entries().len(), 1);
        assert_eq!(library.get("pink-bow").unwrap().format, Format::Png);
        assert!(!scratch.0.join("stickers").join("pink-bow.svg").exists());
        assert!(scratch.0.join("stickers").join("pink-bow.png").is_file());
    }

    #[test]
    fn removing_deletes_the_image_and_the_entry_and_is_safe_to_repeat() {
        let scratch = Scratch::new();
        let mut library = scratch.library();
        let (sticker, bytes) = svg("pink-bow");
        library.save(&sticker, bytes).unwrap();

        library.remove("pink-bow").unwrap();
        library.remove("pink-bow").unwrap();
        library.remove("never-saved").unwrap();

        assert!(library.entries().is_empty());
        assert!(!scratch.0.join("stickers").join("pink-bow.svg").exists());
        assert!(scratch.library().entries().is_empty());
    }

    #[test]
    fn the_library_has_a_limit_but_an_update_always_fits() {
        let scratch = Scratch::new();
        let mut library = scratch.library();
        for n in 0..MAX_SAVED {
            let (sticker, bytes) = svg(&format!("s{n}"));
            library.save(&sticker, bytes).unwrap();
        }

        let (extra, bytes) = svg("one-too-many");
        let err = library.save(&extra, bytes).unwrap_err();
        assert!(err.to_string().contains("maximum"), "{err}");
        assert!(library.get("one-too-many").is_none());
        assert!(!scratch.0.join("stickers").join("one-too-many.svg").exists());

        let (update, bytes) = svg("s7");
        assert!(library.save(&update, bytes).is_ok());
    }

    #[test]
    fn opening_drops_what_is_gone_or_not_a_valid_id_and_survives_a_broken_index() {
        let scratch = Scratch::new();
        let mut library = scratch.library();
        let (kept, bytes) = svg("kept");
        let (lost, bytes_lost) = svg("lost");
        library.save(&kept, bytes).unwrap();
        library.save(&lost, bytes_lost).unwrap();
        std::fs::remove_file(scratch.0.join("stickers").join("lost.svg")).unwrap();

        // A hand-edited index naming something outside the folder.
        let mut index = std::fs::read_to_string(scratch.0.join("stickers.json")).unwrap();
        index = index.replace(
            "\"stickers\": [",
            r#""stickers": [{"id":"..\\evil","name":"x","category":"x","format":"svg","size":1,"sha256":"x"},"#,
        );
        std::fs::write(scratch.0.join("stickers.json"), index).unwrap();

        let reopened = scratch.library();
        assert_eq!(
            reopened
                .entries()
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["kept"]
        );

        std::fs::write(scratch.0.join("stickers.json"), "not json at all").unwrap();
        assert!(scratch.library().entries().is_empty());
    }

    // ---- the whole path ----------------------------------------------------

    #[tokio::test]
    async fn a_tampered_download_is_never_saved() {
        let scratch = Scratch::new();
        let (sticker, bytes) = svg("pink-bow");
        let mut tampered = bytes.to_vec();
        tampered[10] = b'X';
        let server = serve(vec![(
            "/api/v1/stickers/pink-bow/file",
            Reply::bytes(&tampered),
        )]);

        let fetched = fetch_file(&server.base_url, &sticker).await.unwrap();
        assert!(verify(&sticker, &fetched).is_err());
        assert!(scratch.library().entries().is_empty());
    }

    #[tokio::test]
    async fn a_good_download_is_fetched_verified_and_saved() {
        let scratch = Scratch::new();
        let (sticker, bytes) = svg("pink-bow");
        let server = serve(vec![(
            "/api/v1/stickers/pink-bow/file",
            Reply::bytes(bytes),
        )]);

        let fetched = fetch_file(&server.base_url, &sticker).await.unwrap();
        verify(&sticker, &fetched).unwrap();
        let saved = scratch.library().save(&sticker, &fetched).unwrap();

        assert_eq!(saved.sha256, sticker.sha256);
        assert_eq!(scratch.library().entries(), &[saved]);
    }
}
