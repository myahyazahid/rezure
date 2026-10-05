//! The pictures on the Donate page — the QRIS, and the logo on a link or a
//! wallet — and the rules every one of them is held to.
//!
//! The server describes each picture as `{ format, size, sha256 }` inside the
//! donate config (so `Last-Modified` covers it); the picture itself is a
//! separate download, kept on disk:
//!
//! * it is fetched only when the saved file isn't the one described, and the
//!   checksum decides that — never the file's age;
//! * the address is built by the caller, never taken from a `url` in the
//!   response;
//! * at most the announced size is read, and the bytes must hash to the
//!   announced checksum **and** be the announced format before they're kept
//!   (an SVG must also be free of scripts and the like);
//! * a picture is passed on exactly as it came. A QRIS has to stay scannable,
//!   so nothing here re-encodes, crops or resizes it.
//!
//! # When the endpoint is down
//!
//! The page must keep what it was last given. A saved picture is "current"
//! while it is the one the (fresh or `304`-confirmed) config describes; when
//! the current one can't be got — offline, the endpoint dead, a download that
//! doesn't verify — the *earlier* one is still shown, flagged `stale`. An
//! outdated picture the page can warn about beats none; one that failed
//! verification is never shown or kept at all.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::sticker_catalog::Format;
use super::sticker_library::{forbidden_svg_markup, sniff};
use crate::utils::bounded_get::get_bounded;

/// Largest QRIS image the server accepts: 1 MB.
pub const MAX_QRIS_BYTES: u64 = 1024 * 1024;

/// Largest link or wallet logo the server accepts: 256 KB.
pub const MAX_ICON_BYTES: u64 = 256 * 1024;

const TIMEOUT: Duration = Duration::from_secs(15);

/// What a picture file is. A QRIS is never an SVG — which of these a picture
/// may be is decided by its caller, per slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpg,
    Webp,
    Svg,
}

impl ImageFormat {
    const ALL: [Self; 4] = [Self::Png, Self::Jpg, Self::Webp, Self::Svg];

    /// For a QRIS: photographs and screenshots only.
    pub const QRIS: [Self; 3] = [Self::Png, Self::Jpg, Self::Webp];

    /// For a link or wallet logo: what a brand mark comes as. JPEG is in: a
    /// coin logo saved from a web page very often is one, and the server's
    /// `IconFile` takes it.
    pub const ICON: [Self; 4] = [Self::Svg, Self::Png, Self::Jpg, Self::Webp];

    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "png" => Some(Self::Png),
            "jpg" => Some(Self::Jpg),
            "webp" => Some(Self::Webp),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Svg => "image/svg+xml",
        }
    }

    /// Whether `bytes` are, by their content, a file of this format.
    fn matches(self, bytes: &[u8]) -> bool {
        match self {
            Self::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpg => bytes.starts_with(b"\xFF\xD8\xFF"),
            Self::Webp => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
            Self::Svg => sniff(bytes) == Some(Format::Svg) && forbidden_svg_markup(bytes).is_none(),
        }
    }
}

/// The server's description of one picture, after validation — what a
/// download is held to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageDescriptor {
    pub format: ImageFormat,
    pub size: u64,
    /// Lowercase hex SHA-256.
    pub sha256: String,
}

/// Mirrors the wire shape of a picture description. A `url` beside it, if
/// there is one, is not read here.
#[derive(Debug, Deserialize)]
pub struct ApiImage {
    format: String,
    size: u64,
    sha256: String,
}

impl ApiImage {
    /// `None` for a description that can't be trusted to hold a download to:
    /// a format the slot doesn't take, a size out of range, a checksum that
    /// isn't one.
    pub fn validated(self, allowed: &[ImageFormat], max_bytes: u64) -> Option<ImageDescriptor> {
        let sha256 = self.sha256.to_ascii_lowercase();
        let acceptable = (1..=max_bytes).contains(&self.size)
            && sha256.len() == 64
            && sha256.bytes().all(|b| b.is_ascii_hexdigit());
        let format = ImageFormat::from_wire(&self.format).filter(|f| allowed.contains(f))?;
        acceptable.then_some(ImageDescriptor {
            format,
            size: self.size,
            sha256,
        })
    }
}

/// A picture ready to show: the file, exactly as the server sent it, as a
/// `data:` URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataImage {
    pub format: ImageFormat,
    pub sha256: String,
    pub data_url: String,
}

/// What became of one picture.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImageOutcome {
    pub image: Option<DataImage>,
    /// There is a picture on the server but none to show at all — never
    /// downloaded, and the download failed or didn't verify.
    pub unavailable: bool,
    /// The picture shown is an earlier one standing in for the current one.
    pub stale: bool,
}

impl ImageOutcome {
    fn image(format: ImageFormat, bytes: &[u8], stale: bool) -> Self {
        Self {
            image: Some(DataImage {
                format,
                sha256: sha256_hex(bytes),
                data_url: format!("data:{};base64,{}", format.mime(), BASE64.encode(bytes)),
            }),
            unavailable: false,
            stale,
        }
    }

    fn current(format: ImageFormat, bytes: &[u8]) -> Self {
        Self::image(format, bytes, false)
    }

    fn previous(format: ImageFormat, bytes: &[u8]) -> Self {
        Self::image(format, bytes, true)
    }

    pub fn unavailable() -> Self {
        Self {
            image: None,
            unavailable: true,
            stale: false,
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Where one picture is kept: `<dir>/<stem>.<ext>`, one file at most.
pub struct Slot {
    dir: PathBuf,
    stem: String,
    /// Nothing bigger than this is read back or kept.
    max_bytes: u64,
}

impl Slot {
    pub fn new(dir: &Path, stem: impl Into<String>, max_bytes: u64) -> Self {
        Self {
            dir: dir.to_path_buf(),
            stem: stem.into(),
            max_bytes,
        }
    }

    pub fn file(&self, format: ImageFormat) -> PathBuf {
        self.dir
            .join(format!("{}.{}", self.stem, format.extension()))
    }

    /// The saved picture, if — and only if — it is the one `descriptor`
    /// describes. Its checksum is recomputed rather than remembered: a file
    /// edited or truncated on disk is simply not the image any more.
    pub fn saved(&self, descriptor: &ImageDescriptor) -> Option<Vec<u8>> {
        let bytes = std::fs::read(self.file(descriptor.format)).ok()?;
        (bytes.len() as u64 <= self.max_bytes && sha256_hex(&bytes) == descriptor.sha256)
            .then_some(bytes)
    }

    /// Whatever picture is saved, whether or not it is the one currently
    /// described — for when the current one can't be got. It still has to be
    /// the format its file name says, so a stray file isn't shown.
    pub fn previous(&self) -> Option<(ImageFormat, Vec<u8>)> {
        ImageFormat::ALL.into_iter().find_map(|format| {
            let bytes = std::fs::read(self.file(format)).ok()?;
            (bytes.len() as u64 <= self.max_bytes && format.matches(&bytes))
                .then_some((format, bytes))
        })
    }

    /// Keeps `bytes`, through a temporary name so a crash never leaves a
    /// truncated picture, and drops a leftover of another format.
    pub fn save(&self, descriptor: &ImageDescriptor, bytes: &[u8]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let target = self.file(descriptor.format);
        let partial = target.with_extension(format!("{}.part", descriptor.format.extension()));
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, &target)?;
        for other in ImageFormat::ALL {
            if other != descriptor.format {
                let _ = std::fs::remove_file(self.file(other));
            }
        }
        Ok(())
    }

    /// Removes the saved picture, whatever its format.
    pub fn clear(&self) {
        for format in ImageFormat::ALL {
            let _ = std::fs::remove_file(self.file(format));
        }
    }
}

/// Whether `bytes` are the picture `descriptor` announced. The first thing
/// that isn't true is the reason, for the log.
fn verify(descriptor: &ImageDescriptor, bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() as u64 != descriptor.size {
        return Err("it isn't the size announced");
    }
    if sha256_hex(bytes) != descriptor.sha256 {
        return Err("it doesn't match the announced checksum");
    }
    if !descriptor.format.matches(bytes) {
        return Err("it isn't the announced image format (or an SVG with markup it can't have)");
    }
    Ok(())
}

/// Works out one picture for the page.
///
/// * No picture described: nothing to show. When the server really said so
///   (not `offline`, where "none" may just be a missing cache), a saved copy
///   is deleted, so a removed picture doesn't linger.
/// * The saved file is the described one: show it, no request.
/// * Otherwise download it from `url` — unless `offline` — and keep it only
///   if [`verify`] passes.
/// * If the described one can't be got, show the earlier one, marked stale;
///   with nothing saved at all, the outcome is "unavailable".
pub async fn resolve(
    slot: &Slot,
    url: &str,
    descriptor: Option<&ImageDescriptor>,
    offline: bool,
) -> ImageOutcome {
    let Some(descriptor) = descriptor else {
        if !offline {
            slot.clear();
        }
        return ImageOutcome::default();
    };

    if let Some(bytes) = slot.saved(descriptor) {
        return ImageOutcome::current(descriptor.format, &bytes);
    }

    if !offline {
        match get_bounded(url, descriptor.size, TIMEOUT).await {
            Ok(bytes) => match verify(descriptor, &bytes) {
                Ok(()) => {
                    // Verified, so it's shown even if it can't be kept.
                    if let Err(err) = slot.save(descriptor, &bytes) {
                        log::warn!("could not save a donate picture: {err}");
                    }
                    return ImageOutcome::current(descriptor.format, &bytes);
                }
                Err(reason) => log::warn!("refusing a downloaded donate picture: {reason}"),
            },
            Err(err) => log::warn!("could not download a donate picture: {err:?}"),
        }
    }

    match slot.previous() {
        Some((format, bytes)) => ImageOutcome::previous(format, &bytes),
        None => ImageOutcome::unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_http::{serve, Reply};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR some pixels";
    const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0\0\x10JFIF\0 some pixels";
    const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 some pixels";
    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>"##;
    const PATH: &str = "/picture";

    /// A scratch folder, removed when the test is done.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("rezure-donate-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn slot(&self) -> Slot {
            Slot::new(&self.0, "pic", MAX_QRIS_BYTES)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn descriptor(format: ImageFormat, bytes: &[u8]) -> ImageDescriptor {
        ImageDescriptor {
            format,
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        }
    }

    fn url(server: &crate::utils::test_http::TestServer) -> String {
        format!("{}{PATH}", server.base_url)
    }

    // ---- reading a description ---------------------------------------------

    fn api(format: &str, size: u64, sha256: &str) -> ApiImage {
        ApiImage {
            format: format.to_string(),
            size,
            sha256: sha256.to_string(),
        }
    }

    #[test]
    fn a_description_is_validated_against_what_the_slot_takes() {
        let sha = "a".repeat(64);

        let qris = api("jpg", 159_658, &sha.to_ascii_uppercase())
            .validated(&ImageFormat::QRIS, MAX_QRIS_BYTES);
        assert_eq!(
            qris,
            Some(ImageDescriptor {
                format: ImageFormat::Jpg,
                size: 159_658,
                sha256: sha.clone(),
            })
        );

        // A QRIS is never an SVG; a logo may be any of the four.
        assert_eq!(
            api("svg", 10, &sha).validated(&ImageFormat::QRIS, MAX_QRIS_BYTES),
            None
        );
        for format in ["svg", "png", "jpg", "webp"] {
            assert!(
                api(format, 10, &sha)
                    .validated(&ImageFormat::ICON, MAX_ICON_BYTES)
                    .is_some(),
                "{format}"
            );
        }
        assert_eq!(
            api("gif", 10, &sha).validated(&ImageFormat::ICON, MAX_ICON_BYTES),
            None
        );
    }

    #[test]
    fn a_description_that_cannot_hold_a_download_to_anything_is_refused() {
        let sha = "b".repeat(64);
        let icon = |d: ApiImage| d.validated(&ImageFormat::ICON, MAX_ICON_BYTES);

        assert_eq!(icon(api("gif", 10, &sha)), None);
        assert_eq!(icon(api("png", 0, &sha)), None);
        assert_eq!(icon(api("png", MAX_ICON_BYTES + 1, &sha)), None);
        assert_eq!(icon(api("png", 10, "short")), None);
        assert_eq!(icon(api("png", 10, &"z".repeat(64))), None);
        assert!(icon(api("png", MAX_ICON_BYTES, &sha)).is_some());
    }

    // ---- getting a picture -------------------------------------------------

    #[tokio::test]
    async fn a_new_picture_is_downloaded_verified_and_kept() {
        let scratch = Scratch::new();
        let d = descriptor(ImageFormat::Png, PNG);
        let server = serve(vec![(PATH, Reply::bytes(PNG))]);

        let outcome = resolve(&scratch.slot(), &url(&server), Some(&d), false).await;

        let image = outcome.image.expect("the picture is shown");
        assert!(!outcome.unavailable && !outcome.stale);
        assert_eq!(image.sha256, d.sha256);
        let (head, encoded) = image.data_url.split_once(',').unwrap();
        assert_eq!(head, "data:image/png;base64");
        assert_eq!(BASE64.decode(encoded).unwrap(), PNG, "passed on untouched");
        assert_eq!(std::fs::read(scratch.file("pic.png")).unwrap(), PNG);
    }

    #[tokio::test]
    async fn every_format_a_slot_takes_goes_through() {
        for (format, bytes) in [
            (ImageFormat::Png, PNG),
            (ImageFormat::Jpg, JPEG),
            (ImageFormat::Webp, WEBP),
            (ImageFormat::Svg, SVG),
        ] {
            let scratch = Scratch::new();
            let d = descriptor(format, bytes);
            let server = serve(vec![(PATH, Reply::bytes(bytes))]);

            let outcome = resolve(&scratch.slot(), &url(&server), Some(&d), false).await;

            assert!(outcome.image.is_some(), "{format:?}");
        }
    }

    #[tokio::test]
    async fn a_saved_picture_is_shown_without_asking_the_server_again() {
        let scratch = Scratch::new();
        let d = descriptor(ImageFormat::Jpg, JPEG);
        let server = serve(vec![(PATH, Reply::bytes(JPEG))]);

        resolve(&scratch.slot(), &url(&server), Some(&d), false).await;
        let again = resolve(&scratch.slot(), &url(&server), Some(&d), false).await;

        assert!(again.image.is_some() && !again.stale);
        assert_eq!(server.requests().len(), 1, "the checksum says it's current");
    }

    #[tokio::test]
    async fn a_replaced_picture_is_downloaded_again_and_the_old_file_goes() {
        let scratch = Scratch::new();
        let server = serve(vec![(PATH, Reply::bytes(WEBP))]);
        scratch
            .slot()
            .save(&descriptor(ImageFormat::Png, PNG), PNG)
            .unwrap();

        let new = descriptor(ImageFormat::Webp, WEBP);
        let outcome = resolve(&scratch.slot(), &url(&server), Some(&new), false).await;

        assert_eq!(outcome.image.unwrap().sha256, new.sha256);
        assert!(!outcome.stale);
        assert!(scratch.file("pic.webp").is_file());
        assert!(!scratch.file("pic.png").exists(), "no stale copy left");
    }

    /// A download that isn't the announced picture is never shown or kept: a
    /// QRIS like that could send a donation anywhere.
    #[tokio::test]
    async fn a_picture_that_is_not_what_was_announced_is_never_shown_or_kept() {
        let mut tampered = PNG.to_vec();
        tampered[12] ^= 0xFF;
        let mut longer = PNG.to_vec();
        longer.push(0);
        let scripted = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>x()</script></svg>"#;
        let cases: Vec<(&str, ImageDescriptor, Vec<u8>)> = vec![
            (
                "different bytes",
                descriptor(ImageFormat::Png, PNG),
                tampered,
            ),
            (
                "not the announced size",
                descriptor(ImageFormat::Png, PNG),
                longer,
            ),
            (
                "announced as a PNG, but a JPEG with a matching checksum",
                descriptor(ImageFormat::Png, JPEG),
                JPEG.to_vec(),
            ),
            (
                "an SVG with a script, honestly declared and hashed",
                descriptor(ImageFormat::Svg, scripted),
                scripted.to_vec(),
            ),
        ];

        for (label, d, served) in cases {
            let scratch = Scratch::new();
            let server = serve(vec![(PATH, Reply::bytes(&served))]);

            let outcome = resolve(&scratch.slot(), &url(&server), Some(&d), false).await;

            assert_eq!(outcome, ImageOutcome::unavailable(), "{label}");
            assert!(!scratch.file("pic.png").exists(), "{label}");
            assert!(!scratch.file("pic.svg").exists(), "{label}");
        }
    }

    #[tokio::test]
    async fn more_than_the_announced_size_is_not_read() {
        let scratch = Scratch::new();
        let d = descriptor(ImageFormat::Png, PNG);
        let mut huge = PNG.to_vec();
        huge.extend_from_slice(&[0; 4096]);
        let server = serve(vec![(PATH, Reply::bytes(&huge).without_length())]);

        let outcome = resolve(&scratch.slot(), &url(&server), Some(&d), false).await;

        assert_eq!(outcome, ImageOutcome::unavailable());
    }

    // ---- when the endpoint is down -----------------------------------------

    /// The point of keeping them: the endpoint can be dead and the page still
    /// shows what it was last given.
    #[tokio::test]
    async fn offline_the_saved_picture_is_still_shown() {
        let scratch = Scratch::new();
        let d = descriptor(ImageFormat::Png, PNG);
        scratch.slot().save(&d, PNG).unwrap();
        let server = serve(vec![]);

        let outcome = resolve(&scratch.slot(), &url(&server), Some(&d), true).await;

        assert!(outcome.image.is_some());
        assert!(!outcome.stale, "it is the one the cached config describes");
        assert!(server.requests().is_empty(), "offline never asks");
    }

    #[tokio::test]
    async fn offline_with_nothing_saved_the_picture_is_unavailable() {
        let scratch = Scratch::new();
        let d = descriptor(ImageFormat::Png, PNG);

        let outcome = resolve(&scratch.slot(), "http://127.0.0.1:1/x", Some(&d), true).await;

        assert_eq!(outcome, ImageOutcome::unavailable());
    }

    /// The endpoint answers the config but not the picture: a dead image
    /// route must not wipe out the picture people already have.
    #[tokio::test]
    async fn a_failed_download_keeps_the_previous_picture_and_flags_it() {
        let scratch = Scratch::new();
        let old = descriptor(ImageFormat::Png, PNG);
        scratch.slot().save(&old, PNG).unwrap();
        let server = serve(vec![(PATH, Reply::status(500))]);

        // The server now describes a different image that can't be fetched.
        let new = descriptor(ImageFormat::Jpg, JPEG);
        let outcome = resolve(&scratch.slot(), &url(&server), Some(&new), false).await;

        let image = outcome.image.expect("the earlier picture stays");
        assert!(outcome.stale && !outcome.unavailable);
        assert_eq!(image.sha256, old.sha256, "it is the old one, not the new");
        assert_eq!(image.format, ImageFormat::Png);
    }

    #[tokio::test]
    async fn a_download_that_fails_verification_does_not_replace_the_previous_picture() {
        let scratch = Scratch::new();
        let old = descriptor(ImageFormat::Png, PNG);
        scratch.slot().save(&old, PNG).unwrap();
        // The server announces one image and serves another.
        let new = descriptor(ImageFormat::Jpg, JPEG);
        let server = serve(vec![(PATH, Reply::bytes(b"\xFF\xD8\xFFsomething else"))]);

        let outcome = resolve(&scratch.slot(), &url(&server), Some(&new), false).await;

        assert!(outcome.stale);
        assert_eq!(outcome.image.unwrap().sha256, old.sha256);
        assert_eq!(std::fs::read(scratch.file("pic.png")).unwrap(), PNG);
        assert!(!scratch.file("pic.jpg").exists());
    }

    #[tokio::test]
    async fn offline_with_an_outdated_copy_saved_it_is_shown_as_possibly_outdated() {
        let scratch = Scratch::new();
        scratch
            .slot()
            .save(&descriptor(ImageFormat::Png, PNG), PNG)
            .unwrap();
        // The cached config describes a newer image that was never downloaded.
        let newer = descriptor(ImageFormat::Png, b"\x89PNG\r\n\x1a\nthe newer one");

        let outcome = resolve(&scratch.slot(), "http://127.0.0.1:1/x", Some(&newer), true).await;

        assert!(outcome.image.is_some() && outcome.stale);
    }

    #[test]
    fn a_stray_file_is_not_mistaken_for_a_saved_picture() {
        let scratch = Scratch::new();
        std::fs::write(scratch.file("pic.png"), b"not an image").unwrap();

        assert!(scratch.slot().previous().is_none());
    }

    // ---- removal and tampering ---------------------------------------------

    #[tokio::test]
    async fn a_picture_the_server_removed_is_deleted_but_a_missing_cache_is_not_taken_for_that() {
        let scratch = Scratch::new();
        let d = descriptor(ImageFormat::Png, PNG);
        scratch.slot().save(&d, PNG).unwrap();

        // Offline, "no picture" may only mean the config cache is gone.
        let offline = resolve(&scratch.slot(), "http://127.0.0.1:1/x", None, true).await;
        assert_eq!(offline, ImageOutcome::default());
        assert!(scratch.file("pic.png").is_file());

        // Online, it's the server's answer.
        let online = resolve(&scratch.slot(), "http://127.0.0.1:1/x", None, false).await;
        assert_eq!(online, ImageOutcome::default());
        assert!(!scratch.file("pic.png").exists());
    }

    #[test]
    fn a_file_edited_on_disk_is_no_longer_the_current_picture() {
        let scratch = Scratch::new();
        let d = descriptor(ImageFormat::Png, PNG);
        scratch.slot().save(&d, PNG).unwrap();
        assert!(scratch.slot().saved(&d).is_some());

        std::fs::write(scratch.file("pic.png"), b"\x89PNG\r\n\x1a\nswapped").unwrap();
        assert!(scratch.slot().saved(&d).is_none());
    }
}
