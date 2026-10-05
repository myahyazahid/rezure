//! Files only the current user can open.
//!
//! Windows' OpenSSH refuses a private key that any other account can read
//! ("UNPROTECTED PRIVATE KEY FILE … bad permissions"), and a file normally
//! takes its permissions from the folder it lands in. Neither the drive root
//! (`BUILTIN\Users`, `Authenticated Users`) nor even the user's own profile
//! is reliable for that — software routinely adds groups to `AppData`. So a
//! file that has to pass the check gets its permissions spelled out instead
//! of inherited, the way `ssh-keygen` does for every key it writes.

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Creates `path`, which must not exist yet, readable and writable by the
/// current user and no one else.
///
/// The permissions are part of the `CreateFileW` call itself, so there is no
/// moment where the file exists with its folder's permissions — anything
/// written to it was never readable by another account.
pub fn create(path: &Path) -> io::Result<File> {
    // D:P — a *protected* DACL, so nothing is inherited from the folder.
    // One ACE: full access, for the current user's SID only.
    let sddl = wide(format!("D:P(A;;FA;;;{})", current_user_sid()?).as_ref());
    let path = wide(path.as_os_str());

    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the calls;
    // `descriptor` is only read after the conversion succeeded and is freed
    // with `LocalFree` as that API requires; the handle is owned by the
    // returned `File` and nowhere else.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            path.as_ptr(),
            FILE_GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        // Read before `LocalFree`, which may overwrite the thread's last error.
        let created = if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(File::from_raw_handle(handle))
        };
        LocalFree(descriptor);
        created
    }
}

/// The current user's SID as a string (`S-1-5-21-…`), for an SDDL ACE.
///
/// The SID itself rather than a name: a name is translated through the
/// locale and the domain, a SID is what the check compares against.
fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no
    // closing; `token` is closed once read; the buffer is sized by
    // `GetTokenInformation`'s own answer and `u64`-backed so the
    // `TOKEN_USER` at its start is aligned; the SID string is freed with
    // `LocalFree` as `ConvertSidToStringSidW` requires.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        // A TOKEN_USER is followed by the SID it points into, so its size
        // isn't known until asked for. This first call fails by design.
        let mut needed = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
        );
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(error);
        }

        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut string = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut string) == 0 {
            return Err(io::Error::last_os_error());
        }
        let len = (0..).take_while(|&i| *string.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(string, len));
        LocalFree(string.cast());
        Ok(sid)
    }
}

/// `value` as NUL-terminated UTF-16, for a `PCWSTR` argument.
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rezure-private-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn the_sid_is_a_user_sid() {
        let sid = current_user_sid().expect("the process token has a user");
        assert!(sid.starts_with("S-1-5-"), "{sid}");
    }

    #[test]
    fn a_created_file_holds_what_was_written() {
        let path = scratch("write");
        let mut file = create(&path).expect("a fresh path can be created");
        file.write_all(b"secret").unwrap();
        drop(file);
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        let _ = std::fs::remove_file(path);
    }

    /// `CREATE_NEW`: an existing file — possibly one someone else planted,
    /// with their own permissions — is never reused.
    #[test]
    fn an_existing_file_is_never_reused() {
        let path = scratch("exists");
        std::fs::write(&path, b"planted").unwrap();
        let err = create(&path).expect_err("an existing path must be refused");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        let _ = std::fs::remove_file(path);
    }
}
