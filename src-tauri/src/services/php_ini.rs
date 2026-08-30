//! Generates Rezure's own `php.ini` for the bundled PHP.
//!
//! The official Windows PHP zip ships with every extension disabled by
//! default — no active `php.ini` at all, just `php.ini-development` and
//! `php.ini-production` templates nobody's told PHP to use. Composer needs
//! `openssl` just to talk to Packagist; real Laravel/WordPress installs
//! need `pdo_mysql`/`mysqli`/`mbstring`/`zip`/... Without this, PHP runs
//! but almost nothing beyond a trivial script actually works.

use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::error::AppError;

/// Enabled for every PHP process Rezure spawns — the FastCGI service
/// (`services::process`) and Composer/scaffolding (`services::scaffold`).
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

fn runtime_dir() -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base.join("Rezure").join("data").join("php"))
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

    let extension_dir = php_exe
        .parent()
        .map(|dir| dir.join("ext"))
        .ok_or_else(|| AppError::Io("php.exe has no parent directory".to_string()))?;

    let mut ini = format!(
        "extension_dir = \"{}\"\n",
        extension_dir.display().to_string().replace('\\', "/")
    );
    for extension in EXTENSIONS {
        ini.push_str(&format!("extension={extension}\n"));
    }
    ini.push_str("memory_limit = 256M\n");
    ini.push_str("upload_max_filesize = 64M\n");
    ini.push_str("post_max_size = 64M\n");
    ini.push_str("max_execution_time = 300\n");

    let ini_path = dir.join("php.ini");
    fs::write(&ini_path, ini)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", ini_path.display())))?;

    Ok(ini_path)
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

        fs::remove_file(&ini_path).unwrap();
    }
}
