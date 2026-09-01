//! Local queue for telemetry the app has recorded but not yet sent — a
//! companion to `services::telemetry::TelemetryClient`, which is the only
//! writer, and `services::telemetry::send_pending`, which is the only reader.

use rusqlite::Connection;

use crate::utils::error::AppError;

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

/// One queued row, ready to be sent.
pub struct PendingEvent {
    pub id: String,
    pub payload: String,
    pub kind: String,
}

/// Queues one event or heartbeat payload for later sending.
pub fn insert_pending(
    conn: &Connection,
    id: &str,
    payload_json: &str,
    kind: &str,
    created_at: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO pending_events (id, payload, type, created_at, sent_at)
         VALUES (?1, ?2, ?3, ?4, NULL)",
        (id, payload_json, kind, created_at),
    )
    .map_err(db_err)?;
    Ok(())
}

/// The oldest `limit` rows that haven't been sent yet — oldest first, so a
/// long-unreachable queue drains in the order it was recorded.
pub fn fetch_unsent(conn: &Connection, limit: i64) -> Result<Vec<PendingEvent>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, payload, type FROM pending_events
             WHERE sent_at IS NULL ORDER BY created_at ASC LIMIT ?1",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([limit], |row| {
            Ok(PendingEvent {
                id: row.get(0)?,
                payload: row.get(1)?,
                kind: row.get(2)?,
            })
        })
        .map_err(db_err)?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row.map_err(db_err)?);
    }
    Ok(events)
}

/// Marks a row as successfully sent.
pub fn mark_sent(conn: &Connection, id: &str, sent_at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE pending_events SET sent_at = ?1 WHERE id = ?2",
        (sent_at, id),
    )
    .map_err(db_err)?;
    Ok(())
}

/// Drops sent rows older than `cutoff` — retention, not correctness: a row
/// still `NULL` (unsent) is never touched here regardless of age.
pub fn delete_sent_before(conn: &Connection, cutoff: i64) -> Result<usize, AppError> {
    conn.execute(
        "DELETE FROM pending_events WHERE sent_at IS NOT NULL AND sent_at < ?1",
        [cutoff],
    )
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_migrations_for_test;

    #[test]
    fn inserts_a_row_with_no_sent_at() {
        let conn = init_migrations_for_test();
        insert_pending(&conn, "evt-1", "{}", "event", 1_700_000_000).unwrap();

        let (id, kind, sent_at): (String, String, Option<i64>) = conn
            .query_row(
                "SELECT id, type, sent_at FROM pending_events WHERE id = ?1",
                ["evt-1"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(id, "evt-1");
        assert_eq!(kind, "event");
        assert_eq!(sent_at, None);
    }

    #[test]
    fn fetch_unsent_only_returns_rows_with_no_sent_at() {
        let conn = init_migrations_for_test();
        insert_pending(&conn, "evt-1", "{}", "event", 100).unwrap();
        insert_pending(&conn, "evt-2", "{}", "event", 200).unwrap();
        mark_sent(&conn, "evt-1", 150).unwrap();

        let unsent = fetch_unsent(&conn, 10).unwrap();
        assert_eq!(unsent.len(), 1);
        assert_eq!(unsent[0].id, "evt-2");
    }

    #[test]
    fn fetch_unsent_respects_the_limit_oldest_first() {
        let conn = init_migrations_for_test();
        insert_pending(&conn, "evt-1", "{}", "event", 100).unwrap();
        insert_pending(&conn, "evt-2", "{}", "event", 200).unwrap();

        let unsent = fetch_unsent(&conn, 1).unwrap();
        assert_eq!(unsent.len(), 1);
        assert_eq!(unsent[0].id, "evt-1");
    }

    #[test]
    fn delete_sent_before_leaves_unsent_rows_alone() {
        let conn = init_migrations_for_test();
        insert_pending(&conn, "evt-1", "{}", "event", 100).unwrap();
        insert_pending(&conn, "evt-2", "{}", "event", 200).unwrap();
        mark_sent(&conn, "evt-1", 150).unwrap();

        let deleted = delete_sent_before(&conn, 1_000).unwrap();
        assert_eq!(deleted, 1);

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
        let remaining_id: String = conn
            .query_row("SELECT id FROM pending_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining_id, "evt-2");
    }
}
