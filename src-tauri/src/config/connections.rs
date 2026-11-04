//! Remote database connections — servers Rezure talks to but does not run.
//!
//! # Why this isn't a `Profile`
//!
//! A [`Profile`](crate::config::profiles::Profile) describes a *datadir*:
//! which folder, which engine wrote it, which `my.ini` it depends on, which
//! binary can open it, and which port the `mysqld` Rezure spawns will listen
//! on. Every one of those exists because Rezure owns that process.
//!
//! A connection owns no process. It is a host, a port, a user and a
//! credential — nothing to start, nothing to stop, nothing to corrupt by
//! opening it twice. Folding it into `Profile` would mean making `datadir`,
//! `binary_dir` and `defaults_file` optional and teaching every reader of
//! them that they might be meaningless, which is how "the profile's datadir"
//! stops being a fact anyone can rely on.
//!
//! So the two are stored separately, and which one is being *read* is a
//! third thing: [`ConnectionStore::active_id`]. `None` means the local
//! profile is what the Databases page is looking at.
//!
//! # What is not stored here
//!
//! Passwords. They go to Windows Credential Manager via
//! `services::secrets`, keyed by connection id, and never touch this file.
//!
//! Stored beside `settings.json` and `profiles.json` in `etc/connections.json`.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::services::db_engine::Engine;
use crate::utils::error::AppError;
use crate::utils::paths;

/// How hard to insist on TLS.
///
/// Deliberately three states rather than a bool: "preferred" is what both
/// clients already do on their own, and spelling it out keeps the stored
/// value honest for a connection the user never thought about, instead of
/// recording a `false` that looks like a decision to disable TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    /// Never negotiate TLS. For a server on a private network or reached
    /// through a tunnel, where the encryption is somebody else's job.
    Disabled,
    /// The client's own default — use TLS when the server offers it.
    #[default]
    Preferred,
    /// Refuse to connect without TLS. What a managed provider needs.
    Required,
}

/// Which kind of server a connection points at — and so which client
/// `services::database` talks to it with.
///
/// Older files have no such field and were all MySQL-family, which is what
/// the default keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    /// MySQL or MariaDB, reached with the client binaries that ship beside
    /// a local server build — see `engine`.
    #[default]
    Mysql,
    /// Microsoft SQL Server, reached through the ODBC driver
    /// (`services::mssql`).
    Sqlserver,
    /// PostgreSQL, reached with the `psql`/`pg_dump` of an installed
    /// PostgreSQL build (`services::postgres_client`).
    Postgres,
}

/// How the SSH session authenticates.
///
/// A key is the better answer and stays the default the UI offers, but a
/// great many small VPSes are set up with password login and nothing else,
/// and telling their owner to go configure key auth first is telling them
/// the feature doesn't work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SshAuth {
    Key {
        /// Absolute path to a private key file (`.pem`, `id_ed25519`, …).
        path: String,
    },
    /// The password is never stored here — it goes to Credential Manager
    /// alongside the database one, under its own key.
    Password,
}

/// How to reach a database that only listens on its own machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnel {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    /// Stable across renames and host edits, and the key the password is
    /// filed under in Credential Manager — so editing a connection never
    /// orphans its credential.
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub kind: ServerKind,
    /// Which engine the *server* runs, for a [`ServerKind::Mysql`]
    /// connection. Ignored for every other kind.
    ///
    /// Not cosmetic: the client binary is picked from this, and the two
    /// engines' clients disagree on options (TLS flags, MySQL 8's
    /// `mysqldump` extras), so guessing here produces failures with no
    /// obvious cause.
    pub engine: Engine,
    /// SQL Server only: sign in as the Windows user Rezure runs as, with no
    /// password — how LocalDB and most office domain servers are reached.
    /// `user` is then empty and nothing is filed in Credential Manager.
    #[serde(default)]
    pub windows_auth: bool,
    /// SQL Server only: accept the server's certificate without checking who
    /// issued it.
    ///
    /// ODBC Driver 18 encrypts by default and refuses a self-signed
    /// certificate, which is what nearly every development and in-house
    /// server has. Off unless the user turns it on: it trades away the check
    /// that the server is the one they meant to reach.
    #[serde(default)]
    pub trust_server_certificate: bool,
    /// True only for the connections Rezure creates for servers it runs
    /// itself — its LocalDB instance (`services::mssql_localdb`) and its
    /// PostgreSQL service (`services::postgres`): reached on this machine,
    /// owned by Rezure, so they're treated like the local server — writable,
    /// "used by" shown, and not removable by hand.
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub tls_mode: TlsMode,
    /// Refuses every write Rezure can issue — create, drop and import.
    ///
    /// Defaults to true for a reason: the whole point of this feature is
    /// reaching servers that aren't throwaway local ones, and the cost of a
    /// wrong write there is unbounded. Turning it off is a deliberate act.
    #[serde(default = "default_true")]
    pub read_only: bool,
    /// Whether the password was saved to Credential Manager. When false the
    /// user is asked for it each session and it lives only in memory.
    #[serde(default)]
    pub save_password: bool,
    /// When set, the database is reached through an SSH forward instead of
    /// directly.
    ///
    /// This changes what `host` and `port` above *mean*: they are resolved
    /// on the SSH server, not on this machine — so they are usually
    /// `127.0.0.1` and the database's real port, which is precisely the
    /// case a tunnel exists for.
    #[serde(default)]
    pub ssh: Option<SshTunnel>,
    /// Unix seconds, for ordering the switcher by recency.
    pub last_used_at: Option<i64>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStore {
    #[serde(default)]
    pub connections: Vec<Connection>,
    /// The connection the Databases page is currently reading, or `None`
    /// for "the active local profile".
    ///
    /// Kept here rather than in `profiles.json` because selecting a remote
    /// connection does *not* touch the local server: it keeps running,
    /// serving whichever profile it was already on. Switching back is then
    /// free, and closing Rezure never leaves a datadir half-served.
    #[serde(default)]
    pub active_id: Option<String>,
}

impl ConnectionStore {
    pub fn active(&self) -> Option<&Connection> {
        let id = self.active_id.as_deref()?;
        self.connections.iter().find(|c| c.id == id)
    }

    /// Rejects a second connection to the same host/port/user. Two entries
    /// for one account are indistinguishable in the switcher and their
    /// stored passwords drift apart the moment one of them is updated.
    ///
    /// The SSH host is part of the identity, not decoration: every tunnelled
    /// connection has a database host of `127.0.0.1`, so staging and
    /// production would look like the same endpoint without it.
    pub fn endpoint_taken_by(
        &self,
        host: &str,
        port: u16,
        user: &str,
        ssh_host: Option<&str>,
        excluding_id: Option<&str>,
    ) -> Option<&Connection> {
        self.connections.iter().find(|c| {
            let same_gateway = match (c.ssh.as_ref().map(|s| s.host.as_str()), ssh_host) {
                (Some(existing), Some(candidate)) => existing.eq_ignore_ascii_case(candidate),
                (None, None) => true,
                _ => false,
            };
            c.port == port
                && c.host.eq_ignore_ascii_case(host)
                && c.user == user
                && same_gateway
                && Some(c.id.as_str()) != excluding_id
        })
    }

    /// Makes sure the store holds `seed` — the connection to a server Rezure
    /// runs itself — exactly once.
    ///
    /// Not a plain "add if missing", because the store can hold a copy that
    /// no longer reads as Rezure's: `kind` and `managed` are `serde(default)`,
    /// so a build from before they existed rewrites this file without them
    /// and the copy comes back as an unmanaged MySQL connection. It can't
    /// connect (wrong client) and, left alone, sits in the switcher beside a
    /// freshly seeded one. So:
    ///
    /// * the managed entry of `seed.kind` is kept if there is one;
    /// * otherwise an entry already at `seed`'s endpoint is turned back into
    ///   Rezure's, keeping its id (an active selection still points at it)
    ///   and when it was last used;
    /// * otherwise `seed` is added;
    ///
    /// and every other entry at that endpoint — a duplicate by the rule
    /// [`endpoint_taken_by`](Self::endpoint_taken_by) enforces on add — is
    /// dropped, with the selection moved to the kept one if it was on it.
    pub fn settle_managed(&mut self, seed: Connection) -> SettledManaged {
        let (host, port, user) = (seed.host.clone(), seed.port, seed.user.clone());
        let mut settled = SettledManaged::default();

        let existing = self
            .connections
            .iter()
            .find(|c| c.managed && c.kind == seed.kind)
            .map(|c| c.id.clone());
        let stale = self
            .endpoint_taken_by(&host, port, &user, None, None)
            .map(|c| c.id.clone());
        let kept_id = match (existing, stale) {
            (Some(id), _) => id,
            (None, Some(id)) => {
                if let Some(entry) = self.connections.iter_mut().find(|c| c.id == id) {
                    *entry = Connection {
                        id: id.clone(),
                        last_used_at: entry.last_used_at,
                        ..seed
                    };
                }
                settled.repaired = Some(id.clone());
                id
            }
            (None, None) => {
                let id = seed.id.clone();
                self.connections.push(seed);
                settled.added = true;
                id
            }
        };

        while let Some(id) = self
            .endpoint_taken_by(&host, port, &user, None, Some(&kept_id))
            .map(|c| c.id.clone())
        {
            self.connections.retain(|c| c.id != id);
            if self.active_id.as_deref() == Some(id.as_str()) {
                self.active_id = Some(kept_id.clone());
            }
            settled.dropped.push(id);
        }
        settled
    }
}

/// What [`ConnectionStore::settle_managed`] changed — the ids it touched, so
/// the caller can clear what's keyed by them outside this file (passwords in
/// Credential Manager, SSH tunnels).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SettledManaged {
    /// The seed was added as a new entry.
    pub added: bool,
    /// An entry at the seed's endpoint that was turned back into Rezure's.
    pub repaired: Option<String>,
    /// Duplicates at that endpoint that were removed.
    pub dropped: Vec<String>,
}

impl SettledManaged {
    /// Whether the store changed and needs saving.
    pub fn changed(&self) -> bool {
        self.added || self.repaired.is_some() || !self.dropped.is_empty()
    }
}

fn store_path() -> Result<std::path::PathBuf, AppError> {
    Ok(paths::etc()?.join("connections.json"))
}

/// Reads the store, or an empty one.
///
/// A malformed file is reported rather than silently replaced: the passwords
/// in Credential Manager are keyed by ids that live only in this file, so
/// quietly starting from empty would strand every one of them.
pub fn load() -> Result<ConnectionStore, AppError> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(ConnectionStore::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", path.display())))?;
    serde_json::from_str(&raw).map_err(|e| {
        AppError::Settings(format!(
            "{} is not valid JSON ({e}) - fix or delete it to continue",
            path.display()
        ))
    })
}

pub fn save(store: &ConnectionStore) -> Result<(), AppError> {
    let path = store_path()?;
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| AppError::Settings(format!("could not serialise connections: {e}")))?;
    std::fs::write(&path, json)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))
}

/// The fields a caller supplies when registering a connection — everything
/// except the id and the bookkeeping this module fills in itself.
#[derive(Debug, Clone)]
pub struct NewConnection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub kind: ServerKind,
    pub engine: Engine,
    pub windows_auth: bool,
    pub trust_server_certificate: bool,
    pub tls_mode: TlsMode,
    pub read_only: bool,
    pub save_password: bool,
    pub ssh: Option<SshTunnel>,
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(id: &str, host: &str, port: u16, user: &str) -> Connection {
        Connection {
            id: id.to_string(),
            name: id.to_string(),
            host: host.to_string(),
            port,
            user: user.to_string(),
            kind: ServerKind::Mysql,
            engine: Engine::MySql,
            windows_auth: false,
            trust_server_certificate: false,
            managed: false,
            tls_mode: TlsMode::default(),
            read_only: true,
            save_password: false,
            ssh: None,
            last_used_at: None,
        }
    }

    #[test]
    fn an_unset_active_id_means_the_local_profile() {
        let store = ConnectionStore {
            connections: vec![connection("a", "db.example.com", 3306, "root")],
            active_id: None,
        };
        assert!(store.active().is_none());
    }

    #[test]
    fn an_active_id_pointing_at_a_deleted_connection_reads_as_local() {
        // Removing the active connection must not leave the page pointed at
        // something that no longer exists.
        let store = ConnectionStore {
            connections: vec![],
            active_id: Some("gone".to_string()),
        };
        assert!(store.active().is_none());
    }

    #[test]
    fn the_same_endpoint_is_refused_regardless_of_host_casing() {
        let store = ConnectionStore {
            connections: vec![connection("a", "DB.example.com", 3306, "root")],
            active_id: None,
        };
        assert!(store
            .endpoint_taken_by("db.example.com", 3306, "root", None, None)
            .is_some());
    }

    #[test]
    fn the_same_host_with_a_different_user_is_a_different_connection() {
        let store = ConnectionStore {
            connections: vec![connection("a", "db.example.com", 3306, "root")],
            active_id: None,
        };
        assert!(store
            .endpoint_taken_by("db.example.com", 3306, "app", None, None)
            .is_none());
    }

    #[test]
    fn editing_a_connection_does_not_collide_with_itself() {
        let store = ConnectionStore {
            connections: vec![connection("a", "db.example.com", 3306, "root")],
            active_id: None,
        };
        assert!(store
            .endpoint_taken_by("db.example.com", 3306, "root", None, Some("a"))
            .is_none());
    }

    #[test]
    fn two_tunnels_to_the_same_local_port_are_different_connections() {
        // Both are 127.0.0.1:3306 as far as the database is concerned; only
        // the SSH host tells staging and production apart.
        let mut staging = connection("a", "127.0.0.1", 3306, "root");
        staging.ssh = Some(SshTunnel {
            host: "staging.example.com".to_string(),
            port: 22,
            user: "deploy".to_string(),
            auth: SshAuth::Password,
        });
        let store = ConnectionStore {
            connections: vec![staging],
            active_id: None,
        };
        assert!(store
            .endpoint_taken_by("127.0.0.1", 3306, "root", Some("prod.example.com"), None)
            .is_none());
        assert!(store
            .endpoint_taken_by("127.0.0.1", 3306, "root", Some("staging.example.com"), None)
            .is_some());
        // A direct connection to the same address is not the tunnelled one.
        assert!(store
            .endpoint_taken_by("127.0.0.1", 3306, "root", None, None)
            .is_none());
    }

    #[test]
    fn a_tunnelled_connection_round_trips_through_json() {
        let mut connection = connection("a", "127.0.0.1", 6066, "root");
        connection.ssh = Some(SshTunnel {
            host: "34.124.178.228".to_string(),
            port: 22,
            user: "deploy".to_string(),
            auth: SshAuth::Key {
                path: r"C:\keys\id_rsa.pem".to_string(),
            },
        });
        let json = serde_json::to_string(&connection).unwrap();
        let parsed: Connection = serde_json::from_str(&json).unwrap();
        let ssh = parsed.ssh.expect("the tunnel must survive a round trip");
        assert_eq!(ssh.host, "34.124.178.228");
        assert_eq!(ssh.user, "deploy");
        assert!(matches!(ssh.auth, SshAuth::Key { .. }));
        // The database host stays as the SSH server sees it.
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 6066);
    }

    #[test]
    fn a_connection_defaults_to_read_only_when_the_field_is_missing() {
        // Files written before `readOnly` existed must not silently grant
        // write access to a remote server.
        let parsed: Connection = serde_json::from_str(
            r#"{"id":"a","name":"a","host":"h","port":3306,"user":"root",
                 "engine":"mysql","lastUsedAt":null}"#,
        )
        .expect("the older shape must still parse");
        assert!(parsed.read_only);
        assert_eq!(parsed.tls_mode, TlsMode::Preferred);
        // Every connection saved before SQL Server existed was MySQL-family.
        assert_eq!(parsed.kind, ServerKind::Mysql);
        assert!(!parsed.windows_auth && !parsed.managed);
    }

    #[test]
    fn a_sql_server_connection_round_trips_through_json() {
        let mut connection = connection("a", r"(localdb)\Rezure", 0, "");
        connection.kind = ServerKind::Sqlserver;
        connection.windows_auth = true;
        connection.managed = true;
        let json = serde_json::to_string(&connection).unwrap();
        assert!(json.contains(r#""kind":"sqlserver""#), "{json}");
        let parsed: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kind, ServerKind::Sqlserver);
        assert!(parsed.windows_auth && parsed.managed);
        assert_eq!(parsed.host, r"(localdb)\Rezure");
    }

    /// What `ensure_postgres` seeds: Rezure's own PostgreSQL.
    fn postgres_seed(id: &str) -> Connection {
        let mut seed = connection(id, "127.0.0.1", 5432, "postgres");
        seed.kind = ServerKind::Postgres;
        seed.managed = true;
        seed.read_only = false;
        seed
    }

    /// The seed after a build that predates `kind`/`managed` rewrote it.
    fn stripped(mut connection: Connection) -> Connection {
        connection.kind = ServerKind::Mysql;
        connection.managed = false;
        connection
    }

    #[test]
    fn settling_into_an_empty_store_adds_the_seed() {
        let mut store = ConnectionStore::default();
        let settled = store.settle_managed(postgres_seed("new"));
        assert!(settled.added && settled.repaired.is_none() && settled.dropped.is_empty());
        assert_eq!(store.connections.len(), 1);
        assert!(store.connections[0].managed);
    }

    #[test]
    fn settling_again_changes_nothing() {
        let mut store = ConnectionStore {
            connections: vec![postgres_seed("kept")],
            active_id: None,
        };
        let settled = store.settle_managed(postgres_seed("new"));
        assert!(!settled.changed());
        assert_eq!(store.connections.len(), 1);
        assert_eq!(store.connections[0].id, "kept");
    }

    #[test]
    fn a_stripped_copy_is_repaired_in_place_instead_of_seeded_beside() {
        let mut old = stripped(postgres_seed("old"));
        old.last_used_at = Some(42);
        let mut store = ConnectionStore {
            connections: vec![old],
            active_id: Some("old".to_string()),
        };
        let settled = store.settle_managed(postgres_seed("new"));
        assert_eq!(settled.repaired.as_deref(), Some("old"));
        assert!(!settled.added);
        assert_eq!(store.connections.len(), 1);
        let repaired = &store.connections[0];
        assert_eq!(repaired.id, "old", "the id is kept so the selection holds");
        assert_eq!(repaired.kind, ServerKind::Postgres);
        assert!(repaired.managed);
        assert_eq!(repaired.last_used_at, Some(42));
        assert_eq!(store.active_id.as_deref(), Some("old"));
    }

    #[test]
    fn a_stripped_copy_beside_the_real_one_is_dropped() {
        let mut store = ConnectionStore {
            connections: vec![
                stripped(postgres_seed("stale")),
                connection("other", "127.0.0.1", 3306, "root"),
                postgres_seed("kept"),
            ],
            active_id: Some("stale".to_string()),
        };
        let settled = store.settle_managed(postgres_seed("new"));
        assert_eq!(settled.dropped, vec!["stale".to_string()]);
        assert!(!settled.added && settled.repaired.is_none());
        let ids: Vec<_> = store.connections.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["other", "kept"]);
        assert_eq!(store.active_id.as_deref(), Some("kept"));
    }

    #[test]
    fn a_tunnelled_connection_at_the_same_address_is_left_alone() {
        // 127.0.0.1:5432 on a server reached over SSH is somebody else's
        // PostgreSQL, not Rezure's.
        let mut remote = connection("remote", "127.0.0.1", 5432, "postgres");
        remote.kind = ServerKind::Postgres;
        remote.ssh = Some(SshTunnel {
            host: "db.example.com".to_string(),
            port: 22,
            user: "deploy".to_string(),
            auth: SshAuth::Password,
        });
        let mut store = ConnectionStore {
            connections: vec![remote, postgres_seed("kept")],
            active_id: None,
        };
        assert!(!store.settle_managed(postgres_seed("new")).changed());
        assert_eq!(store.connections.len(), 2);
    }
}
