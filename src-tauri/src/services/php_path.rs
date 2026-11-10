//! Makes Rezure's active PHP — and Composer and Node.js with it — the `php`,
//! `composer` and `node` every terminal on the machine resolves.
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
//! # Why it's on by default
//!
//! This is the one feature that changes something outside Rezure. On a
//! machine that already has Laragon or XAMPP on PATH, enabling it means
//! `php` system-wide stops being theirs and becomes Rezure's.
//!
//! It used to be opt-in for that reason, but in practice most users never
//! found the switch. They then ran `php`/`composer` from Laragon in the
//! terminal while their sites ran on Rezure's PHP, and got version
//! mismatches they couldn't explain. So startup now turns it on once,
//! even when another PHP is present (see `Settings::php_path_auto_enabled`
//! in `config::settings`). The toggle stays, a user who turns it off stays
//! off, and [`disable`] still puts everything back. No UAC either way: only
//! the *user* PATH and junctions under Rezure's own folder are written.
//!
//! It carries Composer and Node.js along: `current\composer` and
//! `current\node` are junctions of their own, pointed at the active
//! versions, with PATH entries right after PHP's — see [`Tool`] for why they
//! share this switch instead of having their own.
//!
//! Besides the PATH entries, one more thing is written: `PHP_INI_SCAN_DIR`
//! pointing at [`php_ini::conf_d`] so the exposed `php` reads the same user
//! settings the web server does. Both are undone by [`disable`], and both
//! leave anything they didn't put there alone.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::php;
use super::php_ini;
use crate::utils::error::AppError;
use crate::utils::paths;
use crate::utils::powershell::{quote_ps, run};

/// `%LOCALAPPDATA%\Rezure\current` — holds the switchable junctions.
fn current_root() -> Result<PathBuf, AppError> {
    paths::current()
}

/// The single directory that goes on PATH.
pub fn link_dir() -> Result<PathBuf, AppError> {
    Ok(current_root()?.join("php"))
}

/// A runtime that rides on this switch besides PHP itself, each through its
/// own junction under `current\` and its own PATH entry right after PHP's.
///
/// - **Composer** (`current\composer`): its `composer.bat` shim (see
///   `composer::bin_dir`) runs `php` from PATH, so a global `composer`
///   without a global `php` would be a command that exists and then fails.
///   Composer-Setup puts both on PATH for the same reason.
/// - **Node.js** (`current\node`): the active version's folder, which holds
///   `node`, `npm` and `npx`, and also whatever `npm install -g` adds,
///   since a zip install's global prefix is that same folder.
///
/// One switch rather than one per runtime: what users needed was "what I
/// installed in Switch works in my terminal", and three toggles to find is
/// how the PHP one went unnoticed in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Composer,
    Node,
}

impl Tool {
    /// In the order their entries go on PATH, after PHP's.
    pub const ALL: [Tool; 2] = [Tool::Composer, Tool::Node];

    fn folder(self) -> &'static str {
        match self {
            Tool::Composer => "composer",
            Tool::Node => "node",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tool::Composer => "Composer",
            Tool::Node => "Node.js",
        }
    }

    /// The folder this tool's link should point at right now, or `None`
    /// when no version of it is installed.
    fn target(self) -> Option<PathBuf> {
        match self {
            Tool::Composer => super::composer::bin_dir(),
            Tool::Node => super::node::terminal_bin_dir(None),
        }
    }

    /// `current\<folder>`, the directory that goes on PATH.
    pub fn link_dir(self) -> Result<PathBuf, AppError> {
        Ok(current_root()?.join(self.folder()))
    }
}

/// Every tool's link directory, as PATH spells it, in [`Tool::ALL`] order.
fn tool_link_strings() -> Result<Vec<String>, AppError> {
    Tool::ALL
        .iter()
        .map(|tool| tool.link_dir().map(|dir| dir.display().to_string()))
        .collect()
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
    /// Where `current\composer` points, if it exists — `None` while no
    /// Composer is installed, even with the feature on.
    pub composer_target: Option<String>,
    /// Where `current\node` points, the same way.
    pub node_target: Option<String>,
}

/// Reads the *raw* user PATH from the registry, without expanding
/// `%VAR%` references.
///
/// `[Environment]::GetEnvironmentVariable(…, 'User')` expands them, and
/// writing that back turns a `REG_EXPAND_SZ` entry into a literal — the
/// classic way PATH gets quietly mangled. Reading raw and writing back with
/// the *same* value kind avoids it.
fn read_user_path() -> Result<String, AppError> {
    read_user_env("Path", "reading your PATH")
}

/// Reads one value out of the user's `Environment` key, unexpanded.
///
/// Shared by PATH and `PHP_INI_SCAN_DIR` because the reason for
/// `DoNotExpandEnvironmentNames` is the same for both: a value read expanded
/// and written back turns a `REG_EXPAND_SZ` entry into a literal.
fn read_user_env(name: &str, what: &str) -> Result<String, AppError> {
    let script = format!(
        "$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment'); \
         if ($null -eq $k) {{ '' }} else {{ \
         [string]$k.GetValue({name}, '', \
         [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }}",
        name = quote_ps(name)
    );
    run(&script, what)
}

/// Tells already-running shells and newly-launched apps that the environment
/// changed, so neither PATH nor `PHP_INI_SCAN_DIR` needs a sign-out to take
/// effect. Appended to every script here that writes one.
const BROADCAST_SETTING_CHANGE: &str = "Add-Type -Namespace RezureWin32 -Name Env \
     -MemberDefinition '[DllImport(\"user32.dll\", SetLastError=true, CharSet=CharSet.Auto)] \
     public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, \
     string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'; \
     $r = [UIntPtr]::Zero; \
     [void][RezureWin32.Env]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, \
     'Environment', 2, 5000, [ref]$r)";

/// Writes the user PATH back, preserving its registry value kind, then
/// broadcasts `WM_SETTINGCHANGE` so newly-launched apps see it without a
/// sign-out.
fn write_user_path(value: &str) -> Result<(), AppError> {
    let script = format!(
        "$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true); \
         $kind = $k.GetValueKind('Path'); \
         $k.SetValue('Path', {value}, $kind); \
         $k.Close(); \
         {BROADCAST_SETTING_CHANGE}",
        value = quote_ps(value)
    );
    run(&script, "updating your PATH").map(|_| ())
}

/// Sets a user environment variable, or removes it when `value` is `None`.
///
/// Written as `REG_SZ`, unlike PATH: the value is a plain absolute path with
/// no `%VAR%` in it, and a variable Rezure created is Rezure's to type.
fn write_user_env(name: &str, value: Option<&str>) -> Result<(), AppError> {
    let assignment = match value {
        Some(value) => format!("$k.SetValue({}, {})", quote_ps(name), quote_ps(value)),
        // Deleting a value that isn't there throws, and "already gone" is
        // exactly the state this wants to reach.
        None => format!(
            "if ($null -ne $k.GetValue({name})) {{ $k.DeleteValue({name}) }}",
            name = quote_ps(name)
        ),
    };
    let script = format!(
        "$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true); \
         {assignment}; \
         $k.Close(); \
         {BROADCAST_SETTING_CHANGE}"
    );
    run(&script, "updating your environment").map(|_| ())
}

/// Splits a PATH string into its non-empty entries, for *reading* it.
fn entries(path: &str) -> Vec<&str> {
    path.split(';')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect()
}

/// Also used by `php_ini::terminal_scan_dir`, which compares against the
/// same kind of hand-edited, `;`-separated environment value.
pub(super) fn same_dir(a: &str, b: &str) -> bool {
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

/// Link directories Rezure used before the single-folder layout.
///
/// Only ever *removed* from PATH, never added — this exists so a PATH left
/// pointing at the old location can be repaired.
fn legacy_link_dirs() -> Vec<String> {
    dirs::data_local_dir()
        .map(|base| {
            vec![base
                .join("Rezure")
                .join("current")
                .join("php")
                .display()
                .to_string()]
        })
        .unwrap_or_default()
}

/// Replaces a stale pre-move PATH entry with the current one, in place.
///
/// Without this, a migrated install is left in a state nothing else can see
/// or repair: PATH still names `%LOCALAPPDATA%\Rezure\current\php`, which no
/// longer exists, while [`status`] reports `on_path: false` because it looks
/// for the *new* directory. So `php` is broken in every terminal, the UI shows
/// the feature as switched off, and enabling it would append a second entry
/// while leaving the dead one in front of it.
///
/// The swap is positional — the old entry's slot is reused rather than being
/// dropped and re-appended — because the entry only wins over a Laragon or
/// XAMPP `php` by coming before it.
///
/// Returns `true` if PATH was rewritten.
pub fn repair_legacy_entry() -> Result<bool, AppError> {
    let link = link_dir()?.display().to_string();
    let legacy = legacy_link_dirs();
    let raw = read_user_path()?;

    let mut found = false;
    let repaired = raw
        .split(';')
        .map(|segment| {
            if legacy.iter().any(|old| same_dir(segment, old)) {
                found = true;
                link.as_str()
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join(";");

    if !found {
        return Ok(false);
    }

    // A PATH that already carries the new entry as well would end up with it
    // twice. Drop the duplicate that isn't in the reclaimed slot.
    let deduped = {
        let mut seen = false;
        repaired
            .split(';')
            .filter(|segment| {
                if same_dir(segment, &link) {
                    let first = !seen;
                    seen = true;
                    return first;
                }
                true
            })
            .collect::<Vec<_>>()
            .join(";")
    };

    write_user_path(&deduped)?;
    log::info!("repaired a pre-move PHP entry in PATH: it now points at {link}");
    Ok(true)
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
        composer_target: junction_target(&Tool::Composer.link_dir()?),
        node_target: junction_target(&Tool::Node.link_dir()?),
    })
}

/// Whether the feature is on, read straight from PATH — cheaper than a full
/// [`status`], which also resolves both junctions.
fn enabled() -> Result<bool, AppError> {
    let link = link_dir()?.display().to_string();
    Ok(entries(&read_user_path()?)
        .iter()
        .any(|entry| same_dir(entry, &link)))
}

/// Points `link` at `target`, creating it if needed; a no-op when it
/// already points there.
///
/// Re-pointing is delete-then-create: `New-Item -Force` refuses to replace a
/// junction that has content behind it. The delete uses
/// `[IO.Directory]::Delete($link, $false)`, which removes only the reparse
/// point — `Remove-Item -Recurse` would follow the link and delete the real
/// install on the other side.
fn point_junction(link: &Path, target: &Path, what: &str) -> Result<(), AppError> {
    if junction_target(link)
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
    run(&script, what).map(|_| ())
}

/// Removes only the junction itself, never what it points at.
fn remove_junction(link: &Path, what: &str) -> Result<(), AppError> {
    // `exists()` follows the link, so a junction whose target was deleted
    // reads as absent; `symlink_metadata` sees the reparse point itself.
    if std::fs::symlink_metadata(link).is_err() {
        return Ok(());
    }
    let script = format!(
        "[System.IO.Directory]::Delete({}, $false)",
        quote_ps(&link.display().to_string())
    );
    run(&script, what).map(|_| ())
}

/// Points `tool`'s link at its active version, or removes it when none is
/// installed — a dangling link would make the command resolve to a folder
/// that's no longer there.
pub fn sync_tool(tool: Tool) -> Result<(), AppError> {
    let link = tool.link_dir()?;
    match tool.target() {
        Some(target) => point_junction(
            &link,
            &target,
            &format!("pointing the {} link", tool.label()),
        ),
        None => remove_junction(&link, &format!("removing the {} link", tool.label())),
    }
}

/// Re-points `tool`'s link after an install or switch of it, only when the
/// feature is on. Best-effort by design: callers are installs and switches
/// that already succeeded, and must not report failure over this.
pub fn refresh_tool_link(tool: Tool) {
    match enabled() {
        Ok(true) => {
            if let Err(err) = sync_tool(tool) {
                log::warn!("could not re-point the {} link: {err}", tool.label());
            }
        }
        Ok(false) => {}
        Err(err) => log::warn!("could not read whether the PHP link is on: {err}"),
    }
}

/// Inserts any missing tool entry right after PHP's, for an install that
/// turned the feature on before it carried that tool. No-op when it's off or
/// every entry is already there. Returns `true` if PATH was rewritten.
pub fn ensure_tool_entries() -> Result<bool, AppError> {
    let link = link_dir()?.display().to_string();
    let tools = tool_link_strings()?;
    let raw = read_user_path()?;

    match with_tool_entries(&raw, &link, &tools) {
        Some(updated) => {
            write_user_path(&updated)?;
            log::info!("added Rezure's tool links to PATH after {link}");
            Ok(true)
        }
        None => Ok(false),
    }
}

/// The pure half of [`ensure_tool_entries`]: `raw` with whichever of `tools`
/// it lacks inserted right after `link`, in order, or `None` when there's
/// nothing to do. Entries already present stay where they are.
fn with_tool_entries(raw: &str, link: &str, tools: &[String]) -> Option<String> {
    let present = entries(raw);
    if !present.iter().any(|e| same_dir(e, link)) {
        return None;
    }
    let missing: Vec<&str> = tools
        .iter()
        .map(String::as_str)
        .filter(|tool| !present.iter().any(|e| same_dir(e, tool)))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut inserted = false;
    let mut out = Vec::new();
    for segment in raw.split(';') {
        out.push(segment);
        if !inserted && same_dir(segment, link) {
            out.extend(missing.iter().copied());
            inserted = true;
        }
    }
    Some(out.join(";"))
}

/// Puts Rezure's `conf.d` on the machine-wide `PHP_INI_SCAN_DIR`, so the
/// `php` this feature exposes reads the same user settings the web server
/// does.
///
/// Without it, the terminal `php` loads only the ini sitting beside
/// `php.exe` — a per-version file that a switch replaces — while every web
/// request reads `conf.d`. The two would then disagree about which
/// extensions and limits are in force, which is the confusing half of the
/// problem this folder exists to fix.
///
/// An existing value is kept and ours appended, never replaced: the variable
/// is machine-wide, so another tool may already be using it, and appending
/// also puts Rezure's folder last, where it wins on any setting both name.
fn ensure_scan_dir_env() -> Result<(), AppError> {
    let conf_d = php_ini::ensure_conf_d()?.display().to_string();
    let current = read_user_env(php_ini::SCAN_DIR_ENV, "reading your PHP settings folder")?;

    if entries(&current)
        .iter()
        .any(|entry| same_dir(entry, &conf_d))
    {
        return Ok(());
    }

    let updated = if entries(&current).is_empty() {
        conf_d
    } else {
        format!("{current};{conf_d}")
    };
    write_user_env(php_ini::SCAN_DIR_ENV, Some(&updated))
}

/// Takes Rezure's folder back out of `PHP_INI_SCAN_DIR`, removing the
/// variable entirely only when nothing else was in it.
///
/// The same rule PATH gets: turning the feature off undoes what Rezure did
/// and nothing else. A value some other tool set is left standing.
fn remove_scan_dir_env() -> Result<(), AppError> {
    let conf_d = php_ini::conf_d()?.display().to_string();
    let current = read_user_env(php_ini::SCAN_DIR_ENV, "reading your PHP settings folder")?;

    let rest = without_entry(&current, &conf_d);
    if rest == current {
        return Ok(());
    }

    write_user_env(
        php_ini::SCAN_DIR_ENV,
        (!entries(&rest).is_empty()).then_some(rest.as_str()),
    )
}

/// Points the junction at the active version, creating it if needed (see
/// [`point_junction`]), and every [`Tool`] link at its active version.
pub fn sync() -> Result<(), AppError> {
    let link = link_dir()?;
    let target = php::active_exe()?
        .parent()
        .ok_or_else(|| AppError::Io("the active PHP has no parent directory".to_string()))?
        .to_path_buf();

    // Behind the junction, PATH resolves `php` to this folder directly —
    // Rezure's `-c` ini never enters the picture — and PHP reads only the
    // ini sitting next to `php.exe`. Versions installed before Rezure
    // wrote one still have none, so this heals them on the switch that
    // first exposes them, ahead of the up-to-date check below.
    match php_ini::ensure_cli_php_ini(&target) {
        Ok(Some(path)) => log::info!("wrote {}", path.display()),
        Ok(None) => {}
        Err(err) => log::warn!("could not write php.ini in {}: {err}", target.display()),
    }

    // Best-effort, and here rather than only in `enable`, so an install that
    // turned this feature on before Rezure had a `conf.d` gets the variable
    // on its next switch instead of never.
    if let Err(err) = ensure_scan_dir_env() {
        log::warn!("could not set {}: {err}", php_ini::SCAN_DIR_ENV);
    }

    // Best-effort: a Composer or Node problem must not take `php` down
    // with it.
    for tool in Tool::ALL {
        if let Err(err) = sync_tool(tool) {
            log::warn!("could not point the {} link: {err}", tool.label());
        }
    }

    point_junction(
        &link,
        &target,
        "pointing the PHP link at the active version",
    )
}

/// `raw` with Rezure's entries moved to the front — `ours` in order, PHP's
/// first — and any earlier copies of them dropped.
fn with_entries_first(raw: &str, ours: &[String]) -> String {
    let rest = without_entries(raw, ours);
    let front = ours.join(";");
    if rest.is_empty() {
        front
    } else {
        format!("{front};{rest}")
    }
}

/// `raw` with every one of `ours` removed, every other byte kept — see
/// [`without_entry`].
fn without_entries(raw: &str, ours: &[String]) -> String {
    ours.iter()
        .fold(raw.to_string(), |path, entry| without_entry(&path, entry))
}

/// PHP's link directory followed by every tool's, as PATH spells them.
fn all_link_strings() -> Result<Vec<String>, AppError> {
    let mut ours = vec![link_dir()?.display().to_string()];
    ours.extend(tool_link_strings()?);
    Ok(ours)
}

/// Points every junction at its active version and puts them first on PATH.
///
/// First, so they win over a Laragon, XAMPP or Composer-Setup entry already
/// there — which is the whole point, and exactly what the UI has to have
/// warned about. A tool's entry goes on even with nothing of it installed
/// yet: installing one later re-points the link, and PATH is never touched
/// again.
pub fn enable() -> Result<PhpPathStatus, AppError> {
    sync()?;

    let user_path = read_user_path()?;
    write_user_path(&with_entries_first(&user_path, &all_link_strings()?))?;

    status()
}

/// Removes every entry from PATH, drops Rezure's folder from
/// `PHP_INI_SCAN_DIR`, and deletes every junction, leaving the machine as it
/// was.
pub fn disable() -> Result<PhpPathStatus, AppError> {
    let user_path = read_user_path()?;
    write_user_path(&without_entries(&user_path, &all_link_strings()?))?;

    // The folder itself stays — it holds the user's settings, and Rezure's
    // own processes keep reading it. Only the machine-wide pointer goes.
    if let Err(err) = remove_scan_dir_env() {
        log::warn!("could not clear {}: {err}", php_ini::SCAN_DIR_ENV);
    }

    for tool in Tool::ALL {
        remove_junction(
            &tool.link_dir()?,
            &format!("removing the {} link", tool.label()),
        )?;
    }
    remove_junction(&link_dir()?, "removing the PHP link")?;

    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The positional swap `repair_legacy_entry` performs, on a plain string
    /// so it can be checked without touching the real registry.
    fn swap(raw: &str, legacy: &[String], link: &str) -> (String, bool) {
        let mut found = false;
        let repaired = raw
            .split(';')
            .map(|segment| {
                if legacy.iter().any(|old| same_dir(segment, old)) {
                    found = true;
                    link
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
            .join(";");
        (repaired, found)
    }

    #[test]
    fn a_stale_entry_is_replaced_where_it_stood() {
        // Position is the whole point: the entry only beats Laragon's `php`
        // by sitting in front of it, so a repair that appended instead of
        // swapping would silently hand `php` back to Laragon.
        let legacy = vec![r"C:\Users\x\AppData\Local\Rezure\current\php".to_string()];
        let (out, found) = swap(
            r"C:\Users\x\AppData\Local\Rezure\current\php;C:\laragon\bin\php;C:\Windows",
            &legacy,
            r"C:\rezure\current\php",
        );
        assert!(found);
        assert_eq!(out, r"C:\rezure\current\php;C:\laragon\bin\php;C:\Windows");
    }

    #[test]
    fn a_path_without_a_stale_entry_is_left_alone() {
        let legacy = vec![r"C:\Users\x\AppData\Local\Rezure\current\php".to_string()];
        let original = r"C:\laragon\bin\php;C:\Windows;";
        let (out, found) = swap(original, &legacy, r"C:\rezure\current\php");
        assert!(!found, "nothing to repair");
        assert_eq!(out, original, "every other byte must survive untouched");
    }

    #[test]
    fn a_trailing_separator_survives_the_swap() {
        // `without_entry` documents why this matters: PATH belongs to the
        // user, so a repair must not quietly reformat it.
        let legacy = vec![r"C:\old\php".to_string()];
        let (out, _) = swap(r"C:\old\php;C:\Windows;", &legacy, r"C:\rezure\current\php");
        assert!(out.ends_with(';'));
    }

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

    const PHP: &str = r"C:\rezure\current\php";
    const COMPOSER: &str = r"C:\rezure\current\composer";
    const NODE: &str = r"C:\rezure\current\node";

    fn ours() -> Vec<String> {
        vec![PHP.to_string(), COMPOSER.to_string(), NODE.to_string()]
    }

    #[test]
    fn enabling_puts_php_composer_and_node_first_and_round_trips() {
        for original in [r"C:\a;C:\b;", r"C:\a", ""] {
            let enabled = with_entries_first(original, &ours());
            assert!(enabled.starts_with(&format!("{PHP};{COMPOSER};{NODE}")));
            assert_eq!(
                without_entries(&enabled, &ours()),
                original,
                "disabling must restore {original:?}"
            );
        }

        // Re-enabling moves stale copies to the front instead of duplicating.
        let stale = format!(r"C:\x;{NODE};{COMPOSER};{PHP};C:\y");
        assert_eq!(
            with_entries_first(&stale, &ours()),
            format!(r"{PHP};{COMPOSER};{NODE};C:\x;C:\y")
        );
    }

    #[test]
    fn an_older_enabled_path_gains_the_missing_tools_right_after_php() {
        let tools = vec![COMPOSER.to_string(), NODE.to_string()];

        // Enabled before either tool rode along.
        assert_eq!(
            with_tool_entries(&format!(r"C:\a;{PHP};C:\b;"), PHP, &tools),
            Some(format!(r"C:\a;{PHP};{COMPOSER};{NODE};C:\b;"))
        );
        // Enabled on 3.6.0, which already had Composer: only Node is added.
        assert_eq!(
            with_tool_entries(&format!(r"{PHP};{COMPOSER};C:\b"), PHP, &tools),
            Some(format!(r"{PHP};{NODE};{COMPOSER};C:\b"))
        );
        // Feature off: nothing to add.
        assert_eq!(with_tool_entries(r"C:\a;C:\b", PHP, &tools), None);
        // Already there: nothing to add.
        assert_eq!(
            with_tool_entries(&format!("{PHP};{COMPOSER};{NODE}"), PHP, &tools),
            None
        );
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
