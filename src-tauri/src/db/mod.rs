//! SQLite models & queries — a companion store to the filesystem, not a
//! replacement for it. `services::projects::scan_projects` stays the source
//! of truth for *which* projects exist; this holds what the filesystem
//! can't: history (`last_opened_at`, `open_count`) across restarts.
//!
//! Schema changes are migrations appended to [`migrations`], never a
//! hand-edit of an existing one, per `CLAUDE.md`.

pub mod projects;
pub mod telemetry;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use crate::utils::error::AppError;
use crate::utils::paths;

fn migrations() -> &'static Migrations<'static> {
    static MIGRATIONS: OnceLock<Migrations> = OnceLock::new();
    MIGRATIONS.get_or_init(|| {
        Migrations::new(vec![
            M::up(
                "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                domain TEXT NOT NULL,
                stack TEXT NOT NULL,
                first_seen_at INTEGER NOT NULL,
                last_opened_at INTEGER,
                open_count INTEGER NOT NULL DEFAULT 0
            )",
            ),
            M::up(
                "CREATE TABLE pending_events (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                type TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                sent_at INTEGER
            )",
            ),
        ])
    })
}

/// `%LOCALAPPDATA%\Rezure\rezure.db`.
pub fn db_path() -> Result<PathBuf, AppError> {
    paths::db_file()
}

/// Opens the database, creating and migrating it as needed.
pub fn init() -> Result<Connection, AppError> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Database(format!("could not create {}: {e}", parent.display()))
        })?;
    }

    let mut conn = Connection::open(&path)
        .map_err(|e| AppError::Database(format!("could not open {}: {e}", path.display())))?;
    migrations()
        .to_latest(&mut conn)
        .map_err(|e| AppError::Database(format!("migration failed: {e}")))?;

    Ok(conn)
}

/// Tauri-managed state holding the single SQLite connection.
pub struct DbState(pub Mutex<Connection>);

impl DbState {
    pub fn new(conn: Connection) -> Self {
        Self(Mutex::new(conn))
    }
}

/// An in-memory, fully-migrated connection for `db::projects`' tests — same
/// schema as the real database, isolated per test and never touching disk.
#[cfg(test)]
pub fn init_migrations_for_test() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    migrations().to_latest(&mut conn).unwrap();
    conn
}
