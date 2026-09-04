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
//!
//! Neither of those is a place a *user* can configure PHP, which is what
//! [`conf_d`] is for. The generated copy is rewritten on every start, and
//! the install-folder copy isn't read by the web SAPI at all (that one is
//! started with `-c` naming the generated ini), so before this folder
//! existed there was no edit a user could make that both survived a start
//! and reached a web request. `PHP_INI_SCAN_DIR` gives them one.

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
    // Every modern Laravel stack that touches formatting or localisation
    // wants this — Filament hard-requires `ext-intl`, and a project that
    // needs it and doesn't have it fails as a blank 500 with the reason
    // buried in `laravel.log`, which is the worst shape a missing default
    // can take. The official Windows zip already carries both the DLL and
    // the ICU libraries it links against, so enabling it costs nothing that
    // isn't already on disk.
    "intl",
    "mbstring",
    "mysqli",
    "openssl",
    "pdo_mysql",
    // Laravel 11's default `.env` uses SQLite (a local file, no server
    // needed) until a project's config points it at MariaDB instead.
    "pdo_sqlite",
    // Not in the official zip — `services::php_ext` installs it on request.
    // Listed here so that the moment its DLL lands in a version's `ext/`, the
    // generated ini turns it on by itself; versions without it are unaffected,
    // since every name here is filtered against what's really on disk.
    "redis",
    "sqlite3",
    "zip",
];

/// The CA bundle's name inside [`paths::etc`].
const CA_BUNDLE_FILE: &str = "cacert.pem";

/// The environment variable PHP reads to find *extra* ini files, on top of
/// whichever one it was told to load.
///
/// Every PHP process Rezure spawns is given this, and [`super::php_path`]
/// sets it machine-wide when the PATH switch is on, so the user's own
/// terminal reads the same folder.
pub const SCAN_DIR_ENV: &str = "PHP_INI_SCAN_DIR";

/// Dropped into [`conf_d`] the first time it's created. Not an `.ini`, so
/// PHP never tries to parse it.
const CONF_D_README: &str = "This folder is yours. Rezure never writes into it.

Any .ini file here is loaded after Rezure's generated php.ini and overrides
it, for both the web server and the `php` you run in a terminal. Files are
read in alphabetical order, so a `90-` prefix wins over a `10-` one.

Example - save this as 90-local.ini:

    memory_limit = 1G
    extension=intl

Rezure's own php.ini is regenerated on every start; edits made there are
lost. This is the file to edit instead.
";

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

/// The CA bundle both TLS stacks get pointed at, when one is installed.
///
/// It lives in `etc/` rather than beside a PHP build because it is not a
/// property of any one version: a bundle installed once has to keep working
/// across a version switch, and both copies of the ini name the same file.
fn ca_bundle() -> Option<PathBuf> {
    let path = paths::etc().ok()?.join(CA_BUNDLE_FILE);
    path.is_file().then_some(path)
}

/// The one folder a user's own PHP settings live in.
///
/// Deliberately outside the version folders. `bin/php/<version>/php.ini` is
/// per-version, so a setting written there is gone after the next switch,
/// and it is not read by the FastCGI service at all — that one is started
/// with `-c` naming the generated ini, which makes the install-folder copy
/// invisible to every web request. One folder shared by every version also
/// means `php -m` in a terminal and a web request can't disagree about
/// which settings are in force.
///
/// What sharing it costs: a fragment enabling an extension some older
/// version doesn't ship makes *that* version warn on startup. That is the
/// user's own file, in a folder they opened to write it — visible in a way
/// a silently-discarded setting never was.
pub fn conf_d() -> Result<PathBuf, AppError> {
    Ok(paths::etc()?.join("php").join("conf.d"))
}

/// [`conf_d`], created if it isn't there yet, with the note explaining what
/// it's for.
///
/// PHP is handed this path whether or not anything is in it: an empty scan
/// directory costs nothing, and creating it up front is what makes the
/// folder discoverable — a user who is told to "put your settings in
/// etc/php/conf.d" should find it already waiting, not have to guess that
/// creating it is allowed.
pub fn ensure_conf_d() -> Result<PathBuf, AppError> {
    let dir = conf_d()?;
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;

    // Best-effort: the folder is what PHP needs, the note is a courtesy.
    let readme = dir.join("README.txt");
    if !readme.exists() {
        let _ = fs::write(&readme, CONF_D_README);
    }

    Ok(dir)
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

/// Extension names the user's own `conf.d` fragments already enable.
///
/// Found the hard way: someone who enabled `intl` in `conf.d` before Rezure
/// made it a default ends up with it enabled twice, and PHP answers every
/// single start with `Module "intl" is already loaded`. On the CLI that is
/// noise; under FastCGI it is bytes emitted ahead of the response, the same
/// class of failure `output_buffering` is set for.
///
/// Both spellings PHP accepts are recognised, since a fragment copied out of
/// an old php.ini says `extension=php_intl.dll` where a modern one says
/// `extension=intl`.
fn user_enabled_extensions(conf_d: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(conf_d) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ini"))
        .filter_map(|path| fs::read_to_string(path).ok())
        .flat_map(|content| {
            content
                .lines()
                .map(str::trim)
                .filter(|line| !line.starts_with(';'))
                .filter_map(|line| line.strip_prefix("extension"))
                .filter_map(|rest| rest.trim_start().strip_prefix('='))
                .map(|value| {
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_start_matches("php_")
                        .trim_end_matches(".dll")
                        .to_lowercase()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The directives themselves, shared by both copies.
///
/// `extension_dir` is always absolute: PHP resolves a relative one against
/// the *working directory* of whatever invoked it, so `php artisan` run
/// from a project folder would look for the DLLs under that project, find
/// none, and load no extensions at all.
fn render(extension_dir: &Path, tmp: &Path, ca_bundle: Option<&Path>, conf_d: &Path) -> String {
    // Not decoration: this file is regenerated under the user's feet, so the
    // one thing it owes whoever opens it is where their own settings go.
    let mut ini = format!(
        "; Generated by Rezure - rewritten every time PHP starts, so edits here are lost.\n\
         ; Put your own settings in:\n\
         ;   {conf_d}\n\
         ; Any .ini file there is loaded after this one and overrides it.\n\n",
        conf_d = ini_value(conf_d)
    );
    ini.push_str(&format!(
        "extension_dir = \"{}\"\n",
        ini_value(extension_dir)
    ));
    // Anything the user already turned on in `conf.d` is left to their file:
    // enabling it here too would only earn an "already loaded" warning on
    // every start.
    let user_enabled = user_enabled_extensions(conf_d);
    for extension in enabled_extensions(extension_dir) {
        if user_enabled.contains(&extension.to_lowercase()) {
            continue;
        }
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
    // The Windows PHP zip ships no CA store, and neither directive has a
    // usable built-in default there, so out of the box libcurl and OpenSSL
    // cannot verify any certificate at all: every outbound HTTPS call dies
    // with "cURL error 60: unable to get local issuer certificate". That is
    // not a niche path, it is Composer, Laravel's Http client and every API
    // a project talks to. Both stacks are named because they are separate:
    // curl.cainfo covers ext/curl, openssl.cafile covers the stream
    // wrappers, so file_get_contents("https://...") verifies too.
    //
    // Written only when the bundle is really on disk: a cainfo naming a
    // file that is not there is a failure of its own, and a worse one to
    // read than the default.
    if let Some(bundle) = ca_bundle {
        ini.push_str(&format!("curl.cainfo = \"{}\"\n", ini_value(bundle)));
        ini.push_str(&format!("openssl.cafile = \"{}\"\n", ini_value(bundle)));
    }
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
    fs::write(
        &ini_path,
        render(
            &extension_dir(php_dir),
            &tmp,
            ca_bundle().as_deref(),
            &ensure_conf_d()?,
        ),
    )
    .map_err(|e| AppError::Io(format!("could not write {}: {e}", ini_path.display())))?;

    Ok(ini_path)
}

/// Writes `php.ini` into a PHP install folder, so the `php` a user types
/// into their own terminal loads the same extensions Rezure's own
/// processes get. Returns the path when one was written or repaired, and
/// `None` when the install's ini was already correct.
///
/// Never overwritten wholesale: this file sits in a folder the user can open,
/// and a hand-tuned ini (a raised `memory_limit`, an extra extension, Xdebug)
/// is theirs to keep. Only `extension_dir` is corrected, by
/// [`repair_extension_dir`].
///
/// That correction exists because the obvious assumption turned out to be
/// wrong. This ini sits beside the very `php.exe` it configures, so its
/// `extension_dir` looks like it cannot drift — but the value is *absolute*,
/// and moving the install folder carries the file along while leaving the path
/// inside it pointing at where the folder used to be. PHP answers that by
/// loading no extensions at all.
pub fn ensure_cli_php_ini(php_dir: &Path) -> Result<Option<PathBuf>, AppError> {
    let ini_path = php_dir.join("php.ini");
    if ini_path.exists() {
        // Present, but not necessarily still correct — see below.
        return repair_extension_dir(php_dir).map(|repaired| repaired.then_some(ini_path));
    }

    let tmp = ensure_tmp_dir()?;
    fs::write(
        &ini_path,
        render(
            &extension_dir(php_dir),
            &tmp,
            ca_bundle().as_deref(),
            &ensure_conf_d()?,
        ),
    )
    .map_err(|e| AppError::Io(format!("could not write {}: {e}", ini_path.display())))?;

    Ok(Some(ini_path))
}

/// Adds `extension=<name>` to an install's own `php.ini` when it isn't already
/// there, and reports whether it had to.
///
/// Additive only, and deliberately so. The generated ini needs nothing like
/// this — it is rewritten from the real contents of `ext/` on every start
/// — but the copy inside the version folder is written once and then left to
/// the user forever ([`ensure_cli_php_ini`]). Without this, installing an
/// extension would work for served sites and appear to have done nothing at
/// all in a terminal.
///
/// A commented-out line does not count as enabled: `;extension=redis` is what
/// a file looks like after someone turned it off on purpose, and the answer to
/// that is to add the real line, not to edit theirs.
pub fn ensure_extension_enabled(php_dir: &Path, name: &str) -> Result<bool, AppError> {
    let ini_path = php_dir.join("php.ini");
    let Ok(existing) = fs::read_to_string(&ini_path) else {
        // No ini yet: the next `ensure_cli_php_ini` writes one that already
        // lists every extension present in `ext/`, including this one.
        return Ok(false);
    };

    let wanted = format!("extension={name}");
    let already = existing
        .lines()
        .map(str::trim)
        .any(|line| !line.starts_with(';') && line.replace(' ', "") == wanted);
    if already {
        return Ok(false);
    }

    let separator = if existing.ends_with('\n') { "" } else { "\n" };
    fs::write(&ini_path, format!("{existing}{separator}{wanted}\n"))
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", ini_path.display())))?;

    log::info!("enabled {name} in {}", ini_path.display());
    Ok(true)
}

/// Reads the directory an existing ini's `extension_dir` names, if it has one.
fn declared_extension_dir(ini: &str) -> Option<&str> {
    ini.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with(';'))
        .find_map(|line| {
            let value = line.strip_prefix("extension_dir")?.trim_start();
            let value = value.strip_prefix('=')?.trim();
            Some(value.trim_matches('"').trim())
        })
}

/// Points a stale `extension_dir` back at the DLLs it was meant to name.
///
/// This ini lives *inside* the version folder it configures, so moving that
/// folder carries the file along with an absolute path baked into it. PHP then
/// loads no extensions at all — `curl`, `mbstring` and `pdo_mysql` simply
/// vanish, and every startup warning names a directory that isn't there.
///
/// Only that one directive is rewritten. The rest of the file may well be the
/// user's own, and [`ensure_cli_php_ini`] deliberately never clobbers it.
///
/// A relative value is repaired too: PHP resolves one against the *working
/// directory* of whatever invoked it, so a bare `ext` breaks the moment
/// `php artisan` is run from a project folder — the exact case this ini exists
/// to serve.
pub fn repair_extension_dir(php_dir: &Path) -> Result<bool, AppError> {
    let ini_path = php_dir.join("php.ini");
    let Ok(existing) = fs::read_to_string(&ini_path) else {
        return Ok(false);
    };

    let correct = extension_dir(php_dir);
    // Nothing to point at — a half-installed version. Leave it alone rather
    // than writing a path that is just as wrong in a different way.
    if !correct.is_dir() {
        return Ok(false);
    }

    let Some(declared) = declared_extension_dir(&existing) else {
        return Ok(false);
    };
    let declared_path = Path::new(declared);
    if declared_path.is_absolute() && declared_path.is_dir() {
        return Ok(false);
    }

    let replacement = format!("extension_dir = \"{}\"", ini_value(&correct));
    let repaired = existing
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with(';') && trimmed.starts_with("extension_dir") {
                replacement.as_str()
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(&ini_path, format!("{repaired}\n"))
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", ini_path.display())))?;

    log::info!(
        "repaired extension_dir in {}: {declared} -> {}",
        ini_path.display(),
        ini_value(&correct)
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ensure_php_ini` writes one fixed path by design, so every test that
    /// calls it has to take a turn: run in parallel, one test reads the file
    /// while another is still writing it and sees a truncated ini. The failure
    /// looks like a missing extension, which is a lie about the code.
    fn ini_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A PHP folder with an `ext/` directory and the ini content given.
    fn fake_php_dir(label: &str, ini: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rezure-test-inirepair-{}-{label}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("ext")).unwrap();
        fs::write(dir.join("php.ini"), ini).unwrap();
        dir
    }

    #[test]
    fn a_stale_extension_dir_is_repointed_at_the_real_ext_folder() {
        let dir = fake_php_dir(
            "stale",
            "extension_dir = \"C:/gone/away/ext\"\nextension=curl\nmemory_limit = 256M\n",
        );

        assert!(repair_extension_dir(&dir).unwrap(), "must report a repair");

        let content = fs::read_to_string(dir.join("php.ini")).unwrap();
        assert!(content.contains(&ini_value(&dir.join("ext"))));
        assert!(!content.contains("C:/gone/away/ext"));
        // Everything else the file said has to survive.
        assert!(content.contains("extension=curl"));
        assert!(content.contains("memory_limit = 256M"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_extension_dir_is_made_absolute() {
        // PHP resolves a relative one against the caller's working directory,
        // so `php artisan` from a project folder would load nothing.
        let dir = fake_php_dir("relative", "extension_dir = \"ext\"\n");
        assert!(repair_extension_dir(&dir).unwrap());
        assert!(fs::read_to_string(dir.join("php.ini"))
            .unwrap()
            .contains(&ini_value(&dir.join("ext"))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_correct_extension_dir_is_left_untouched() {
        let dir = fake_php_dir("ok", "placeholder\n");
        let good = format!(
            "extension_dir = \"{}\"\nextension=curl\n",
            ini_value(&dir.join("ext"))
        );
        fs::write(dir.join("php.ini"), &good).unwrap();

        assert!(!repair_extension_dir(&dir).unwrap(), "nothing to repair");
        assert_eq!(fs::read_to_string(dir.join("php.ini")).unwrap(), good);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_commented_out_extension_dir_is_not_treated_as_the_setting() {
        let dir = fake_php_dir("commented", "");
        let ini = format!(
            ";extension_dir = \"C:/gone/ext\"\nextension_dir = \"{}\"\n",
            ini_value(&dir.join("ext"))
        );
        fs::write(dir.join("php.ini"), &ini).unwrap();

        assert!(!repair_extension_dir(&dir).unwrap());
        assert_eq!(fs::read_to_string(dir.join("php.ini")).unwrap(), ini);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_php_ini_enables_every_required_extension() {
        let _guard = ini_guard();
        let fake_php_exe = std::env::temp_dir()
            .join(format!("rezure-test-phpini-{}", std::process::id()))
            .join("php.exe");

        let ini_path = ensure_php_ini(&fake_php_exe).unwrap();
        let content = fs::read_to_string(&ini_path).unwrap();

        // A default the user's own conf.d already enables is deliberately left
        // out of this file - see `user_enabled_extensions`. The rule is
        // "enabled somewhere", not "enabled here".
        let user_enabled = user_enabled_extensions(&conf_d().unwrap());
        for extension in EXTENSIONS {
            assert!(
                content.contains(&format!("extension={extension}"))
                    || user_enabled.contains(&extension.to_lowercase()),
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
        let _guard = ini_guard();
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

    /// Filament and half of Laravel's formatting helpers need `intl`, and
    /// the ICU libraries it links against ship in the same zip as the DLL.
    /// A version that predates it still gets a clean ini, because the list
    /// is filtered against what's really in `ext/`.
    #[test]
    fn intl_is_on_by_default_but_only_where_the_build_ships_it() {
        assert!(EXTENSIONS.contains(&"intl"), "intl must be a default");

        let php_dir =
            std::env::temp_dir().join(format!("rezure-test-ini-intl-{}", std::process::id()));
        let ext = php_dir.join("ext");
        let _ = fs::remove_dir_all(&php_dir);
        fs::create_dir_all(&ext).unwrap();
        fs::write(ext.join("php_intl.dll"), "").unwrap();

        assert!(enabled_extensions(&ext).contains(&"intl".to_string()));
        // The same list against a build without it must not name it.
        fs::remove_file(ext.join("php_intl.dll")).unwrap();
        assert!(!enabled_extensions(&ext).contains(&"intl".to_string()));

        let _ = fs::remove_dir_all(&php_dir);
    }

    /// A version still being unpacked has no `ext/` to inspect, and an ini
    /// listing nothing would be worse than one listing too much.
    #[test]
    fn a_missing_ext_folder_falls_back_to_the_full_list() {
        let nowhere = std::env::temp_dir().join("rezure-test-ini-no-ext-folder");
        assert_eq!(enabled_extensions(&nowhere).len(), EXTENSIONS.len());
    }

    /// Without these, every HTTPS call out of PHP fails with cURL error 60,
    /// because the Windows build carries no CA store of its own.
    #[test]
    fn render_points_both_tls_stacks_at_the_ca_bundle() {
        let dir = std::env::temp_dir().join("rezure-test-ini-ca");
        let bundle = dir.join(CA_BUNDLE_FILE);

        let content = render(&dir.join("ext"), &dir, Some(&bundle), &dir.join("conf.d"));

        let expected = ini_value(&bundle);
        assert!(
            content.contains(&format!("curl.cainfo = \"{expected}\"")),
            "missing curl.cainfo, got: {content}"
        );
        assert!(
            content.contains(&format!("openssl.cafile = \"{expected}\"")),
            "missing openssl.cafile, got: {content}"
        );
        assert!(!content.contains('\\'), "paths must use forward slashes");
    }

    /// Naming a bundle that is not on disk is an error of its own, so the
    /// pair is left out entirely rather than written blind.
    #[test]
    fn render_omits_the_ca_directives_when_no_bundle_is_installed() {
        let dir = std::env::temp_dir().join("rezure-test-ini-no-ca");

        let content = render(&dir.join("ext"), &dir, None, &dir.join("conf.d"));

        assert!(!content.contains("curl.cainfo"));
        assert!(!content.contains("openssl.cafile"));
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

    /// Installing an extension has to reach the terminal `php` too, and that
    /// one reads an ini Rezure wrote once and never rewrites.
    #[test]
    fn an_installed_extension_is_appended_to_an_existing_cli_ini() {
        let dir = fake_php_dir(
            "addext",
            "extension_dir = \"C:/php/ext\"
extension=curl
",
        );

        assert!(ensure_extension_enabled(&dir, "redis").unwrap(), "must add");
        let content = fs::read_to_string(dir.join("php.ini")).unwrap();
        assert!(content.contains("extension=redis"));
        // The user's file is added to, never rewritten.
        assert!(content.contains("extension=curl"));

        // Idempotent: a second install attempt must not add a second line.
        assert!(!ensure_extension_enabled(&dir, "redis").unwrap());
        assert_eq!(content.matches("extension=redis").count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    /// `;extension=redis` is what a file looks like after someone turned it
    /// off deliberately. Uncommenting their line would be editing their
    /// decision; adding the real line is not.
    #[test]
    fn a_commented_out_extension_does_not_count_as_enabled() {
        let dir = fake_php_dir(
            "addext-commented",
            ";extension=redis
",
        );

        assert!(ensure_extension_enabled(&dir, "redis").unwrap());
        let content = fs::read_to_string(dir.join("php.ini")).unwrap();
        assert!(content.contains(";extension=redis"), "their line survives");
        assert!(content.lines().any(|line| line == "extension=redis"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// The duplicate this prevents is not hypothetical: anyone who followed
    /// the advice to enable an extension in `conf.d` before Rezure made it a
    /// default gets `Module "intl" is already loaded` on every start.
    #[test]
    fn an_extension_the_user_already_enabled_is_not_enabled_twice() {
        let dir =
            std::env::temp_dir().join(format!("rezure-test-ini-dup-{}-confd", std::process::id()));
        let ext = dir.join("ext");
        let conf_d = dir.join("conf.d");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&ext).unwrap();
        fs::create_dir_all(&conf_d).unwrap();
        for dll in ["php_intl.dll", "php_curl.dll"] {
            fs::write(ext.join(dll), "").unwrap();
        }
        // Their file, in both spellings PHP accepts.
        fs::write(conf_d.join("10-intl.ini"), "extension=intl\n").unwrap();
        fs::write(conf_d.join("20-curl.ini"), ";extension=zip\n").unwrap();

        let content = render(&ext, &dir, None, &conf_d);

        assert!(
            !content.contains("extension=intl"),
            "conf.d already enables intl, got: {content}"
        );
        // A commented-out fragment line is not an enable, so curl still has to
        // come from here.
        assert!(content.contains("extension=curl"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// The old spelling has to be recognised too, or the duplicate comes back
    /// for anyone whose fragment was copied out of a full php.ini.
    #[test]
    fn the_php_dll_spelling_counts_as_enabled_too() {
        let dir =
            std::env::temp_dir().join(format!("rezure-test-ini-dup2-{}-confd", std::process::id()));
        let ext = dir.join("ext");
        let conf_d = dir.join("conf.d");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&ext).unwrap();
        fs::create_dir_all(&conf_d).unwrap();
        fs::write(ext.join("php_intl.dll"), "").unwrap();
        fs::write(conf_d.join("intl.ini"), "extension = \"php_intl.dll\"").unwrap();

        assert!(!render(&ext, &dir, None, &conf_d).contains("extension=intl"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// Prints the ini Rezure would really hand PHP right now, for the version
    /// that is actually active, against the real `conf.d`. Run with:
    /// `cargo test --lib services::php_ini::tests::print_generated_ini -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_generated_ini() {
        let Ok(php_exe) = crate::services::php::active_exe() else {
            println!("no PHP installed - nothing to generate");
            return;
        };
        let path = ensure_php_ini(&php_exe).unwrap();
        println!(
            "{}
---",
            path.display()
        );
        println!("{}", fs::read_to_string(&path).unwrap());
        println!(
            "--- conf.d already enables: {:?}",
            user_enabled_extensions(&conf_d().unwrap())
        );
    }

    /// The generated file is disposable, so it has to name the folder that
    /// isn't — otherwise a user editing it has no way to know their change
    /// will be gone by the next start.
    #[test]
    fn the_generated_ini_points_at_the_folder_the_user_owns() {
        let dir = std::env::temp_dir().join("rezure-test-ini-confd-header");
        let conf_d = dir.join("conf.d");

        let content = render(&dir.join("ext"), &dir, None, &conf_d);

        let first = content.lines().next().unwrap_or_default();
        assert!(
            first.starts_with(';'),
            "the header must be a comment: {first}"
        );
        assert!(
            content.contains(&ini_value(&conf_d)),
            "the header must name the conf.d folder, got: {content}"
        );
        // A comment must not read as a setting — `extension_dir` is parsed
        // back out of this file by `repair_extension_dir`.
        assert_eq!(
            declared_extension_dir(&content),
            Some(ini_value(&dir.join("ext")).as_str()),
            "the header must not shadow the real extension_dir"
        );
    }

    /// PHP is handed this path on every spawn, so it has to exist before the
    /// first one — and the note is what makes the empty folder explain
    /// itself to whoever opens it.
    #[test]
    fn ensure_conf_d_creates_the_folder_and_explains_it() {
        let dir = ensure_conf_d().unwrap();

        assert!(dir.is_dir(), "{} must exist", dir.display());
        assert!(dir.ends_with("conf.d"));
        let readme = dir.join("README.txt");
        assert!(readme.is_file(), "the note must be there");
        // Not an .ini, or PHP would try to parse it as settings.
        assert!(fs::read_to_string(&readme)
            .unwrap()
            .contains("90-local.ini"));

        // Idempotent, since every spawn calls it.
        assert_eq!(ensure_conf_d().unwrap(), dir);
    }

    /// A user's own fragment must never be overwritten by a start, which is
    /// the whole failure this folder exists to end.
    #[test]
    fn regenerating_the_ini_leaves_a_users_fragment_alone() {
        let _guard = ini_guard();
        let conf_d = ensure_conf_d().unwrap();
        let fragment = conf_d.join("99-rezure-test.ini");
        fs::write(&fragment, "memory_limit = 1G\n").unwrap();

        let fake_php_exe = std::env::temp_dir()
            .join(format!("rezure-test-confd-keep-{}", std::process::id()))
            .join("php.exe");
        let ini_path = ensure_php_ini(&fake_php_exe).unwrap();

        assert_eq!(
            fs::read_to_string(&fragment).unwrap(),
            "memory_limit = 1G\n",
            "a start must not touch the user's own settings"
        );

        let _ = fs::remove_file(&fragment);
        let _ = fs::remove_file(&ini_path);
    }
}
