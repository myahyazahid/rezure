//! Moving files and folders to the Windows Recycle Bin.
//!
//! Used where Rezure deletes something the user made — a project folder —
//! so a wrong click can still be undone from the Recycle Bin rather than
//! being gone for good.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::UI::Shell::{
    SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
    SHFILEOPSTRUCTW,
};

use crate::utils::error::AppError;

/// Sends `path` (a file or a whole folder) to the Recycle Bin.
///
/// `SHFileOperationW` with `FOF_ALLOWUNDO` is what Explorer's own Delete
/// does. Rezure has already asked the user, so the shell's confirmation and
/// progress dialogs are switched off. A path on a drive with no Recycle Bin
/// (a network share) would be deleted outright by the shell, so that case is
/// refused instead of risking it.
pub fn move_to_recycle_bin(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Err(AppError::Io(format!("{} doesn't exist", path.display())));
    }
    if path.to_string_lossy().starts_with(r"\\") {
        return Err(AppError::Io(format!(
            "{} is on a network share, which has no Recycle Bin",
            path.display()
        )));
    }

    // The shell takes a list of paths, each NUL-terminated, the list ended
    // by one more NUL.
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.extend([0, 0]);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };

    // SAFETY: `op` is fully initialised, and `from` — the double-NUL
    // terminated list it points into — outlives the call.
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(AppError::Io(format!(
            "Windows couldn't move {} to the Recycle Bin (code {code:#x}) - a file in it may be \
             open in another program",
            path.display()
        )));
    }
    if op.fAnyOperationsAborted != 0 {
        return Err(AppError::Io(format!(
            "moving {} to the Recycle Bin was cancelled",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ignored by default: it really does put a (tiny) folder in this
    /// machine's Recycle Bin. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn a_folder_goes_to_the_recycle_bin_and_is_gone_from_its_place() {
        let dir = std::env::temp_dir().join(format!("rezure-recycle-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("nested").join("index.php"), "<?php echo 1;").unwrap();

        move_to_recycle_bin(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn a_missing_path_is_an_error_not_a_silent_success() {
        let missing = std::env::temp_dir().join("rezure-recycle-test-does-not-exist");
        assert!(move_to_recycle_bin(&missing).is_err());
    }
}
