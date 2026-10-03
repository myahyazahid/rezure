//! Runs one PowerShell script with admin rights, through the real UAC prompt.
//!
//! Shared by every step in Rezure that genuinely needs elevation: rewriting
//! the hosts file (`services::hosts`), and running the two Microsoft
//! installers SQL Server support depends on (`services::msi`). Each caller
//! writes its own script — the elevated part is kept to the one command that
//! needs it — and judges success by checking the effect afterwards, never by
//! the elevated process's exit code (see [`run_script`]).
//!
//! Also answers the opposite question — whether Rezure is *already*
//! elevated — for the one service that refuses to run that way
//! ([`is_elevated`]).

use std::path::Path;
use std::process::Command;

use crate::utils::command::HiddenWindow;
use crate::utils::powershell::quote_ps;

/// The exit code `launcher` reports when the user declines the prompt —
/// Windows' own `ERROR_CANCELLED`.
const DECLINED_EXIT_CODE: i32 = 1223;

/// How an elevated run ended, as far as it can be known from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevated {
    /// The prompt was approved and the script ran to completion. Whether it
    /// *succeeded* is for the caller to check against the thing it changed.
    Ran,
    /// The user clicked No on the UAC prompt; nothing ran.
    Declined,
}

/// The `powershell -Command` one-liner that re-launches `script_path`
/// elevated.
///
/// `-Verb RunAs` throws synchronously if the user declines the UAC prompt —
/// before `-Wait` would ever start blocking — so that failure mode alone is
/// reliable to catch here, and is surfaced as exit code 1223.
///
/// The script path is wrapped in *embedded* double quotes on top of the
/// PowerShell string literal: `Start-Process -ArgumentList @(...)` joins the
/// array elements with plain spaces and quotes none of them itself, so a
/// path containing spaces (`C:\Users\Jane Doe\...` — every Windows account
/// whose name has one) would otherwise reach the elevated PowerShell split
/// across several arguments, leaving `-File` holding only the first
/// fragment. That fails *before* the script runs, so it leaves no error log
/// behind. Windows paths cannot contain `"`, so nothing further needs
/// escaping.
fn launcher(script_path: &Path) -> String {
    let script_arg = quote_ps(&format!("\"{}\"", script_path.display()));
    format!(
        "try {{ Start-Process powershell -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',{script_arg}) -Verb RunAs -Wait; exit 0 }} catch {{ exit {DECLINED_EXIT_CODE} }}"
    )
}

/// Runs `script_path` elevated and waits for it to finish.
///
/// Deliberately does *not* trust `$p.ExitCode` from `Start-Process -Verb
/// RunAs -PassThru` as a success signal — that combination is known to
/// report bogus exit codes (a process launched via `ShellExecuteEx` for
/// elevation isn't always bound properly for exit-code retrieval). Callers
/// wait for [`Elevated::Ran`] and then check whether what the script was
/// meant to do actually happened: a file's contents, an installed driver.
pub fn run_script(script_path: &Path) -> std::io::Result<Elevated> {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &launcher(script_path),
        ])
        // The UAC prompt this launcher raises is its own elevated window —
        // hiding this console doesn't hide the consent dialog.
        .hidden()
        .status()?;

    Ok(if status.code() == Some(DECLINED_EXIT_CODE) {
        Elevated::Declined
    } else {
        Elevated::Ran
    })
}

/// Whether Rezure itself is running elevated ("Run as administrator").
///
/// PostgreSQL refuses to start from an elevated process — "Execution of
/// PostgreSQL by a user with administrative permissions is not permitted" —
/// so `services::process` asks before spawning one, to say so up front rather
/// than report a server that exited during startup. False whenever the
/// question can't be answered; the server's own refusal still reaches its
/// log then.
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no
    // closing; `token` is only used, then closed, when `OpenProcessToken`
    // succeeded; and `elevation` is a `TOKEN_ELEVATION` whose exact size is
    // what `GetTokenInformation` is told it may write.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A username with a space in it (`C:\Users\Jane Doe\...`) must still
    /// reach the elevated PowerShell as a single `-File` argument.
    #[test]
    fn launcher_quotes_a_script_path_containing_spaces() {
        let launcher = launcher(Path::new(r"C:\Users\Jane Doe\app\apply-hosts.ps1"));
        assert!(
            launcher.contains(r#"'"C:\Users\Jane Doe\app\apply-hosts.ps1"'"#),
            "path must carry its own double quotes inside the argument list: {launcher}"
        );
    }

    #[test]
    fn a_declined_prompt_is_reported_with_windows_own_cancel_code() {
        assert!(launcher(Path::new(r"C:\x.ps1")).contains("exit 1223"));
    }
}
