//! Project record shape, shared between the live filesystem scan
//! (`services::projects`) and its persisted SQLite history.
//!
//! The filesystem stays authoritative for *which* projects exist and their
//! name/path/domain/stack; SQLite only adds what a rescan can't recover —
//! `last_opened_at` / `open_count`. `upsert_seen` and `record_opened` are
//! deliberately separate: a scan must never reset a project's history, and
//! opening a project must never touch its scanned fields.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::utils::error::AppError;

/// Where a project came from, which decides what can be done to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    /// Found by scanning `www_root()`. Managed by moving folders in and out.
    Scanned,
    /// A folder elsewhere the user pointed Rezure at. Unlinkable, and the
    /// only kind that can go missing while still being listed.
    Linked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub domain: String,
    /// Detected framework/stack (Laravel, Vue, WordPress, ...), shown as a
    /// badge in the UI.
    pub stack: String,
    /// Whether `domain` currently resolves to 127.0.0.1 via the OS hosts
    /// file — read-only, no admin rights needed to check.
    pub has_hosts_entry: bool,
    /// Unix seconds of the last time this project was opened (site, folder,
    /// or terminal) — `None` until it's been opened at least once.
    pub last_opened_at: Option<i64>,
    pub open_count: i64,
    pub kind: ProjectKind,
    /// True for a linked project whose folder is no longer there — moved,
    /// deleted, or on a drive that isn't plugged in. Listed anyway rather
    /// than quietly dropped, since the last case fixes itself.
    pub missing: bool,
    /// True when `domain` can't be written into the generated nginx config
    /// safely — see `services::projects::is_safe_domain`. Listed with an
    /// explanation rather than served, because serving it would mean
    /// emitting a config that stops every other site too.
    pub domain_invalid: bool,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

/// Records a project as currently present on disk, updating its scanned
/// fields. Leaves `first_seen_at`/`last_opened_at`/`open_count` alone if the
/// row already exists.
pub fn upsert_seen(conn: &Connection, project: &ProjectInfo) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO projects (id, name, path, domain, stack, first_seen_at, last_opened_at, open_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 0)
         ON CONFLICT(id) DO UPDATE SET
             name = excluded.name,
             path = excluded.path,
             domain = excluded.domain,
             stack = excluded.stack",
        (
            &project.id,
            &project.name,
            &project.path,
            &project.domain,
            &project.stack,
            now(),
        ),
    )
    .map_err(db_err)?;
    Ok(())
}

/// Bumps `open_count` and sets `last_opened_at` to now. Creates a row for
/// `id` if none exists yet (the following scan's `upsert_seen` fills in the
/// real name/path/domain/stack).
pub fn record_opened(conn: &Connection, id: &str) -> Result<(), AppError> {
    let ts = now();
    conn.execute(
        "INSERT INTO projects (id, name, path, domain, stack, first_seen_at, last_opened_at, open_count)
         VALUES (?1, '', '', '', '', ?2, ?2, 1)
         ON CONFLICT(id) DO UPDATE SET
             last_opened_at = excluded.last_opened_at,
             open_count = open_count + 1",
        (id, ts),
    )
    .map_err(db_err)?;
    Ok(())
}

/// Every project's history, keyed by id, for merging into a freshly scanned
/// list.
pub fn fetch_history(conn: &Connection) -> Result<HashMap<String, (Option<i64>, i64)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, last_opened_at, open_count FROM projects")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, Option<i64>>(1)?, row.get::<_, i64>(2)?),
            ))
        })
        .map_err(db_err)?;

    let mut history = HashMap::new();
    for row in rows {
        let (id, entry) = row.map_err(db_err)?;
        history.insert(id, entry);
    }
    Ok(history)
}

/// One project's history, for callers that only need a single row.
#[allow(dead_code)]
pub fn history_for(conn: &Connection, id: &str) -> Result<(Option<i64>, i64), AppError> {
    conn.query_row(
        "SELECT last_opened_at, open_count FROM projects WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(db_err)
    .map(|row| row.unwrap_or((None, 0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_migrations_for_test;

    fn sample(id: &str) -> ProjectInfo {
        ProjectInfo {
            id: id.to_string(),
            name: id.to_string(),
            path: format!("C:/rezure/www/{id}"),
            domain: format!("{id}.test"),
            stack: "PHP".to_string(),
            has_hosts_entry: false,
            last_opened_at: None,
            open_count: 0,
            kind: ProjectKind::Scanned,
            missing: false,
            domain_invalid: false,
        }
    }

    #[test]
    fn a_project_with_no_history_reports_none_and_zero() {
        let conn = init_migrations_for_test();
        upsert_seen(&conn, &sample("blog")).unwrap();

        let history = fetch_history(&conn).unwrap();
        assert_eq!(history.get("blog"), Some(&(None, 0)));
    }

    #[test]
    fn rescanning_never_resets_history() {
        let conn = init_migrations_for_test();
        upsert_seen(&conn, &sample("blog")).unwrap();
        record_opened(&conn, "blog").unwrap();
        record_opened(&conn, "blog").unwrap();

        // A rescan re-upserts the same project — history must survive it.
        upsert_seen(&conn, &sample("blog")).unwrap();

        let (last_opened, count) = history_for(&conn, "blog").unwrap();
        assert!(last_opened.is_some());
        assert_eq!(count, 2);
    }

    #[test]
    fn record_opened_creates_a_row_if_the_project_was_never_scanned() {
        let conn = init_migrations_for_test();
        record_opened(&conn, "not-yet-scanned").unwrap();

        let (last_opened, count) = history_for(&conn, "not-yet-scanned").unwrap();
        assert!(last_opened.is_some());
        assert_eq!(count, 1);
    }

    #[test]
    fn a_project_never_seen_has_no_history_row() {
        let conn = init_migrations_for_test();
        let (last_opened, count) = history_for(&conn, "ghost").unwrap();
        assert_eq!(last_opened, None);
        assert_eq!(count, 0);
    }
}
