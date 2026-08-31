//! Every location Rezure writes to, derived from a single root.
//!
//! # Why one visible folder
//!
//! Rezure's storage used to be split across three OS locations — runtimes and
//! service state in `%LOCALAPPDATA%`, JSON config in `%APPDATA%`, projects and
//! dumps in `%USERPROFILE%\rezure`. That is the tidy Windows convention, but
//! it is the wrong shape for this app:
//!
//! * **Path length.** Windows still enforces `MAX_PATH` (260) for most tooling,
//!   and Composer's `vendor/` trees are deep. `C:\rezure\www\` leaves ~40 more
//!   characters of headroom than a docroot under `C:\Users\<name>\rezure\www\`,
//!   which is the difference between `composer install` finishing and failing
//!   halfway with an unhelpful error.
//! * **It's meant to be opened.** Users drop runtimes into `bin`, put projects
//!   in `www`, and go looking for dumps. Laragon and XAMPP both keep that at
//!   the drive root for exactly this reason, and Rezure's users know that
//!   layout already.
//!
//! # Layout
//!
//! ```text
//! C:\rezure\
//! ├── bin\        runtimes Rezure downloaded and checksum-verified
//! ├── custom\     runtimes added by hand — drop a folder in, it's installed
//! ├── data\       service state: nginx config, MariaDB's datadir, php.ini
//! ├── etc\        settings.json, profiles.json, links.json
//! ├── current\    the PHP junction that goes on PATH
//! ├── www\        projects
//! ├── dumps\      exported .sql files
//! └── rezure.db
//! ```
//!
//! `bin` and `custom` stay separate on purpose: `InstalledRuntime::managed` is
//! derived from which of the two a runtime was found in, so merging them would
//! make every hand-added runtime claim to have been verified by Rezure.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::utils::error::AppError;

/// Overrides the root. Set by the test suite, and an escape hatch for anyone
/// who wants Rezure's data on another drive.
pub const HOME_ENV: &str = "REZURE_HOME";

/// The default root — short, at the drive root, next to `C:\laragon`.
pub const DEFAULT_HOME: &str = r"C:\rezure";

/// Resolved once per process. A root that answered differently between two
/// calls would split the app's data across two trees mid-session, so this is
/// deliberately not re-read.
static HOME: OnceLock<PathBuf> = OnceLock::new();

/// Whether a directory can actually be written to, rather than merely
/// existing. `create_dir_all` on `C:\rezure` succeeds on a machine whose
/// policy then denies writes inside it, and finding that out at the first
/// download is far worse than finding it out here.
fn writable(dir: &Path) -> bool {
    // Named per process: two instances starting at once must not delete each
    // other's probe and have one of them conclude the directory is read-only.
    // That instance would fall back to `%LOCALAPPDATA%` and quietly run against
    // a second, empty copy of everything.
    let probe = dir.join(format!(".rezure-write-probe-{}", std::process::id()));
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn resolve() -> Result<PathBuf, AppError> {
    if let Some(raw) = std::env::var_os(HOME_ENV) {
        let explicit = PathBuf::from(raw);
        fs::create_dir_all(&explicit).map_err(|e| {
            AppError::Io(format!(
                "{HOME_ENV} is set to {} but it couldn't be created: {e}",
                explicit.display()
            ))
        })?;
        return Ok(explicit);
    }

    let preferred = PathBuf::from(DEFAULT_HOME);
    if fs::create_dir_all(&preferred).is_ok() && writable(&preferred) {
        return Ok(preferred);
    }

    // A locked-down machine — a policy denying writes at the drive root, or a
    // C: that isn't writable at all. Falling back beats refusing to start;
    // everything still works, it just isn't in the nice short place.
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    let fallback = base.join("Rezure");
    fs::create_dir_all(&fallback).map_err(|e| {
        AppError::Io(format!(
            "neither {DEFAULT_HOME} nor {} could be created: {e}",
            fallback.display()
        ))
    })?;
    Ok(fallback)
}

/// The root every other path here hangs off.
pub fn home() -> Result<PathBuf, AppError> {
    if let Some(cached) = HOME.get() {
        return Ok(cached.clone());
    }
    let resolved = resolve()?;
    Ok(HOME.get_or_init(|| resolved).clone())
}

/// Creates `dir` if it isn't there yet, and hands it back.
fn ensure(dir: PathBuf) -> Result<PathBuf, AppError> {
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    Ok(dir)
}

/// Runtimes Rezure downloaded and verified.
pub fn bin() -> Result<PathBuf, AppError> {
    Ok(home()?.join("bin"))
}

/// Runtimes the user added by hand. The filesystem is the whole registry: a
/// folder appearing here *is* an installed version, and deleting it removes
/// one.
pub fn custom_bin() -> Result<PathBuf, AppError> {
    Ok(home()?.join("custom"))
}

/// Mutable service state — generated nginx config, MariaDB's datadir, the
/// php.ini Rezure writes. Distinct from `bin`, which holds the binaries as
/// they were extracted.
pub fn data() -> Result<PathBuf, AppError> {
    Ok(home()?.join("data"))
}

/// Rezure's own JSON config. Created eagerly: the config layer writes into it
/// without a separate mkdir step.
pub fn etc() -> Result<PathBuf, AppError> {
    ensure(home()?.join("etc"))
}

/// Holds the switchable PHP junction — the one directory that goes on PATH.
pub fn current() -> Result<PathBuf, AppError> {
    Ok(home()?.join("current"))
}

/// The projects root.
pub fn www() -> Result<PathBuf, AppError> {
    Ok(home()?.join("www"))
}

/// Where `export_database` writes its `.sql` files.
pub fn dumps() -> Result<PathBuf, AppError> {
    Ok(home()?.join("dumps"))
}

/// The SQLite database.
pub fn db_file() -> Result<PathBuf, AppError> {
    Ok(home()?.join("rezure.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_path_sits_under_the_one_root() {
        let root = home().expect("a root must resolve");
        for path in [
            bin().unwrap(),
            custom_bin().unwrap(),
            data().unwrap(),
            etc().unwrap(),
            current().unwrap(),
            www().unwrap(),
            dumps().unwrap(),
            db_file().unwrap(),
        ] {
            assert!(
                path.starts_with(&root),
                "{} escaped the root {}",
                path.display(),
                root.display()
            );
        }
    }

    #[test]
    fn bin_and_custom_bin_never_collide() {
        // They decide whether a runtime counts as checksum-verified, so they
        // must stay two different directories.
        assert_ne!(bin().unwrap(), custom_bin().unwrap());
    }

    #[test]
    fn the_root_is_resolved_once() {
        assert_eq!(home().unwrap(), home().unwrap());
    }

    #[test]
    fn writable_rejects_a_path_that_is_not_a_directory() {
        assert!(!writable(Path::new(r"Z:\definitely-not-a-real-drive")));
    }
}
