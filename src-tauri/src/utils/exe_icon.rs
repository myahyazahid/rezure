//! The icon an installed program shows in Explorer, as a `data:` URL the
//! frontend can put straight into an `<img>`.
//!
//! Used to put each SQL client's own logo next to its name in the "Open
//! with" menu. Read out of the `.exe` rather than bundled: it's always the
//! logo of the version actually installed, any client added to
//! `services::db_clients` gets one for free, and the repo carries no other
//! products' artwork.
//!
//! The image is written as a one-image `.ico` holding a 32-bit DIB — the
//! format the pixels come out of Windows in already, so no PNG encoder is
//! needed, and one WebView2 renders with its alpha channel intact.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

/// Pixels per side asked for: the menu shows it at 20 CSS pixels, so this
/// stays sharp up to 240% display scaling.
const SIZE: u32 = 48;

/// `exe`'s icon as a `data:image/x-icon` URL, or `None` when it has none or
/// Windows won't hand it over — the caller shows a fallback mark instead.
///
/// Cached per path for the life of the process: the menu asks every time
/// it's opened, and an installed program's icon doesn't change under it.
pub fn data_url(exe: &Path) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);

    if let Some(cached) = cache.lock().ok().and_then(|c| c.get(exe).cloned()) {
        return cached;
    }
    let url = extract(exe).map(|(width, height, pixels)| {
        format!(
            "data:image/x-icon;base64,{}",
            BASE64.encode(ico(width, height, &pixels))
        )
    });
    if let Ok(mut cache) = cache.lock() {
        cache.insert(exe.to_path_buf(), url.clone());
    }
    url
}

/// The first icon in `exe`, as its width, height and bottom-up BGRA rows.
fn extract(exe: &Path) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::Graphics::Gdi::DeleteObject;
    use windows_sys::Win32::UI::Shell::SHDefExtractIconW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    let path: Vec<u16> = exe
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `path` is a NUL-terminated UTF-16 string that outlives the
    // call. The icon and the two bitmaps `GetIconInfo` creates are owned
    // here, and each is released exactly once below whichever way this
    // returns.
    unsafe {
        let mut icon = std::ptr::null_mut();
        let found = SHDefExtractIconW(path.as_ptr(), 0, 0, &mut icon, std::ptr::null_mut(), SIZE);
        // S_FALSE (1) means "no icon in this file".
        if found != 0 || icon.is_null() {
            return None;
        }

        let mut info: ICONINFO = std::mem::zeroed();
        let pixels = if GetIconInfo(icon, &mut info) != 0 {
            let pixels = icon_pixels(info.hbmColor, info.hbmMask);
            if !info.hbmColor.is_null() {
                DeleteObject(info.hbmColor);
            }
            if !info.hbmMask.is_null() {
                DeleteObject(info.hbmMask);
            }
            pixels
        } else {
            None
        };
        DestroyIcon(icon);
        pixels
    }
}

/// Reads a colour icon's pixels. Icons from before alpha channels carry
/// transparency only in `mask` — every alpha byte reads 0 — so for those
/// the mask is turned into alpha, or the whole icon would be invisible.
///
/// # Safety
/// `color` and `mask` must be bitmaps from one `GetIconInfo` call (`color`
/// may be null, for a monochrome icon, which yields `None`).
unsafe fn icon_pixels(
    color: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    mask: windows_sys::Win32::Graphics::Gdi::HBITMAP,
) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::Graphics::Gdi::{GetObjectW, BITMAP};

    if color.is_null() {
        return None;
    }
    let mut bitmap: BITMAP = std::mem::zeroed();
    let size = std::mem::size_of::<BITMAP>() as i32;
    if GetObjectW(color, size, (&mut bitmap as *mut BITMAP).cast()) == 0 {
        return None;
    }
    let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
    if width <= 0 || height <= 0 || width > 256 || height > 256 {
        return None;
    }
    let (width, height) = (width as u32, height as u32);

    let mut pixels = bgra_rows(color, width, height)?;
    if pixels.chunks_exact(4).all(|pixel| pixel[3] == 0) {
        let mask = bgra_rows(mask, width, height)?;
        for (pixel, masked) in pixels.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
            // A black mask pixel is drawn; a white one is see-through.
            pixel[3] = if masked[0] == 0 { 255 } else { 0 };
        }
    }
    Some((width, height, pixels))
}

/// `bitmap` converted to 32-bit BGRA, bottom row first — the order an
/// `.ico` stores its rows in.
///
/// # Safety
/// `bitmap` must be a valid bitmap handle of at least `width` × `height`.
unsafe fn bgra_rows(
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::{
        GetDC, GetDIBits, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };

    let mut header: BITMAPINFO = std::mem::zeroed();
    header.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        // Positive: bottom-up.
        biHeight: height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..std::mem::zeroed()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];

    let screen = GetDC(std::ptr::null_mut());
    if screen.is_null() {
        return None;
    }
    let lines = GetDIBits(
        screen,
        bitmap,
        0,
        height,
        pixels.as_mut_ptr().cast(),
        &mut header,
        DIB_RGB_COLORS,
    );
    ReleaseDC(std::ptr::null_mut(), screen);
    (lines == height as i32).then_some(pixels)
}

/// A one-image `.ico`: the directory, then a 32-bit DIB of `rows` (bottom-up
/// BGRA) followed by an all-zero AND mask — the alpha channel already says
/// what's transparent.
fn ico(width: u32, height: u32, rows: &[u8]) -> Vec<u8> {
    // Each mask row is one bit per pixel, padded to a 4-byte boundary.
    let mask_len = width.div_ceil(32) * 4 * height;
    let dib_len = 40 + rows.len() as u32 + mask_len;
    // 256 is written as 0 in the one-byte directory fields.
    let side = |n: u32| if n >= 256 { 0 } else { n as u8 };

    let mut out = Vec::with_capacity(22 + dib_len as usize);
    // ICONDIR: reserved, type 1 (icon), one image.
    out.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // ICONDIRENTRY.
    out.extend_from_slice(&[side(width), side(height), 0, 0]);
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&dib_len.to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes()); // offset of the DIB
                                                 // BITMAPINFOHEADER — twice the height, counting the mask.
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32 * 2).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(rows.len() as u32 + mask_len).to_le_bytes());
    out.extend_from_slice(&[0; 16]); // resolution and palette: unused
    out.extend_from_slice(rows);
    out.resize(out.len() + mask_len as usize, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ico_is_its_directory_then_a_dib_of_double_height() {
        let rows = vec![0xAB; 2 * 2 * 4];
        let ico = ico(2, 2, &rows);
        // 6 + 16 directory, 40 header, 16 pixels, 2 mask rows of 4 bytes.
        assert_eq!(ico.len(), 22 + 40 + 16 + 8);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(u32::from_le_bytes(ico[14..18].try_into().unwrap()), 64);
        assert_eq!(i32::from_le_bytes(ico[30..34].try_into().unwrap()), 4);
        assert_eq!(&ico[62..78], rows.as_slice());
    }

    #[test]
    fn a_256_pixel_side_is_written_as_zero() {
        let ico = ico(256, 256, &vec![0; 256 * 256 * 4]);
        assert_eq!(&ico[6..8], &[0, 0]);
    }

    /// Every Windows install has Notepad, and Notepad has an icon.
    #[test]
    fn notepad_has_an_icon() {
        let notepad = Path::new(r"C:\Windows\System32\notepad.exe");
        let url = data_url(notepad).expect("notepad.exe has an icon");
        assert!(url.starts_with("data:image/x-icon;base64,"));
        assert!(data_url(Path::new(r"C:\no\such\program.exe")).is_none());
    }
}
