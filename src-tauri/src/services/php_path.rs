//! Optional: makes Rezure's active PHP the `php` every terminal on the
//! machine resolves.
//!
//! # Why a junction rather than rewriting PATH
//!
//! Laragon switches versions by writing the *versioned* folder straight into
//! the user's PATH (`C:\laragon\bin\php\php-8.5.8-…`) and rewriting that
//! entry on every switch. That has three costs: rewriting PATH repeatedly is
//! how PATH gets corrupted, already-open terminals never see the change, and
//! two tools doing it fight over who wrote last.
//!
//! So Rezure adds **one stable entry, once**:
//!
//! ```text
//! %LOCALAPPDATA%\Rezure\current\php   →  junction  →  <active version's folder>
//! ```
//!
//! Switching re-points the junction; PATH is never touched again. Because
//! the PATH string doesn't change, **terminals that are already open pick up
//! the new version too** — they resolve `php` through the same directory,
//! whose target moved underneath them.
//!
//! A *junction*, specifically, not a symbolic link: directory junctions can
//! be created without administrator rights, while symlinks can't. (That's
//! why nvm-windows, which uses a symlink, ships an `elevate.cmd`.)
//!
//! # Why it stays opt-in
//!
//! This is the one feature that changes something outside Rezure. On a
//! machine that already has Laragon or XAMPP on PATH, enabling it means
//! `php` system-wide stops being theirs and becomes Rezure's. That's a
//! decision to be taken deliberately, so nothing here runs unless the user
//! turns it on, and [`disable`] puts everything back.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::php;
use crate::utils::error::AppError;
use crate::utils::powershell::{quote_ps, run};

/// `%LOCALAPPDATA%\Rezure\current` — holds the switchable junctions.
fn current_root() -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base.join("Rezure").join("current"))
}

/// The single directory that goes on PATH.
pub fn link_dir() -> Result<PathBuf, AppError> {
    Ok(current_root()?.join("php"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpPathStatus {
    /// The directory Rezure puts on PATH.
    pub link_dir: String,
    /// Whether `link_dir` is currently in the user's PATH.
    pub on_path: bool,
    /// Where the junction currently points, if it exists.
    pub target: Option<String>,
    /// Whether the junction matches the active version — false means a
    /// switch happened while this was off, and enabling will re-point it.
    pub in_sync: bool,
    /// Other `php.exe` directories already on PATH (Laragon, XAMPP, a manual
    /// install). Surfaced so the UI can say exactly whose `php` is being
    /// taken over instead of leaving the user to find out.
    pub conflicts: Vec<String>,
}

/// Reads the *raw* user PATH from the registry, without expanding
/// `%VAR%` references.
///
/// `[Environment]::GetEnvironmentVariable(…, 'User')` expands them, and
/// writing that back turns a `REG_EXPAND_SZ` entry into a literal — the
/// classic way PATH gets quietly mangled. Reading raw and writing back with
/// the *same* value kind avoids it.
fn read_user_path() -> Result<String, AppError> {
    run(
        "$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment'); \
         if ($null -eq $k) { '' } else { \
         [string]$k.GetValue('Path', '', \
         [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }",
        "reading your PATH",
    )
}

/// Writes the user PATH back, preserving its registry value kind, then
/// broadcasts `WM_SETTINGCHANGE` so newly-launched apps see it without a
/// sign-out.
fn write_user_path(value: &str) -> Result<(), AppError> {
    let script = format!(
        "$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true); \
         $kind = $k.GetValueKind('Path'); \
         $k.SetValue('Path', {value}, $kind); \
         $k.Close(); \
         Add-Type -Namespace RezureWin32 -Name Env -MemberDefinition '[DllImport(\"user32.dll\", \
         SetLastError=true, CharSet=CharSet.Auto)] public static extern IntPtr SendMessageTimeout(\
         IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, \
         out UIntPtr lpdwResult);'; \
         $r = [UIntPtr]::Zero; \
         [void][RezureWin32.Env]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, \
         'Environment', 2, 5000, [ref]$r)",
        value = quote_ps(value)
    );
    run(&script, "updating your PATH").map(|_| ())
}

/// Splits a PATH string into its non-empty entries, for *reading* it.
fn entries(path: &str) -> Vec<&str> {
    path.split(';')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect()
}

fn same_dir(a: &str, b: &str) -> bool {
    a.trim()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.trim().trim_end_matches(['\\', '/']))
}

/// Removes our entry from a raw PATH string, leaving every other byte
/// exactly as it was — empty segments, spacing, a trailing `;` and all.
///
/// Splitting into entries and rejoining is simpler but silently reformats
/// the whole variable: a PATH that ended in `;` comes back without it. This
/// is the user's PATH, not Rezure's, so turning the feature on and off again
/// has to be a no-op on everything except our own entry.
fn without_entry(raw: &str, link: &str) -> String {
    raw.split(';')
        .filter(|segment| !same_dir(segment, link))
        .collect::<Vec<_>>()
        .join(";")
}

/// Where the junction points right now, or `None` if it isn't there.
fn junction_target(link: &Path) -> Option<String> {
    if !link.exists() {
        return None;
    }
    let script = format!(
        "$i = Get-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue; \
         if ($i -and $i.Target) {{ $i.Target | Select-Object -First 1 }}",
        quote_ps(&link.display().to_string())
    );
    run(&script, "reading the PHP link")
        .ok()
        .filter(|target| !target.is_empty())
}

/// Other PHP installs already on PATH — what enabling this would override.
fn conflicts_on_path(path: &str, link: &Path) -> Vec<String> {
    let link = link.display().to_string();
    entries(path)
        .into_iter()
        .filter(|entry| !same_dir(entry, &link))
        .filter(|entry| Path::new(entry).join("php.exe").is_file())
        .map(str::to_string)
        .collect()
}

pub fn status() -> Result<PhpPathStatus, AppError> {
    let link = link_dir()?;
    let link_str = link.display().to_string();
    let user_path = read_user_path().unwrap_or_default();
    let target = junction_target(&link);

    let active_dir = php::installed()
        .into_iter()
        .find(|runtime| runtime.version == php::active_id())
        .map(|runtime| runtime.dir.display().to_string());

    Ok(PhpPathStatus {
        on_path: entries(&user_path)
            .iter()
            .any(|entry| same_dir(entry, &link_str)),
        in_sync: match (&target, &active_dir) {
            (Some(target), Some(active)) => same_dir(target, active),
            _ => false,
        },
        conflicts: conflicts_on_path(&user_path, &link),
        target,
        link_dir: link_str,
    })
}

/// Points the junction at the active version, creating it if needed.
///
/// Re-pointing is delete-then-create: `New-Item -Force` refuses to replace a
/// junction that has content behind it. The delete uses
/// `[IO.Directory]::Delete($link, $false)`, which removes only the reparse
/// point — `Remove-Item -Recurse` would follow the link and delete the real
/// PHP install on the other side.
pub fn sync() -> Result<(), AppError> {
    let link = link_dir()?;
    let target = php::active_exe()?
        .parent()
        .ok_or_else(|| AppError::Io("the active PHP has no parent directory".to_string()))?
        .to_path_buf();

    if junction_target(&link)
        .is_some_and(|current| same_dir(&current, &target.display().to_string()))
    {
        return Ok(());
    }

    std::fs::create_dir_all(current_root()?)
        .map_err(|e| AppError::Io(format!("could not create the link directory: {e}")))?;

    let script = format!(
        "if (Test-Path -LiteralPath {link}) {{ [System.IO.Directory]::Delete({link}, $false) }}; \
         [void](New-Item -ItemType Junction -Path {link} -Target {target})",
        link = quote_ps(&link.display().to_string()),
        target = quote_ps(&target.display().to_string()),
    );
    run(&script, "pointing the PHP link at the active version").map(|_| ())
}

/// Points the junction at the active version and puts it first on PATH.
///
/// First, so it wins over a Laragon or XAMPP entry already there — which is
/// the whole point, and exactly what the UI has to have warned about.
pub fn enable() -> Result<PhpPathStatus, AppError> {
    sync()?;

    let link = link_dir()?.display().to_string();
    let user_path = read_user_path()?;
    let rest = without_entry(&user_path, &link);

    let updated = if rest.is_empty() {
        link
    } else {
        format!("{link};{rest}")
    };
    write_user_path(&updated)?;

    status()
}

/// Removes the entry from PATH and deletes the junction, leaving the machine
/// as it was.
pub fn disable() -> Result<PhpPathStatus, AppError> {
    let link = link_dir()?;
    let link_str = link.display().to_string();

    let user_path = read_user_path()?;
    write_user_path(&without_entry(&user_path, &link_str))?;

    if link.exists() {
        let script = format!(
            "[System.IO.Directory]::Delete({}, $false)",
            quote_ps(&link_str)
        );
        run(&script, "removing the PHP link")?;
    }

    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_ignores_empty_and_padded_segments() {
        assert_eq!(entries("a;;b ; c;"), vec!["a", "b", "c"]);
        assert!(entries("").is_empty());
    }

    /// PATH comparisons have to survive the spellings Windows actually
    /// produces — trailing slashes and mixed case.
    #[test]
    fn directories_compare_ignoring_case_and_trailing_slashes() {
        assert!(same_dir(
            r"C:\Rezure\current\php",
            r"c:\rezure\current\php\"
        ));
        assert!(same_dir(r"C:\a\b/", r"C:\a\b"));
        assert!(!same_dir(r"C:\a\b", r"C:\a\c"));
    }

    /// Enabling when our entry is already somewhere in PATH has to move it
    /// to the front, not add a second copy.
    #[test]
    fn our_entry_is_moved_to_the_front_never_duplicated() {
        let link = r"C:\Rezure\current\php";
        let existing = format!(r"C:\laragon\bin\php\php-8.5.8;{link};C:\other");

        let updated = format!("{link};{}", without_entry(&existing, link));

        assert_eq!(
            updated,
            format!(r"{link};C:\laragon\bin\php\php-8.5.8;C:\other")
        );
        assert_eq!(
            entries(&updated)
                .iter()
                .filter(|e| same_dir(e, link))
                .count(),
            1
        );
    }

    /// The regression the live PATH test caught: splitting into entries and
    /// rejoining drops a trailing `;` and any empty segment, quietly
    /// reformatting a PATH that Rezure was only supposed to add one entry
    /// to. Turning the feature on then off must be a byte-for-byte no-op.
    #[test]
    fn removing_our_entry_leaves_the_rest_of_path_byte_for_byte() {
        let link = r"C:\Rezure\current\php";

        for original in [
            r"C:\a;C:\b;",
            r"C:\a;C:\b",
            r"C:\a;;C:\b;",
            r"C:\ffmpeg\bin;",
            "",
        ] {
            let enabled = format!("{link};{original}");
            assert_eq!(
                without_entry(&enabled, link),
                original,
                "round-tripping {original:?} must give it back unchanged"
            );
        }
    }

    #[test]
    fn conflicts_never_include_our_own_entry() {
        let link = link_dir().unwrap();
        let path = format!("{};C:\\definitely\\not\\php", link.display());
        assert!(conflicts_on_path(&path, &link).is_empty());
    }

    /// Real end-to-end check against the machine's own PATH and filesystem.
    /// It enables, verifies, then disables again. Run with:
    /// `cargo test --lib services::php_path::tests::enable_then_disable_leaves_path_as_it_was -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn enable_then_disable_leaves_path_as_it_was() {
        let before = read_user_path().unwrap();
        println!("conflicts found: {:?}", status().unwrap().conflicts);

        let enabled = enable().unwrap();
        assert!(enabled.on_path, "the entry must be on PATH after enabling");
        assert!(
            enabled.in_sync,
            "the junction must point at the active version"
        );
        println!("link -> {:?}", enabled.target);

        let after_enable = read_user_path().unwrap();
        assert!(
            after_enable.starts_with(&enabled.link_dir),
            "our entry has to come first to win over other PHP installs"
        );

        let disabled = disable().unwrap();
        assert!(!disabled.on_path);
        assert_eq!(
            read_user_path().unwrap(),
            before,
            "disabling must restore PATH byte-for-byte"
        );
    }
}
