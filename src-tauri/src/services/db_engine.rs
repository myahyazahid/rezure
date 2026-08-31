//! Which database engine a datadir belongs to, and the few places the two
//! Rezure supports actually differ.
//!
//! # Why two engines
//!
//! The tools Rezure sits next to disagree: Laragon ships Oracle MySQL,
//! XAMPP ships MariaDB. A datadir made by one **cannot** be opened by the
//! other — the projects forked at MySQL 5.5, and MySQL 8.0+ keeps a data
//! dictionary (`mysql.ibd`) MariaDB has no reader for. Since the point of
//! the profile switcher is to open datadirs Rezure didn't create, refusing
//! one of the two engines would mean refusing half the datadirs on a
//! typical machine.
//!
//! # Why the abstraction is this thin
//!
//! MariaDB's Windows build ships `mysqld.exe`, `mysql.exe`, `mysqladmin.exe`
//! and `mysqldump.exe` as compatibility aliases beside its own `mariadb*`
//! names, so every binary Rezure invokes is spelled identically on both.
//! The only genuine difference is how an *empty* datadir gets bootstrapped —
//! and that never runs for an adopted profile, only for one Rezure creates
//! itself.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

/// The server binary. Same name on both engines — see the module docs.
pub const SERVER_EXE: &str = "mysqld.exe";
/// The interactive console client.
pub const CLIENT_EXE: &str = "mysql.exe";
/// Used for the graceful `shutdown` that a large InnoDB datadir depends on.
pub const ADMIN_EXE: &str = "mysqladmin.exe";
pub const DUMP_EXE: &str = "mysqldump.exe";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    MySql,
    MariaDb,
}

impl Engine {
    /// The `binaries::discover` family, and the folder installs live under
    /// (`bin/<family>/<version>/`). Keeping the two engines in separate
    /// families is what lets a machine hold MySQL 8.4 and MariaDB 11.2 at
    /// once without either shadowing the other.
    pub fn family(&self) -> &'static str {
        match self {
            Engine::MySql => "mysql",
            Engine::MariaDb => "mariadb",
        }
    }

    /// Display name, for UI labels and error messages.
    pub fn label(&self) -> &'static str {
        match self {
            Engine::MySql => "MySQL",
            Engine::MariaDb => "MariaDB",
        }
    }

    /// Reads the engine back off a datadir, so adopting an existing folder
    /// doesn't rely on the user knowing (or correctly guessing) which one
    /// wrote it — picking wrong is exactly the mistake that corrupts data.
    ///
    /// The markers are structural, not cosmetic: `mysql.ibd` is MySQL 8.0's
    /// data-dictionary tablespace, which MariaDB never writes, and
    /// `aria_log_control` belongs to Aria, a storage engine only MariaDB
    /// has. Both engines also leave an `ibdata1`, which is why that shared
    /// file is no help and isn't consulted.
    ///
    /// `None` means neither marker was found — an empty folder, a path
    /// that isn't a datadir at all, or a layout too old to recognize. The
    /// caller asks the user rather than guessing.
    pub fn detect_from_datadir(data_dir: &Path) -> Option<Engine> {
        if data_dir.join("mysql.ibd").is_file() {
            return Some(Engine::MySql);
        }
        if data_dir.join("aria_log_control").is_file() {
            return Some(Engine::MariaDb);
        }
        None
    }

    /// Prepares an empty datadir so the server will start against it.
    ///
    /// The one place the engines genuinely diverge. MariaDB keeps a separate
    /// `mariadb-install-db.exe`; MySQL removed its equivalent in 8.0 and
    /// folded the job into the server itself behind `--initialize-insecure`.
    /// "Insecure" here means *no root password*, which matches how Rezure's
    /// own MariaDB is already bootstrapped — see `services::database`'s
    /// module docs for why a throwaway local server doesn't get one.
    ///
    /// Never called for an adopted profile: [`needs_bootstrap`] is false for
    /// any datadir that already has contents, and Rezure must not write into
    /// someone else's data.
    pub fn bootstrap(&self, server_exe: &Path, data_dir: &Path) -> Result<(), AppError> {
        let bin_dir = server_exe
            .parent()
            .ok_or_else(|| AppError::ProcessBootstrapFailed {
                name: self.label().to_string(),
                reason: "could not locate the server's bin directory".to_string(),
            })?;

        std::fs::create_dir_all(data_dir).map_err(|e| AppError::ProcessBootstrapFailed {
            name: self.label().to_string(),
            reason: format!("could not create {}: {e}", data_dir.display()),
        })?;

        let output = match self {
            // Older Windows builds of `mariadb-install-db` (unlike the Linux
            // installer) don't support `--auth-root-authentication-method` —
            // root is created passwordless and localhost-only by default.
            Engine::MariaDb => Command::new(bin_dir.join("mariadb-install-db.exe"))
                .current_dir(bin_dir)
                .arg(format!("--datadir={}", data_dir.display()))
                .hidden()
                .output(),
            Engine::MySql => Command::new(server_exe)
                .current_dir(bin_dir)
                .arg("--initialize-insecure")
                .arg(format!("--datadir={}", data_dir.display()))
                .hidden()
                .output(),
        }
        .map_err(|e| AppError::ProcessBootstrapFailed {
            name: self.label().to_string(),
            reason: e.to_string(),
        })?;

        if !output.status.success() {
            return Err(AppError::ProcessBootstrapFailed {
                name: self.label().to_string(),
                reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(())
    }
}

/// Whether `data_dir` still needs [`Engine::bootstrap`] — i.e. it's absent
/// or empty. A folder with anything in it is treated as somebody's data and
/// left strictly alone.
pub fn needs_bootstrap(data_dir: &Path) -> bool {
    data_dir
        .read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_datadir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rezure-test-datadir-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The marker each engine leaves. Verified against three real datadirs:
    /// Laragon's MySQL 8.4, XAMPP's MariaDB 10.4, and Rezure's own MariaDB
    /// 11.2 — the first had `mysql.ibd`, the other two `aria_log_control`.
    #[test]
    fn a_mysql_datadir_is_recognized_by_its_data_dictionary() {
        let dir = temp_datadir("mysql");
        fs::write(dir.join("mysql.ibd"), b"").unwrap();
        // Both engines write this one, so its presence must not sway the call.
        fs::write(dir.join("ibdata1"), b"").unwrap();

        assert_eq!(Engine::detect_from_datadir(&dir), Some(Engine::MySql));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_mariadb_datadir_is_recognized_by_its_aria_log() {
        let dir = temp_datadir("mariadb");
        fs::write(dir.join("aria_log_control"), b"").unwrap();
        fs::write(dir.join("ibdata1"), b"").unwrap();

        assert_eq!(Engine::detect_from_datadir(&dir), Some(Engine::MariaDb));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// `ibdata1` alone is not evidence either way — guessing from it would
    /// pick an engine for a datadir the other one wrote.
    #[test]
    fn a_shared_innodb_file_alone_identifies_nothing() {
        let dir = temp_datadir("ambiguous");
        fs::write(dir.join("ibdata1"), b"").unwrap();

        assert_eq!(Engine::detect_from_datadir(&dir), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_empty_or_missing_folder_identifies_nothing() {
        let dir = temp_datadir("empty");
        assert_eq!(Engine::detect_from_datadir(&dir), None);
        assert_eq!(
            Engine::detect_from_datadir(&dir.join("does-not-exist")),
            None
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_an_empty_datadir_is_bootstrapped() {
        let dir = temp_datadir("bootstrap-check");
        assert!(needs_bootstrap(&dir), "an empty folder needs bootstrapping");
        assert!(
            needs_bootstrap(&dir.join("not-created-yet")),
            "a missing folder needs bootstrapping"
        );

        fs::write(dir.join("ibdata1"), b"").unwrap();
        assert!(
            !needs_bootstrap(&dir),
            "a folder with contents is somebody's data — never touch it"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_two_engines_never_share_an_install_family() {
        assert_ne!(Engine::MySql.family(), Engine::MariaDb.family());
    }

    /// Reads the engine off the datadirs actually on this machine. Run with:
    /// `cargo test --lib services::db_engine::tests::print_real_datadirs -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_real_datadirs() {
        for candidate in [
            r"C:\laragon\data\mysql-8.4",
            r"C:\xampp\mysql\data",
            r"C:\Users\Public\does-not-exist",
        ] {
            println!(
                "{candidate} -> {:?}",
                Engine::detect_from_datadir(Path::new(candidate))
            );
        }
    }
}
