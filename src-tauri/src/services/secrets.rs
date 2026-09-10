//! Where a remote connection's password lives.
//!
//! Two places, and never a third:
//!
//! * **Windows Credential Manager**, via `keyring`, for a connection the
//!   user asked to save. The OS owns the encryption and the per-user
//!   isolation, which is exactly the part Rezure should not be inventing.
//! * **Process memory**, for a connection saved without its password. It
//!   is asked for once per session and forgotten when Rezure exits.
//!
//! What it is deliberately *not*: a field in `connections.json`. That file
//! is plain text in a folder users are actively encouraged to open — see
//! `utils::paths` — and a password sitting in it would be readable by
//! anything running as the user, backed up by any sync client pointed at
//! `C:\rezure`, and pasted into every bug report that includes a config
//! dump.
//!
//! Credential Manager can fail (group policy, a locked-down machine). That
//! is reported to the caller rather than silently falling back to writing
//! the password somewhere less protected.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::utils::error::AppError;

/// The Credential Manager "service" every Rezure connection is filed under.
/// The connection id is the account, so entries stay one-to-one with
/// connections and survive a rename.
const SERVICE: &str = "Rezure Database Connections";

/// Passwords for connections the user chose not to save — session only.
fn session_cell() -> &'static Mutex<HashMap<String, String>> {
    static SESSION: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The Credential Manager key for a connection's *SSH* password, as
/// opposed to its database one.
///
/// A connection can need both, and they are rarely the same — so they get
/// two entries rather than one that has to mean different things depending
/// on which code path asks.
pub fn ssh_id(connection_id: &str) -> String {
    format!("{connection_id}:ssh")
}

fn entry(id: &str) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE, id).map_err(|e| AppError::CredentialStore(e.to_string()))
}

/// Files a password under `id` in Credential Manager, replacing whatever
/// was there.
pub fn save(id: &str, password: &str) -> Result<(), AppError> {
    entry(id)?
        .set_password(password)
        .map_err(|e| AppError::CredentialStore(e.to_string()))
}

/// The saved password, or `None` when there is no entry.
///
/// A missing entry is not an error — a connection may legitimately have no
/// password saved, and the caller falls back to the session cache.
pub fn load(id: &str) -> Result<Option<String>, AppError> {
    match entry(id)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::CredentialStore(e.to_string())),
    }
}

/// Holds a password for this run only, for a connection saved without one.
pub fn remember_for_session(id: &str, password: &str) {
    if let Ok(mut session) = session_cell().lock() {
        session.insert(id.to_string(), password.to_string());
    }
}

/// The password to connect with, wherever it is: Credential Manager first,
/// then this session's memory.
///
/// `None` means "no password known", which is a legitimate answer — a
/// passwordless server, or a connection whose password hasn't been entered
/// yet this session. The client will say which if it turns out to matter.
pub fn resolve(id: &str) -> Option<String> {
    if let Ok(Some(saved)) = load(id) {
        return Some(saved);
    }
    session_cell()
        .lock()
        .ok()
        .and_then(|session| session.get(id).cloned())
}

/// Removes every trace of `id`'s password. Best-effort by design: this runs
/// when a connection is deleted, and a Credential Manager that refuses must
/// not block the deletion the user asked for — it would leave a connection
/// on screen that they can no longer get rid of.
pub fn forget(id: &str) {
    for key in [id.to_string(), ssh_id(id)] {
        if let Ok(entry) = entry(&key) {
            let _ = entry.delete_credential();
        }
        if let Ok(mut session) = session_cell().lock() {
            session.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_password_is_returned_when_nothing_is_saved() {
        // Ids are namespaced per test so a real Credential Manager entry
        // from another run can't influence the result.
        let id = "rezure-test-session-only";
        remember_for_session(id, "hunter2");
        assert_eq!(resolve(id).as_deref(), Some("hunter2"));
        forget(id);
    }

    #[test]
    fn forgetting_clears_the_session_copy() {
        let id = "rezure-test-forget";
        remember_for_session(id, "hunter2");
        forget(id);
        assert_eq!(resolve(id), None);
    }

    #[test]
    fn a_connections_ssh_password_is_filed_separately_from_its_database_one() {
        let id = "rezure-test-two-secrets";
        remember_for_session(id, "db-password");
        remember_for_session(&ssh_id(id), "ssh-password");
        assert_eq!(resolve(id).as_deref(), Some("db-password"));
        assert_eq!(resolve(&ssh_id(id)).as_deref(), Some("ssh-password"));

        // Deleting a connection has to take both with it.
        forget(id);
        assert_eq!(resolve(id), None);
        assert_eq!(resolve(&ssh_id(id)), None);
    }

    #[test]
    fn an_unknown_id_has_no_password() {
        assert_eq!(resolve("rezure-test-never-set"), None);
    }
}
