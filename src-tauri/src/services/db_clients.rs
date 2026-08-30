//! Finds the SQL clients already installed on the machine and hands one of
//! them a ready-made connection to Rezure's MariaDB.
//!
//! Rezure deliberately doesn't ship a database GUI of its own. Developers
//! already have a favourite — DBeaver, TablePlus, HeidiSQL, Workbench —
//! with their own saved queries and layout, and a bundled half-clone of it
//! would be one more thing to maintain and one more thing to learn. So
//! "Open" detects what's actually installed and lets the user pick.
//!
//! Detection is by well-known install path, not by registry scraping: an
//! install path is stable, cheap to check, and easy for a contributor to
//! extend with one more entry in [`CANDIDATES`].
//!
//! The bundled `mariadb.exe` console client is always offered as a last
//! entry, so the menu is never empty on a machine with no GUI installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::database::{self, HOST, USER};
use crate::utils::error::AppError;

/// Opens the process in its own console window instead of inheriting
/// Rezure's (which, as a GUI app, doesn't have one).
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbClientInfo {
    pub id: String,
    pub name: String,
    /// Whether the client can be pointed straight at one database. The
    /// ones that can't still open on the right server — the UI says so
    /// rather than silently opening something else than asked for.
    pub opens_database: bool,
}

/// Where a client might be installed, and what it's called.
///
/// `locations` are checked in order and may contain a single `*`, which
/// matches one path segment — enough for the versioned install folders
/// (`MySQL Workbench 8.0 CE`) without pulling in a glob crate.
struct Candidate {
    id: &'static str,
    name: &'static str,
    locations: &'static [&'static str],
    opens_database: bool,
}

/// `$LOCALAPPDATA`, `$PROGRAMFILES` and `$PROGRAMFILES(X86)` are expanded
/// at lookup time — hard-coding `C:\Program Files` breaks on any machine
/// that redirects them.
const CANDIDATES: &[Candidate] = &[
    Candidate {
        id: "tableplus",
        name: "TablePlus",
        locations: &[r"$LOCALAPPDATA\Programs\TablePlus\TablePlus.exe"],
        opens_database: true,
    },
    Candidate {
        id: "dbeaver",
        name: "DBeaver",
        locations: &[
            r"$PROGRAMFILES\DBeaver\dbeaver.exe",
            r"$PROGRAMFILES\DBeaverEE\dbeaver.exe",
            r"$LOCALAPPDATA\DBeaver\dbeaver.exe",
            r"$LOCALAPPDATA\Programs\DBeaver\dbeaver.exe",
        ],
        opens_database: true,
    },
    Candidate {
        id: "heidisql",
        name: "HeidiSQL",
        locations: &[
            r"$PROGRAMFILES\HeidiSQL\heidisql.exe",
            r"$PROGRAMFILESX86\HeidiSQL\heidisql.exe",
        ],
        opens_database: false,
    },
    Candidate {
        id: "workbench",
        name: "MySQL Workbench",
        locations: &[
            r"$PROGRAMFILES\MySQL\*\MySQLWorkbench.exe",
            r"$PROGRAMFILESX86\MySQL\*\MySQLWorkbench.exe",
        ],
        opens_database: false,
    },
    Candidate {
        id: "navicat",
        name: "Navicat",
        locations: &[
            r"$PROGRAMFILES\PremiumSoft\*\navicat.exe",
            r"$PROGRAMFILESX86\PremiumSoft\*\navicat.exe",
        ],
        opens_database: false,
    },
];

/// The always-available fallback: the console client from the same zip as
/// the server.
const CLI_ID: &str = "mariadb-cli";

fn expand(location: &str) -> Option<PathBuf> {
    let (var, rest) = match location {
        l if l.starts_with("$LOCALAPPDATA\\") => (dirs::data_local_dir()?, &l[14..]),
        l if l.starts_with("$PROGRAMFILESX86\\") => (
            PathBuf::from(std::env::var_os("ProgramFiles(x86)")?),
            &l[17..],
        ),
        l if l.starts_with("$PROGRAMFILES\\") => {
            (PathBuf::from(std::env::var_os("ProgramFiles")?), &l[14..])
        }
        l => (PathBuf::new(), l),
    };
    Some(var.join(rest))
}

/// Resolves a location to a real file, expanding a single `*` segment by
/// listing its parent. Returns the first match — a machine with two
/// Workbench versions gets one of them, not an error.
fn resolve(location: &str) -> Option<PathBuf> {
    let path = expand(location)?;
    let text = path.to_str()?;

    let Some((before, after)) = text.split_once('*') else {
        return path.is_file().then_some(path);
    };

    let parent = Path::new(before.trim_end_matches(['\\', '/']));
    let tail = after.trim_start_matches(['\\', '/']);
    let mut matches: Vec<PathBuf> = std::fs::read_dir(parent)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join(tail))
        .filter(|candidate| candidate.is_file())
        .collect();
    matches.sort();
    matches.pop()
}

fn locate(candidate: &Candidate) -> Option<PathBuf> {
    candidate.locations.iter().find_map(|l| resolve(l))
}

/// Every SQL client Rezure can find, plus the bundled console one.
pub fn detect() -> Vec<DbClientInfo> {
    let mut found: Vec<DbClientInfo> = CANDIDATES
        .iter()
        .filter(|candidate| locate(candidate).is_some())
        .map(|candidate| DbClientInfo {
            id: candidate.id.to_string(),
            name: candidate.name.to_string(),
            opens_database: candidate.opens_database,
        })
        .collect();

    found.push(DbClientInfo {
        id: CLI_ID.to_string(),
        name: "MariaDB console (bundled)".to_string(),
        opens_database: true,
    });
    found
}

/// The arguments that carry the connection, per client.
///
/// Every one of these is a documented command-line interface of the client
/// itself; each argument is passed as its own argv entry, so a database
/// name never has to be quoted or escaped into a longer string.
fn connection_args(id: &str, database: &str) -> Vec<String> {
    match id {
        // TablePlus takes a connection URL directly.
        "tableplus" => vec![format!("mysql://{USER}@{HOST}:{}/{database}", database::port())],
        // DBeaver's `-con` takes one pipe-separated spec. `save=false`
        // keeps Rezure from littering the user's DBeaver workspace with a
        // new saved connection on every click.
        "dbeaver" => vec![
            "-con".to_string(),
            format!(
                "driver=mariadb|host={HOST}|port={}|database={database}|user={USER}|save=false|connect=true", database::port()
            ),
        ],
        // Workbench's `-query` opens a connection to a server, with no way
        // to preselect a schema.
        "workbench" => vec!["-query".to_string(), format!("{USER}@{HOST}:{}", database::port())],
        "heidisql" => vec![
            format!("-h={HOST}"),
            format!("-P={}", database::port()),
            format!("-u={USER}"),
        ],
        // Navicat has no documented connection flags — it just opens.
        _ => Vec::new(),
    }
}

/// Launches `client_id` pointed at `database`.
pub fn open(client_id: &str, database: &str) -> Result<(), AppError> {
    if client_id == CLI_ID {
        return open_console(database);
    }

    let candidate = CANDIDATES
        .iter()
        .find(|candidate| candidate.id == client_id)
        .ok_or_else(|| AppError::UnknownDbClient(client_id.to_string()))?;
    let exe = locate(candidate).ok_or_else(|| AppError::UnknownDbClient(client_id.to_string()))?;

    Command::new(&exe)
        .args(connection_args(client_id, database))
        .spawn()
        .map(|_| ())
        .map_err(|e| AppError::OpenFailed {
            target: candidate.name.to_string(),
            reason: e.to_string(),
        })
}

/// Opens the bundled `mariadb.exe` in its own console, already connected
/// and `USE`-ing `database`.
///
/// Spawned directly with `CREATE_NEW_CONSOLE` rather than through `cmd
/// /C start`: no shell parses this command line, so the exe path and the
/// database name can't be re-split on spaces the way `start` would.
fn open_console(database: &str) -> Result<(), AppError> {
    let exe = database::console_client()?;

    let mut cmd = Command::new(&exe);
    cmd.args(["-h", HOST])
        .args(["-P", &database::port().to_string()])
        .args(["-u", USER])
        .arg(database);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }

    cmd.spawn().map(|_| ()).map_err(|e| AppError::OpenFailed {
        target: "the MariaDB console".to_string(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_console_is_always_offered() {
        assert!(
            detect().iter().any(|client| client.id == CLI_ID),
            "the menu must never be empty, even with no GUI client installed"
        );
    }

    #[test]
    fn an_unknown_client_id_is_an_error_not_a_launch() {
        assert!(matches!(
            open("not-a-real-client", "blog"),
            Err(AppError::UnknownDbClient(_))
        ));
    }

    #[test]
    fn tableplus_gets_a_connection_url_naming_the_database() {
        let args = connection_args("tableplus", "blog");
        assert_eq!(args, vec!["mysql://root@127.0.0.1:3306/blog"]);
    }

    #[test]
    fn dbeaver_gets_one_spec_argument_not_a_shell_string() {
        let args = connection_args("dbeaver", "shop_api");
        assert_eq!(args.len(), 2, "the spec must stay a single argv entry");
        assert_eq!(args[0], "-con");
        assert!(args[1].contains("database=shop_api"), "{}", args[1]);
        assert!(args[1].contains("save=false"), "{}", args[1]);
    }

    /// Workbench and HeidiSQL can't be pointed at a schema, and say so —
    /// the UI relies on this flag to set expectations rather than opening
    /// something other than what was clicked.
    #[test]
    fn clients_that_cannot_preselect_a_schema_are_marked_as_such() {
        for id in ["workbench", "heidisql", "navicat"] {
            let candidate = CANDIDATES.iter().find(|c| c.id == id).unwrap();
            assert!(!candidate.opens_database, "{id}");
            assert!(
                !connection_args(id, "blog")
                    .iter()
                    .any(|a| a.contains("blog")),
                "{id} must not claim to open a schema it can't"
            );
        }
    }

    #[test]
    fn a_location_with_no_wildcard_that_does_not_exist_resolves_to_nothing() {
        assert!(resolve(r"$PROGRAMFILES\DefinitelyNotInstalled9f3a\nope.exe").is_none());
    }

    /// Prints what's actually installed on this machine. Run with:
    /// `cargo test --lib services::db_clients::tests::print_detected -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_detected() {
        for client in detect() {
            println!(
                "{} | {} | opens db: {}",
                client.id, client.name, client.opens_database
            );
        }
    }
}
