//! Turns on/off extensions the *official* PHP zip already ships in `ext/`.
//!
//! Distinct from [`super::php_ext`], which downloads extensions that aren't
//! in the zip at all (PECL, e.g. `redis`) — this is only about DLLs already
//! sitting on disk that [`super::php_ini`] simply never writes an
//! `extension=` line for. Before this existed, turning one on meant knowing
//! its exact `extension=` spelling and hand-editing a `.ini` fragment in
//! `conf.d`.
//!
//! [`CATALOG`] is the single source of truth for which extensions are
//! default-on: `php_ini`'s generated file no longer hardcodes that list, it
//! asks [`enabled_ids`] for it, which is `default_on` entries plus whatever
//! the user overrode. That keeps this table and the generated ini from
//! drifting apart, which two separate lists saying the same thing eventually
//! would.
//!
//! # Where a user's choice is stored
//!
//! One JSON file per PHP version — `data/php/<version>/extensions.json` —
//! **not** `conf.d`. `conf.d` is documented as the one folder Rezure never
//! writes into (see `php-versions.md`); a toggle flipped from the UI is
//! Rezure's own write, so it belongs beside the other state Rezure manages
//! for that install, keyed the same way `php_ext` keys installed PECL DLLs:
//! by version, because a build's `ext/` contents differ version to version.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::php;
use crate::utils::error::AppError;
use crate::utils::paths;

#[derive(Debug, Clone, Copy)]
pub struct ExtensionMeta {
    /// Both the `ext/php_<id>.dll` stem and the `extension=<id>` value.
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    /// One sentence, shown in the UI — why a project would want it.
    pub description: &'static str,
    /// Enabled unless the user explicitly turned it off. These are exactly
    /// the extensions the generated ini auto-enabled before this feature
    /// existed — turning this table into the source of truth for that list,
    /// rather than keeping a second one in `php_ini`, means there is nowhere
    /// left for the two to disagree.
    pub default_on: bool,
    /// Environment-dependent or PHP-internal-only — never defaulted on, and
    /// the UI explains why rather than presenting it as an ordinary choice.
    pub debug_only: bool,
    /// True only for `opcache`: PHP loads it with `zend_extension=`, not the
    /// plain `extension=` every other entry here uses — it hooks the engine
    /// itself rather than registering as a normal module. [`super::php_ini`]
    /// reads this to pick the right directive.
    pub zend_extension: bool,
}

/// Every bundled extension Rezure knows a name and description for. A DLL
/// this doesn't cover simply doesn't appear in the toggle UI — no different
/// from being unable to enable it today.
pub const CATALOG: &[ExtensionMeta] = &[
    // -- Default-on: what `php_ini` auto-enabled before this table existed --
    ExtensionMeta {
        id: "curl",
        label: "cURL",
        category: "Network",
        description: "HTTP client used by Composer, Guzzle, and Laravel's HTTP client.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "fileinfo",
        label: "Fileinfo",
        category: "Filesystem",
        description: "Detects file MIME types — Laravel's upload validation depends on it.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "gd",
        label: "GD",
        category: "Image",
        description: "Image creation and resizing without an external library.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "intl",
        label: "Intl",
        category: "Format",
        description: "Locale-aware formatting and collation — Filament hard-requires it.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "mbstring",
        label: "Mbstring",
        category: "Format",
        description: "Multibyte string handling — needed by nearly every modern PHP framework.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "mysqli",
        label: "MySQLi",
        category: "Database",
        description: "Procedural MySQL driver, for tools that don't go through PDO.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "openssl",
        label: "OpenSSL",
        category: "Network",
        description: "TLS support — outbound HTTPS calls fail without it.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "pdo_mysql",
        label: "PDO MySQL",
        category: "Database",
        description: "MySQL/MariaDB driver for PDO — what Laravel's database layer uses.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "pdo_sqlite",
        label: "PDO SQLite",
        category: "Database",
        description: "SQLite driver for PDO — Laravel 11's default local database.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "sqlite3",
        label: "SQLite3",
        category: "Database",
        description: "Native SQLite driver.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "zip",
        label: "Zip",
        category: "Format",
        description: "Reads and writes zip archives — Composer needs it to unpack packages.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "redis",
        label: "Redis",
        category: "Database",
        description: "Redis client, installed separately via the requirements check — toggled here once its DLL is on disk.",
        default_on: true,
        debug_only: false,
        zend_extension: false,
    },
    // -- Off by default: bundled, but not everyone needs them --
    ExtensionMeta {
        id: "bz2",
        label: "Bzip2",
        category: "Format",
        description: "Reads and writes .bz2 compressed files.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "sodium",
        label: "Sodium",
        category: "Security",
        description: "Modern encryption primitives — Laravel's encrypter ships its own fallback.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "exif",
        label: "Exif",
        category: "Image",
        description: "Reads image metadata (orientation, camera data) for upload pipelines.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "xsl",
        label: "XSL",
        category: "Format",
        description: "XSLT transformations for XML documents.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "sockets",
        label: "Sockets",
        category: "Network",
        description: "Low-level raw socket access for long-running daemons and custom protocols.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "ldap",
        label: "LDAP",
        category: "Network",
        description: "Directory service authentication — enterprise SSO integrations.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "imap",
        label: "IMAP",
        category: "Network",
        description: "Reads mailboxes over IMAP/POP3 for legacy mail-processing projects.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "gmp",
        label: "GMP",
        category: "Math",
        description: "Arbitrary-precision arithmetic, required by some crypto libraries.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "bcmath",
        label: "BCMath",
        category: "Math",
        description: "Arbitrary-precision decimal math for billing and financial calculations.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "calendar",
        label: "Calendar",
        category: "Format",
        description: "Calendar conversion functions, rarely needed outside date-heavy niche apps.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "ffi",
        label: "FFI",
        category: "System",
        description: "Calls into native C libraries directly from PHP.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "gettext",
        label: "Gettext",
        category: "Format",
        description: "GNU gettext translation catalogs, an alternative to array-based translations.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "odbc",
        label: "ODBC",
        category: "Database",
        description: "Generic database access via an ODBC driver.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "pdo_odbc",
        label: "PDO ODBC",
        category: "Database",
        description: "ODBC driver for PDO.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "pdo_pgsql",
        label: "PDO PostgreSQL",
        category: "Database",
        description: "PostgreSQL driver for PDO.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "pgsql",
        label: "PostgreSQL",
        category: "Database",
        description: "Procedural PostgreSQL driver.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "shmop",
        label: "Shmop",
        category: "System",
        description: "Shared memory access between PHP processes.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "snmp",
        label: "SNMP",
        category: "Network",
        description: "Queries network devices over SNMP.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "soap",
        label: "SOAP",
        category: "Network",
        description: "SOAP client and server for legacy enterprise web services.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "tidy",
        label: "Tidy",
        category: "Format",
        description: "Cleans up and validates malformed HTML.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "xmlrpc",
        label: "XML-RPC",
        category: "Network",
        description: "XML-RPC client and server, a legacy alternative to SOAP.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "ftp",
        label: "FTP",
        category: "Network",
        description: "FTP and FTPS client functions.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "dba",
        label: "DBA",
        category: "Database",
        description: "Key-value access to dbm-style flat-file databases (Berkeley DB, GDBM, and similar).",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "com_dotnet",
        label: "COM/.NET",
        category: "System",
        description: "Talks to Windows COM objects and .NET assemblies — Windows-only by nature.",
        default_on: false,
        debug_only: false,
        zend_extension: false,
    },
    // -- Performance: changes runtime behavior enough to deserve its own
    // category rather than living under Database/Network/Format --
    ExtensionMeta {
        id: "opcache",
        label: "OPcache",
        category: "Performance",
        description: "Caches compiled bytecode for faster execution — off by default locally, since a stale cache can hide code changes until it's invalidated.",
        default_on: false,
        debug_only: false,
        // The one entry that actually needs this: OPcache hooks the engine
        // itself, so PHP only recognizes it behind `zend_extension=`, not
        // the plain `extension=` every other entry here uses.
        zend_extension: true,
    },
    // -- Debug-only / environment-dependent: never defaulted on --
    ExtensionMeta {
        id: "oci8",
        label: "OCI8",
        category: "Debug",
        description: "Oracle driver that needs Oracle's own client libraries, which Rezure doesn't bundle.",
        default_on: false,
        debug_only: true,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "pdo_oci",
        label: "PDO OCI",
        category: "Debug",
        description: "Oracle driver for PDO — same external client-library requirement as OCI8.",
        default_on: false,
        debug_only: true,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "zend_test",
        label: "Zend Test",
        category: "Debug",
        description: "Internal test harness for the Zend Engine itself, not for applications.",
        default_on: false,
        debug_only: true,
        zend_extension: false,
    },
    ExtensionMeta {
        id: "phpdbg_webhelper",
        label: "phpdbg Web Helper",
        category: "Debug",
        description: "Supports phpdbg's web SAPI mode, unused by the FastCGI setup Rezure runs.",
        default_on: false,
        debug_only: true,
        zend_extension: false,
    },
];

/// What the UI needs for one extension, for one PHP version.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToggle {
    pub id: String,
    pub label: String,
    pub category: String,
    pub description: String,
    /// Whether `php_ini` will write `extension=<id>` for this version right
    /// now — `default_on` unless the user overrode it.
    pub enabled: bool,
    pub default_on: bool,
    pub debug_only: bool,
    /// The DLL isn't in this version's `ext/` at all — shown disabled and
    /// greyed out rather than pretending a toggle here would do anything.
    pub available: bool,
}

pub fn find(id: &str) -> Option<&'static ExtensionMeta> {
    CATALOG.iter().find(|ext| ext.id == id)
}

/// `<data>/php/<version>/extensions.json` — see the module doc for why this
/// isn't in `conf.d`.
fn state_path(php_version: &str) -> Result<PathBuf, AppError> {
    Ok(paths::data()?
        .join("php")
        .join(php_version)
        .join("extensions.json"))
}

/// The user's explicit choices for one PHP version — only the entries where
/// they disagreed with [`ExtensionMeta::default_on`]. Best-effort: a missing
/// or corrupt file reads as "nothing overridden yet" rather than an error,
/// the same way a first run has no file at all.
fn load_overrides(php_version: &str) -> HashMap<String, bool> {
    let path = match state_path(php_version) {
        Ok(path) => path,
        Err(_) => return HashMap::new(),
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_overrides(php_version: &str, overrides: &HashMap<String, bool>) -> Result<(), AppError> {
    let path = state_path(php_version)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    }
    let json = serde_json::to_string_pretty(overrides)
        .map_err(|e| AppError::Io(format!("could not encode {}: {e}", path.display())))?;
    std::fs::write(&path, json)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))
}

/// The ids `php_ini`'s generated file should turn on for this install:
/// [`CATALOG`]'s `default_on` entries, with the user's overrides applied —
/// the "default list ∪ user-enabled − user-disabled" this feature exists
/// for. Independent of what's really in `ext/`; `php_ini` filters that in
/// afterwards, same as it always has.
///
/// Best-effort by design: called on every PHP start, so a version with no
/// override file yet (the common case) or a version nobody has scanned
/// (`php_dir` unresolved) must fall through to the plain defaults rather
/// than block a start on it.
pub fn enabled_ids(php_version: &str) -> Vec<&'static str> {
    let overrides = load_overrides(php_version);
    CATALOG
        .iter()
        .filter(|ext| overrides.get(ext.id).copied().unwrap_or(ext.default_on))
        .map(|ext| ext.id)
        .collect()
}

/// [`CATALOG`]'s `default_on` entries, with no version's overrides applied —
/// what [`enabled_ids`] falls back to for a PHP install `php_ini` can't
/// resolve back to a version (a folder mid-install, or a test's fake path),
/// which is exactly what the generated ini did before per-version toggles
/// existed.
pub fn default_ids() -> Vec<&'static str> {
    CATALOG
        .iter()
        .filter(|e| e.default_on)
        .map(|e| e.id)
        .collect()
}

/// The folder of an installed PHP version, by the id the Switch page uses.
fn php_dir_for(php_version: &str) -> Result<PathBuf, AppError> {
    php::installed()
        .into_iter()
        .find(|runtime| runtime.version == php_version)
        .map(|runtime| runtime.dir)
        .ok_or_else(|| AppError::PhpVersionNotFound(php_version.to_string()))
}

/// Whether `ext/php_<id>.dll` (or PHP 7's `php_<id>2.dll` GD spelling) is
/// really in this build — the same check `php_ini::enabled_extensions` does,
/// duplicated rather than shared because that one works in terms of already-
/// resolved names, not catalog ids.
fn dll_present(extension_dir: &Path, id: &str) -> bool {
    [id.to_string(), format!("{id}2")]
        .into_iter()
        .any(|candidate| extension_dir.join(format!("php_{candidate}.dll")).is_file())
}

/// Every catalog entry, answered for one PHP version: what the user chose,
/// what would apply by default, and whether the DLL is even there to enable.
pub fn status_for(php_version: &str) -> Result<Vec<ExtensionToggle>, AppError> {
    let extension_dir = php_dir_for(php_version)?.join("ext");
    let overrides = load_overrides(php_version);

    Ok(CATALOG
        .iter()
        .map(|ext| {
            let available = dll_present(&extension_dir, ext.id);
            ExtensionToggle {
                id: ext.id.to_string(),
                label: ext.label.to_string(),
                category: ext.category.to_string(),
                description: ext.description.to_string(),
                enabled: overrides.get(ext.id).copied().unwrap_or(ext.default_on),
                default_on: ext.default_on,
                debug_only: ext.debug_only,
                available,
            }
        })
        .collect())
}

/// Records the user's choice and returns the refreshed list, so the UI can't
/// end up showing a toggle in a state that didn't actually save.
///
/// Only ever writes the override file — never touches `conf.d` or the
/// generated ini directly. The change reaches PHP the next time it starts
/// and `php_ini::render` asks [`enabled_ids`] again, same as any other
/// change to what a version's `ext/` contains.
pub fn set_enabled(
    php_version: &str,
    id: &str,
    enabled: bool,
) -> Result<Vec<ExtensionToggle>, AppError> {
    let meta = find(id).ok_or_else(|| AppError::UnknownExtension(id.to_string()))?;
    // Ensures the version itself is real before writing state for it — an
    // override file for a version that was never installed would just be
    // dead weight nothing ever reads.
    php_dir_for(php_version)?;

    let mut overrides = load_overrides(php_version);
    // Kept minimal: a choice that matches the default isn't an override
    // worth remembering, and dropping it means a future change to
    // `default_on` takes effect for anyone who never touched this extension.
    if enabled == meta.default_on {
        overrides.remove(id);
    } else {
        overrides.insert(id.to_string(), enabled);
    }
    save_overrides(php_version, &overrides)?;

    status_for(php_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `paths::home()` resolves once per process and is shared by every test
    /// in this binary (see `paths::tests::the_root_is_resolved_once`), so
    /// these tests can't sandbox it per-test the way `php_ini`'s do with
    /// `std::env::temp_dir()`. Instead each test uses its own fake version
    /// string under the real, cached `data/php/` and cleans up after itself
    /// — collisions are avoided by distinct names, not isolation.
    fn cleanup(fake_version: &str) {
        if let Ok(path) = state_path(fake_version) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    #[test]
    fn every_catalog_id_is_unique() {
        let mut ids: Vec<_> = CATALOG.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            CATALOG.len(),
            "duplicate extension id in CATALOG"
        );
    }

    #[test]
    fn intl_is_a_default_and_zend_test_is_not() {
        assert!(find("intl").unwrap().default_on);
        assert!(!find("zend_test").unwrap().default_on);
        assert!(find("zend_test").unwrap().debug_only);
    }

    #[test]
    fn with_no_overrides_enabled_ids_is_exactly_the_defaults() {
        let fake_version = "0.0.0-rezure-test-no-overrides";
        cleanup(fake_version);

        let expected: Vec<_> = CATALOG
            .iter()
            .filter(|e| e.default_on)
            .map(|e| e.id)
            .collect();
        assert_eq!(enabled_ids(fake_version), expected);

        cleanup(fake_version);
    }

    #[test]
    fn setting_a_non_default_extension_on_persists_across_loads() {
        let fake_version = "0.0.0-rezure-test-sodium-on";
        cleanup(fake_version);

        let mut overrides = HashMap::new();
        overrides.insert("sodium".to_string(), true);
        save_overrides(fake_version, &overrides).unwrap();

        assert!(load_overrides(fake_version).get("sodium").copied().unwrap());
        assert!(enabled_ids(fake_version).contains(&"sodium"));

        cleanup(fake_version);
    }

    #[test]
    fn turning_a_default_back_to_its_default_drops_the_override_instead_of_storing_it() {
        let fake_version = "0.0.0-rezure-test-curl-roundtrip";
        cleanup(fake_version);

        let mut overrides = HashMap::new();
        overrides.insert("curl".to_string(), false);
        save_overrides(fake_version, &overrides).unwrap();
        assert!(!enabled_ids(fake_version).contains(&"curl"));

        // The merge logic `set_enabled` applies: re-choosing the real
        // default removes the override rather than storing it redundantly.
        let meta = find("curl").unwrap();
        let want_enabled = true;
        let mut overrides = load_overrides(fake_version);
        if want_enabled == meta.default_on {
            overrides.remove("curl");
        } else {
            overrides.insert("curl".to_string(), want_enabled);
        }
        save_overrides(fake_version, &overrides).unwrap();
        assert!(
            load_overrides(fake_version).is_empty(),
            "choosing the default again must not leave a redundant override behind"
        );

        cleanup(fake_version);
    }

    #[test]
    fn an_unknown_extension_id_is_refused_before_touching_any_php_version() {
        // The version doesn't need to exist for this to fail correctly —
        // the id is checked first, so this can't accidentally write state
        // for a version that was never installed.
        assert!(matches!(
            set_enabled("0.0.0-does-not-exist", "not-a-real-extension", true),
            Err(AppError::UnknownExtension(_))
        ));
    }

    #[test]
    fn set_enabled_rejects_a_php_version_that_isnt_installed() {
        assert!(matches!(
            set_enabled("0.0.0-rezure-test-not-installed", "sodium", true),
            Err(AppError::PhpVersionNotFound(_))
        ));
    }

    #[test]
    fn a_missing_or_corrupt_state_file_reads_as_no_overrides() {
        assert!(load_overrides("0.0.0-rezure-test-no-file-yet").is_empty());

        let fake_version = "0.0.0-rezure-test-corrupt";
        cleanup(fake_version);
        let path = state_path(fake_version).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_overrides(fake_version).is_empty());

        cleanup(fake_version);
    }
}
