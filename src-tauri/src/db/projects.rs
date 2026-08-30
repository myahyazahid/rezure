//! Project record shape, shared between the live filesystem scan
//! (`services::projects`) and, once Phase 4 lands, a persisted SQLite copy.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
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
}
