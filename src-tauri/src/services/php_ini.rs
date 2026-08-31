//! Generates Rezure's own `php.ini` for the bundled PHP.
//!
//! The official Windows PHP zip ships with every extension disabled by
//! default — no active `php.ini` at all, just `php.ini-development` and
//! `php.ini-production` templates nobody's told PHP to use. Composer needs
//! `openssl` just to talk to Packagist; real Laravel/WordPress installs
//! need `pdo_mysql`/`mysqli`/`mbstring`/`zip`/... Without this, PHP runs
//! but almost nothing beyond a trivial script actually works.
//!
//! Rezure writes two copies, because PHP gets started two different ways:
//!
//! - [`ensure_php_ini`] writes one under Rezure's own data folder and
//!   returns its path to pass as `php -c` — that covers every process
//!   Rezure spawns itself (the FastCGI service, Composer, scaffolding).
//! - [`ensure_cli_php_ini`] writes one *inside the install folder*, which
//!   is the only copy a `php artisan …` typed into the user's own terminal
//!   will ever read. `-c` isn't in play there: the global PATH switch
//!   ([`super::php_path`]) puts the raw install folder on PATH, so without
//!   this the CLI runs with no ini at all — `pdo_mysql` missing, which
//!   surfaces as Laravel's "could not find driver".

use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::error::AppError;
use crate::utils::paths;

/// Enabled for every PHP process Rezure spawns — the FastCGI service
/// (`services::process`) and Composer/scaffolding (`services::scaffold`) —
/// and for the `php` on the user's PATH.
const EXTENSIONS: &[&str] = &[
    "curl",
    "fileinfo",
    "gd",
    "mbstring",
    "mysqli",
    "openssl",
    "pdo_mysql",
    // Laravel 11's default `.env` uses SQLite (a local file, no server
    // needed) until a project's config points it at MariaDB instead.
    "pdo_sqlite",
    "sqlite3",
    "zip",
];

/// PHP on Windows treats `\` in ini values inconsistently depending on
/// what follows it, so every generated path uses forward slashes, which
/// Windows accepts everywhere.
fn ini_value(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn runtime_dir() -> Result<PathBuf, AppError> {
    Ok(paths::data()?.join("php"))
}

/// Sessions and uploads both need a real writable directory of their own.
/// PHP's fallback when these are unset is the system temp dir, which
/// Windows' own cleanup is free to empty out from under a running project.
///
/// Shared by both copies of the ini: it's Rezure's scratch space, not a
/// property of whichever PHP version happens to be active.
fn ensure_tmp_dir() -> Result<PathBuf, AppError> {
    let tmp = runtime_dir()?.join("tmp");
    fs::create_dir_all(&tmp)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", tmp.display())))?;
    Ok(tmp)
}

/// `<php dir>/ext`, the folder the extension DLLs actually live in.
fn extension_dir(php_dir: &Path) -> PathBuf {
    php_dir.join("ext")
}

/// Which of [`EXTENSIONS`] this particular build actually ships, as the
/// names its `extension=` lines have to use.
///
/// Enabling one whose DLL isn't there makes PHP print a startup warning on
/// *every* invocation — noise on the CLI, and bytes ahead of the response
/// under FastCGI. Two reasons a name can miss: leaner builds simply omit
/// some (no `php_zip.dll` in the 7.4 and 8.1 zips), and PHP 7 named GD
/// `php_gd2.dll` before 8.0 renamed it — hence the `<name>2` fallback.
///
/// An `ext/` folder that isn't there to look in (a version mid-install)
/// falls back to the full list, which is what this did before it checked.
fn enabled_extensions(extension_dir: &Path) -> Vec<String> {
    if !extension_dir.is_dir() {
        return EXTENSIONS.iter().map(|e| e.to_string()).collect();
    }

    EXTENSIONS
        .iter()
        .filter_map(|name| {
            [name.to_string(), format!("{name}2")]
                .into_iter()
                .find(|candidate| extension_dir.join(format!("php_{candidate}.dll")).is_file())
        })
        .collect()
}

/// The directives themselves, shared by both copies.
///
/// `extension_dir` is always absolute: PHP resolves a relative one against
/// the *working directory* of whatever invoked it, so `php artisan` run
/// from a project folder would look for the DLLs under that project, find
/// none, and load no extensions at all.
fn render(extension_dir: &Path, tmp: &Path) -> String {
    let mut ini = format!("extension_dir = \"{}\"\n", ini_value(extension_dir));
    for extension in enabled_extensions(extension_dir) {
        ini.push_str(&format!("extension={extension}\n"));
    }
    ini.push_str("memory_limit = 256M\n");
    ini.push_str("upload_max_filesize = 64M\n");
    ini.push_str("post_max_size = 64M\n");
    ini.push_str("max_execution_time = 300\n");
    // Without buffering, any stray byte a project emits before its
    // response — a space after a `?>`, a BOM, a warning — flushes PHP's
    // header block early, and every `header()`/`setcookie()` after that is
    // discarded silently. The symptom is a login that succeeds but never
    // sticks, because `Set-Cookie` never reaches the browser. Laragon
    // buffers (its php.ini derives from `php.ini-development`), so
    // projects moving over from it depend on this being on.
    ini.push_str("output_buffering = 4096\n");
    ini.push_str(&format!("session.save_path = \"{}\"\n", ini_value(tmp)));
    ini.push_str(&format!("upload_tmp_dir = \"{}\"\n", ini_value(tmp)));
    ini.push_str(&format!("sys_temp_dir = \"{}\"\n", ini_value(tmp)));
    // PHP emits a warning on every date call while this is unset. Laravel
    // overrides it per-app, so UTC is only ever the floor.
    ini.push_str("date.timezone = UTC\n");
    ini
}

/// Writes Rezure's `php.ini` for the PHP install at `php_exe`, pointing
/// `extension_dir` at its own `ext/` folder, and returns the ini's path —
/// pass it to `php`/`php-cgi` via `-c`. Regenerated on every call rather
/// than written once and left alone: it's fully derived from `php_exe`'s
/// location, nothing in it is meant to be hand-edited, and this keeps it
/// correct if the PHP version ever changes.
pub fn ensure_php_ini(php_exe: &Path) -> Result<PathBuf, AppError> {
    let dir = runtime_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;

    let tmp = ensure_tmp_dir()?;

    let php_dir = php_exe
        .parent()
        .ok_or_else(|| AppError::Io("php.exe has no parent directory".to_string()))?;

    let ini_path = dir.join("php.ini");
    fs::write(&ini_path, render(&extension_dir(php_dir), &tmp))
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", ini_path.display())))?;

    Ok(ini_path)
}

/// Writes `php.ini` into a PHP install folder, so the `php` a user types
/// into their own terminal loads the same extensions Rezure's own
/// processes get. Returns the path when one was written, or `None` when
/// the install already had an ini.
///
/// Only written when missing, never overwritten: this file sits in a
/// folder the user can open, and a hand-tuned ini (a raised
/// `memory_limit`, an extra extension, Xdebug) is theirs to keep. Unlike
/// the `-c` copy there's no staleness to correct — this one lives beside
/// the very `php.exe` it configures, so its `extension_dir` can't drift
/// onto another version.
pub fn ensure_cli_php_ini(php_dir: &Path) -> Result<Option<PathBuf>, AppError> {
    let ini_path = php_dir.join("php.ini");
    if ini_path.exists() {
        return Ok(None);
    }

    let tmp = ensure_tmp_dir()?;
    fs::write(&ini_path, render(&extension_dir(php_dir), &tmp))
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", ini_path.display())))?;

    Ok(Some(ini_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_php_ini_enables_every_required_extension() {
        let fake_php_exe = std::env::temp_dir()
            .join(format!("rezure-test-phpini-{}", std::process::id()))
            .join("php.exe");

        let ini_path = ensure_php_ini(&fake_php_exe).unwrap();
        let content = fs::read_to_string(&ini_path).unwrap();

        for extension in EXTENSIONS {
            assert!(
                content.contains(&format!("extension={extension}")),
                "missing extension={extension}"
            );
        }
        assert!(content.contains("extension_dir"));
        assert!(!content.contains('\\'), "paths must use forward slashes");

        // Both tests write the same php.ini, so neither owns the cleanup.
        let _ = fs::remove_file(&ini_path);
    }
    /// The one directive a Laragon-era project silently depends on: without
    /// it, a stray byte before the response drops `Set-Cookie` and logins
    /// never stick.
    #[test]
    fn ensure_php_ini_buffers_output_and_points_php_at_a_real_temp_dir() {
        let fake_php_exe = std::env::temp_dir()
            .join(format!("rezure-test-phpini-buf-{}", std::process::id()))
            .join("php.exe");

        let ini_path = ensure_php_ini(&fake_php_exe).unwrap();
        let content = fs::read_to_string(&ini_path).unwrap();
        let tmp = runtime_dir().unwrap().join("tmp");

        assert!(content.contains("output_buffering = 4096"));
        assert!(content.contains("date.timezone = UTC"));
        for directive in ["session.save_path", "upload_tmp_dir", "sys_temp_dir"] {
            assert!(
                content.contains(&format!("{directive} = \"{}\"", ini_value(&tmp))),
                "missing {directive}"
            );
        }
        assert!(tmp.is_dir(), "the temp dir php.ini points at must exist");

        // Both tests write the same php.ini, so neither owns the cleanup.
        let _ = fs::remove_file(&ini_path);
    }

    /// The regression this guards: the PATH switch exposes the install
    /// folder directly, and PHP there reads only the ini sitting next to
    /// `php.exe`. Without one, `php artisan migrate` fails with "could not
    /// find driver" while everything inside Rezure keeps working.
    #[test]
    fn ensure_cli_php_ini_writes_next_to_php_exe_with_an_absolute_extension_dir() {
        let php_dir =
            std::env::temp_dir().join(format!("rezure-test-cli-ini-{}", std::process::id()));
        let _ = fs::remove_dir_all(&php_dir);
        fs::create_dir_all(&php_dir).unwrap();

        let ini_path = ensure_cli_php_ini(&php_dir).unwrap().unwrap();
        assert_eq!(ini_path, php_dir.join("php.ini"));

        let content = fs::read_to_string(&ini_path).unwrap();
        assert!(content.contains("extension=pdo_mysql"));
        assert!(
            content.contains(&format!(
                "extension_dir = \"{}\"",
                ini_value(&php_dir.join("ext"))
            )),
            "extension_dir must be the absolute path to this install's ext/, got: {content}"
        );

        let _ = fs::remove_dir_all(&php_dir);
    }

    /// Every `extension=` line has to name a DLL that's really there, or
    /// PHP warns on every single invocation. Guards both ways a name can
    /// miss: absent entirely, and PHP 7's `php_gd2.dll` spelling.
    #[test]
    fn only_extensions_the_build_actually_ships_are_enabled() {
        let php_dir =
            std::env::temp_dir().join(format!("rezure-test-ini-ext-{}", std::process::id()));
        let ext = php_dir.join("ext");
        let _ = fs::remove_dir_all(&php_dir);
        fs::create_dir_all(&ext).unwrap();
        for dll in ["php_pdo_mysql.dll", "php_curl.dll", "php_gd2.dll"] {
            fs::write(ext.join(dll), "").unwrap();
        }

        let enabled = enabled_extensions(&ext);

        assert!(enabled.contains(&"pdo_mysql".to_string()));
        assert!(enabled.contains(&"curl".to_string()));
        // PHP 7 spelling: the directive has to match the DLL that exists.
        assert!(enabled.contains(&"gd2".to_string()));
        assert!(!enabled.contains(&"gd".to_string()));
        // No php_zip.dll in this build, so no line claiming there is one.
        assert!(!enabled.contains(&"zip".to_string()));

        let _ = fs::remove_dir_all(&php_dir);
    }

    /// A version still being unpacked has no `ext/` to inspect, and an ini
    /// listing nothing would be worse than one listing too much.
    #[test]
    fn a_missing_ext_folder_falls_back_to_the_full_list() {
        let nowhere = std::env::temp_dir().join("rezure-test-ini-no-ext-folder");
        assert_eq!(enabled_extensions(&nowhere).len(), EXTENSIONS.len());
    }

    /// A user's own edits live in this file, so a switch or a re-install
    /// must never rewrite it.
    #[test]
    fn ensure_cli_php_ini_leaves_an_existing_ini_alone() {
        let php_dir =
            std::env::temp_dir().join(format!("rezure-test-cli-ini-keep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&php_dir);
        fs::create_dir_all(&php_dir).unwrap();

        let ini_path = php_dir.join("php.ini");
        fs::write(&ini_path, "; hand-tuned\nmemory_limit = 2G\n").unwrap();

        assert!(ensure_cli_php_ini(&php_dir).unwrap().is_none());
        assert_eq!(
            fs::read_to_string(&ini_path).unwrap(),
            "; hand-tuned\nmemory_limit = 2G\n"
        );

        let _ = fs::remove_dir_all(&php_dir);
    }
}
