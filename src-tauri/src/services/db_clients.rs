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
//! entry for a MySQL-family server, so that menu is never empty on a machine
//! with no GUI installed.
//!
//! # SQL Server
//!
//! Offered for SQL Server, each checked against the real client:
//!
//! * **SQL Server Management Studio** — Microsoft's documented `-S`, `-d`,
//!   `-E`/`-U`. Reaches LocalDB's named pipe directly.
//! * **TablePlus** — a `sqlserver://user@host:port/database` URL, which
//!   TablePlus for Windows opens even when it's already running. Over TCP
//!   only, and it can't do Windows Authentication, so it's offered only for
//!   a SQL login on a known port — for LocalDB, the bridge and the `rezure`
//!   login (see `services::localdb_bridge`).
//! * **DBeaver** — `-con` with `driver=microsoft` (its SQL Server driver's
//!   id; `sqlserver` is the *provider* id and is silently ignored), and
//!   `prop.encrypt=false` where the server offers no TLS — LocalDB through the
//!   bridge — since its JDBC driver otherwise insists. The first time,
//!   DBeaver asks to download that JDBC driver itself.
//!
//! HeidiSQL isn't offered for SQL Server: version 12.8 opened its session
//! manager on the last session for every form of its `-n=4` (MSSQL over
//! TCP) command line tried, and never connected.
//!
//! # PostgreSQL
//!
//! * **TablePlus** — a `postgresql://user@host:port/database` URL.
//! * **DBeaver** — `-con` with `driver=postgres-jdbc`, read out of its
//!   PostgreSQL plugin's `plugin.xml` (`postgresql` there is the "(Old)"
//!   driver it keeps for saved connections).
//! * **pgAdmin 4**, when installed — it has no command line to pass a
//!   connection, so it only opens.
//! * The `psql` console of an installed PostgreSQL build, always, like the
//!   MariaDB console for MySQL.
//!
//! HeidiSQL is again left out until its command line is checked against a
//! real server — the same reason it isn't offered for SQL Server.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::{connections, database, mssql, postgres, postgres_client};
use crate::config::connections::ServerKind;
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
    /// The kinds of server this client is offered for.
    kinds: &'static [ServerKind],
}

const MYSQL: &[ServerKind] = &[ServerKind::Mysql];
const ALL: &[ServerKind] = &[
    ServerKind::Mysql,
    ServerKind::Sqlserver,
    ServerKind::Postgres,
];
const SQLSERVER: &[ServerKind] = &[ServerKind::Sqlserver];
const POSTGRES: &[ServerKind] = &[ServerKind::Postgres];

/// `$LOCALAPPDATA`, `$PROGRAMFILES` and `$PROGRAMFILES(X86)` are expanded
/// at lookup time — hard-coding `C:\Program Files` breaks on any machine
/// that redirects them.
///
/// `C:\laragon\bin\…` is the one literal root: Laragon bundles DBeaver and
/// HeidiSQL there, and a developer moving over from Laragon often has no
/// other copy. Same fixed root `db_profiles::detect_laragon` looks under.
/// Listed after the standalone installs, which the user chose deliberately
/// and are usually the newer build.
const CANDIDATES: &[Candidate] = &[
    Candidate {
        id: "tableplus",
        name: "TablePlus",
        locations: &[
            // The installer asks: per-user lands here, all-users in
            // Program Files.
            r"$LOCALAPPDATA\Programs\TablePlus\TablePlus.exe",
            r"$PROGRAMFILES\TablePlus\TablePlus.exe",
        ],
        opens_database: true,
        kinds: ALL,
    },
    Candidate {
        id: "dbeaver",
        name: "DBeaver",
        locations: &[
            r"$PROGRAMFILES\DBeaver\dbeaver.exe",
            r"$PROGRAMFILES\DBeaverEE\dbeaver.exe",
            r"$LOCALAPPDATA\DBeaver\dbeaver.exe",
            r"$LOCALAPPDATA\Programs\DBeaver\dbeaver.exe",
            r"C:\laragon\bin\dbeaver\dbeaver.exe",
        ],
        opens_database: true,
        kinds: ALL,
    },
    Candidate {
        id: "heidisql",
        name: "HeidiSQL",
        locations: &[
            r"$PROGRAMFILES\HeidiSQL\heidisql.exe",
            r"$PROGRAMFILESX86\HeidiSQL\heidisql.exe",
            r"C:\laragon\bin\heidisql\heidisql.exe",
        ],
        opens_database: true,
        kinds: MYSQL,
    },
    Candidate {
        id: "workbench",
        name: "MySQL Workbench",
        locations: &[
            r"$PROGRAMFILES\MySQL\*\MySQLWorkbench.exe",
            r"$PROGRAMFILESX86\MySQL\*\MySQLWorkbench.exe",
        ],
        opens_database: false,
        kinds: MYSQL,
    },
    Candidate {
        id: "navicat",
        name: "Navicat",
        locations: &[
            r"$PROGRAMFILES\PremiumSoft\*\navicat.exe",
            r"$PROGRAMFILESX86\PremiumSoft\*\navicat.exe",
        ],
        opens_database: false,
        kinds: MYSQL,
    },
    Candidate {
        id: "ssms",
        name: "SQL Server Management Studio",
        locations: &[
            // SSMS 21 moved to a 64-bit build under Program Files; 20 and
            // older live under Program Files (x86).
            r"$PROGRAMFILES\Microsoft SQL Server Management Studio *\Release\Common7\IDE\SSMS.exe",
            r"$PROGRAMFILESX86\Microsoft SQL Server Management Studio *\Common7\IDE\Ssms.exe",
        ],
        opens_database: true,
        kinds: SQLSERVER,
    },
    Candidate {
        id: "pgadmin",
        name: "pgAdmin 4",
        locations: &[
            r"$PROGRAMFILES\pgAdmin 4\runtime\pgAdmin4.exe",
            r"$LOCALAPPDATA\Programs\pgAdmin 4\runtime\pgAdmin4.exe",
            // pgAdmin 6 and older nested a version folder.
            r"$PROGRAMFILES\pgAdmin 4\*\runtime\pgAdmin4.exe",
        ],
        opens_database: false,
        kinds: POSTGRES,
    },
];

/// The always-available fallback: the console client from the same zip as
/// the server.
const CLI_ID: &str = "mariadb-cli";

/// PostgreSQL's fallback: `psql` from an installed PostgreSQL build.
const PSQL_ID: &str = "psql-cli";

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

/// What the Databases page is reading, as a client on this machine would
/// have to reach it. Resolved once per menu or click, then handed to pure
/// functions — which is also what lets the tests pick a target instead of
/// reading whichever one this machine happens to have selected.
#[derive(Debug, Clone)]
enum Target {
    Mysql {
        host: String,
        port: u16,
        user: String,
    },
    /// The SQL Server connection, with its TCP endpoint when it has one.
    Sqlserver {
        connection: Box<crate::config::connections::Connection>,
        tcp: Option<mssql::TcpTarget>,
    },
    Postgres {
        host: String,
        port: u16,
        user: String,
    },
}

fn current_target() -> Target {
    match connections::active() {
        Some(connection) if connection.kind == ServerKind::Sqlserver => Target::Sqlserver {
            tcp: mssql::tcp_target(&connection),
            connection: Box::new(connection),
        },
        Some(connection) if connection.kind == ServerKind::Postgres => {
            let (host, port, user) = postgres_client::endpoint(&connection);
            Target::Postgres { host, port, user }
        }
        _ => {
            let (host, port, user) = database::endpoint();
            Target::Mysql { host, port, user }
        }
    }
}

impl Target {
    fn kind(&self) -> ServerKind {
        match self {
            Target::Mysql { .. } => ServerKind::Mysql,
            Target::Sqlserver { .. } => ServerKind::Sqlserver,
            Target::Postgres { .. } => ServerKind::Postgres,
        }
    }

    /// The TCP endpoint TablePlus and DBeaver need for SQL Server: a SQL
    /// login on a known port — see the module docs.
    fn sqlserver_login_endpoint(&self) -> Option<&mssql::TcpTarget> {
        match self {
            Target::Sqlserver { tcp: Some(tcp), .. }
                if tcp.user.is_some() && tcp.port.is_some() =>
            {
                Some(tcp)
            }
            _ => None,
        }
    }
}

/// Whether `candidate` is offered for `target` at all, before asking whether
/// it's installed.
fn offered(candidate: &Candidate, target: &Target) -> bool {
    if !candidate.kinds.contains(&target.kind()) {
        return false;
    }
    match target {
        Target::Sqlserver { .. } if candidate.id != "ssms" => {
            target.sqlserver_login_endpoint().is_some()
        }
        _ => true,
    }
}

/// The menu for `target`, given a way to tell whether a client is installed.
fn menu_for(target: &Target, installed: impl Fn(&Candidate) -> bool) -> Vec<DbClientInfo> {
    let mut found: Vec<DbClientInfo> = CANDIDATES
        .iter()
        .filter(|candidate| offered(candidate, target) && installed(candidate))
        .map(|candidate| DbClientInfo {
            id: candidate.id.to_string(),
            name: candidate.name.to_string(),
            opens_database: candidate.opens_database,
        })
        .collect();

    match target.kind() {
        ServerKind::Mysql => found.push(DbClientInfo {
            id: CLI_ID.to_string(),
            name: "MariaDB console (bundled)".to_string(),
            opens_database: true,
        }),
        ServerKind::Postgres => found.push(DbClientInfo {
            id: PSQL_ID.to_string(),
            name: "psql console".to_string(),
            opens_database: true,
        }),
        ServerKind::Sqlserver => {}
    }
    found
}

/// Every SQL client Rezure can find for the kind of server the Databases
/// page is reading, plus — for MySQL and PostgreSQL — a console one.
pub fn detect() -> Vec<DbClientInfo> {
    menu_for(&current_target(), |candidate| locate(candidate).is_some())
}

/// TablePlus's connection URL for a SQL Server login.
fn tableplus_sqlserver_url(target: &mssql::TcpTarget, database: &str) -> Option<String> {
    let user = target.user.as_ref()?;
    let port = target.port?;
    let mut url =
        url::Url::parse(&format!("sqlserver://{}:{port}/{database}", target.host)).ok()?;
    url.set_username(user).ok()?;
    Some(url.to_string())
}

/// DBeaver's `-con` spec for SQL Server. `|` separates its fields and `=`
/// its names from values, so a host or user holding either would split the
/// spec — those get no spec rather than a wrong one.
fn dbeaver_sqlserver_spec(target: &mssql::TcpTarget, database: &str) -> Option<String> {
    let user = target.user.as_ref()?;
    if [target.host.as_str(), user.as_str()]
        .iter()
        .any(|value| value.contains(['|', '=']))
    {
        return None;
    }
    let mut spec = format!("driver=microsoft|host={}", target.host);
    if let Some(port) = target.port {
        spec.push_str(&format!("|port={port}"));
    }
    spec.push_str(&format!(
        "|database={database}|user={user}|save=false|connect=true"
    ));
    if target.no_encryption {
        spec.push_str("|prop.encrypt=false");
    } else if target.trust_certificate {
        spec.push_str("|prop.trustServerCertificate=true");
    }
    Some(spec)
}

/// TablePlus's connection URL for PostgreSQL.
fn tableplus_postgres_url(host: &str, port: u16, user: &str, database: &str) -> Option<String> {
    let mut url = url::Url::parse(&format!("postgresql://{host}:{port}/{database}")).ok()?;
    url.set_username(user).ok()?;
    Some(url.to_string())
}

/// DBeaver's `-con` spec for PostgreSQL — `None` for a value that would split
/// it, as for SQL Server.
fn dbeaver_postgres_spec(host: &str, port: u16, user: &str, database: &str) -> Option<String> {
    if [host, user].iter().any(|value| value.contains(['|', '='])) {
        return None;
    }
    Some(format!(
        "driver=postgres-jdbc|host={host}|port={port}|database={database}|user={user}|save=false|connect=true"
    ))
}

/// SSMS's documented flags: `-S` server, `-d` database, then `-E` for
/// Windows Authentication or `-U` for a SQL login, whose password SSMS asks
/// for itself. SSMS reaches LocalDB's pipe directly, so it never needs the
/// bridge.
fn ssms_args(connection: &crate::config::connections::Connection, database: &str) -> Vec<String> {
    let mut args = vec![
        "-S".to_string(),
        mssql::client_server_name(connection),
        "-d".to_string(),
        database.to_string(),
    ];
    if connections::needs_no_password(connection) {
        args.push("-E".to_string());
    } else {
        args.push("-U".to_string());
        args.push(connection.user.clone());
    }
    args
}

/// The arguments that carry the connection, per client.
///
/// Every one of these is a documented command-line interface of the client
/// itself; each argument is passed as its own argv entry, so a database
/// name never has to be quoted or escaped into a longer string.
///
/// The endpoint comes from whichever target the Databases page is showing,
/// not from localhost: handing off while looking at a remote connection has
/// to open that server, or the menu quietly lies about what it opens. The
/// password is deliberately never passed — these clients prompt for it and
/// store it in their own vault, which is where it belongs.
fn args_for(target: &Target, id: &str, database: &str) -> Vec<String> {
    let (host, port, user) = match target {
        Target::Mysql { host, port, user } => (host, port, user),
        Target::Postgres { host, port, user } => {
            return match id {
                "tableplus" => tableplus_postgres_url(host, *port, user, database)
                    .into_iter()
                    .collect(),
                "dbeaver" => dbeaver_postgres_spec(host, *port, user, database)
                    .map(|spec| vec!["-con".to_string(), spec])
                    .unwrap_or_default(),
                // pgAdmin takes no connection on its command line.
                _ => Vec::new(),
            };
        }
        Target::Sqlserver { connection, tcp } => {
            return match (id, tcp) {
                ("ssms", _) => ssms_args(connection, database),
                ("tableplus", Some(tcp)) => {
                    tableplus_sqlserver_url(tcp, database).into_iter().collect()
                }
                ("dbeaver", Some(tcp)) => dbeaver_sqlserver_spec(tcp, database)
                    .map(|spec| vec!["-con".to_string(), spec])
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
        }
    };
    match id {
        // TablePlus takes a connection URL directly.
        "tableplus" => vec![format!("mysql://{user}@{host}:{port}/{database}")],
        // DBeaver's `-con` takes one pipe-separated spec. `save=false`
        // keeps Rezure from littering the user's DBeaver workspace with a
        // new saved connection on every click.
        "dbeaver" => vec![
            "-con".to_string(),
            format!(
                "driver=mariadb|host={host}|port={port}|database={database}|user={user}|save=false|connect=true"
            ),
        ],
        // Workbench's `-query` opens a connection to a server, with no way
        // to preselect a schema.
        "workbench" => vec!["-query".to_string(), format!("{user}@{host}:{port}")],
        // `-db` (long form `--databases`) is HeidiSQL's session "Databases"
        // field — it limits the tree to the schemas named, `;`-separated.
        // Undocumented in `--help`; read out of the option table in
        // heidisql.exe itself (`db | databases`, beside `h | host`).
        "heidisql" => vec![
            format!("-h={host}"),
            format!("-P={port}"),
            format!("-u={user}"),
            format!("-db={database}"),
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
    if client_id == PSQL_ID {
        return open_psql_console(&current_target(), database);
    }

    let candidate = CANDIDATES
        .iter()
        .find(|candidate| candidate.id == client_id)
        .ok_or_else(|| AppError::UnknownDbClient(client_id.to_string()))?;
    let exe = locate(candidate).ok_or_else(|| AppError::UnknownDbClient(client_id.to_string()))?;

    let target = current_target();
    // A client reaching Rezure's LocalDB over the bridge signs in as
    // `rezure`, which has to exist before it tries.
    if let Target::Sqlserver { connection, .. } = &target {
        if connection.managed && client_id != "ssms" {
            mssql::ensure_localdb_login()?;
        }
    }

    client_command(&exe)
        .args(args_for(&target, client_id, database))
        .spawn()
        .map(|_| ())
        .map_err(|e| AppError::OpenFailed {
            target: candidate.name.to_string(),
            reason: e.to_string(),
        })
}

/// A command for a GUI client, started from its own folder the way a
/// Start-menu shortcut or a double-click in Explorer starts it.
///
/// Inheriting Rezure's working directory instead breaks portable clients,
/// which keep their state relative to it. The zip build of DBeaver — the one
/// Laragon bundles — created its `workspace\` there: inside `src-tauri\`
/// under `tauri dev`, whose file watcher then restarted Rezure (it looked
/// like a crash), and for an installed Rezure in its own install folder,
/// which it can't write to.
fn client_command(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    command
}

/// Opens the bundled `mariadb.exe` in its own console, already connected
/// and `USE`-ing `database`.
///
/// Spawned directly with `CREATE_NEW_CONSOLE` rather than through `cmd
/// /C start`: no shell parses this command line, so the exe path and the
/// database name can't be re-split on spaces the way `start` would.
fn open_console(database: &str) -> Result<(), AppError> {
    let exe = database::console_client()?;

    let (host, port, user) = database::endpoint();
    let mut cmd = Command::new(&exe);
    cmd.args(["-h", &host])
        .args(["-P", &port.to_string()])
        .args(["-u", &user])
        .arg(database);

    // `-p` with no value makes the client prompt in its own console, so the
    // password reaches it without passing through an argument list.
    if database::endpoint_prompts_for_password() {
        cmd.arg("-p");
    }

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

/// Opens `psql` in its own console, connected to `database` — the
/// PostgreSQL counterpart of [`open_console`]. `psql` asks for a password
/// there itself when the server wants one.
///
/// The database goes in `PGDATABASE` rather than as an argument, which
/// `psql` would read as a whole connection string if it held an `=`.
fn open_psql_console(target: &Target, database: &str) -> Result<(), AppError> {
    let Target::Postgres { host, port, user } = target else {
        return Err(AppError::UnknownDbClient(PSQL_ID.to_string()));
    };
    let exe = postgres::client_bin_dir()?.join(postgres::CLIENT_EXE);
    let mut cmd = Command::new(&exe);
    cmd.args(["-h", host])
        .args(["-p", &port.to_string()])
        .args(["-U", user])
        .env("PGDATABASE", database)
        .env("PGAPPNAME", "Rezure psql");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }

    cmd.spawn().map(|_| ()).map_err(|e| AppError::OpenFailed {
        target: "the psql console".to_string(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_postgres() -> Target {
        Target::Postgres {
            host: "127.0.0.1".to_string(),
            port: 5432,
            user: "postgres".to_string(),
        }
    }

    #[test]
    fn postgres_offers_tableplus_dbeaver_pgadmin_and_psql() {
        let ids: Vec<_> = menu_for(&local_postgres(), |_| true)
            .into_iter()
            .map(|client| client.id)
            .collect();
        assert_eq!(ids, ["tableplus", "dbeaver", "pgadmin", PSQL_ID]);
        // Never the MySQL tools, and psql even with no GUI installed.
        let bare: Vec<_> = menu_for(&local_postgres(), |_| false)
            .into_iter()
            .map(|client| client.id)
            .collect();
        assert_eq!(bare, [PSQL_ID]);
    }

    #[test]
    fn postgres_clients_get_their_own_connection_arguments() {
        assert_eq!(
            args_for(&local_postgres(), "tableplus", "shop"),
            ["postgresql://postgres@127.0.0.1:5432/shop"]
        );
        assert_eq!(
            args_for(&local_postgres(), "dbeaver", "shop"),
            [
                "-con",
                "driver=postgres-jdbc|host=127.0.0.1|port=5432|database=shop|user=postgres|save=false|connect=true"
            ]
        );
        assert!(args_for(&local_postgres(), "pgadmin", "shop").is_empty());
        assert_eq!(dbeaver_postgres_spec("h|x", 5432, "u", "d"), None);
    }

    #[test]
    fn a_client_starts_in_its_own_folder() {
        let exe = Path::new(r"C:\laragon\bin\dbeaver\dbeaver.exe");
        assert_eq!(
            client_command(exe).get_current_dir(),
            Some(Path::new(r"C:\laragon\bin\dbeaver"))
        );
    }

    fn local_mysql() -> Target {
        Target::Mysql {
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
        }
    }

    fn localdb() -> Target {
        Target::Sqlserver {
            connection: Box::new(crate::config::connections::Connection {
                id: "localdb".to_string(),
                name: "SQL Server LocalDB".to_string(),
                host: r"(localdb)\Rezure".to_string(),
                port: 0,
                user: String::new(),
                kind: ServerKind::Sqlserver,
                engine: crate::services::db_engine::Engine::MariaDb,
                windows_auth: true,
                trust_server_certificate: false,
                managed: true,
                tls_mode: Default::default(),
                read_only: false,
                save_password: false,
                ssh: None,
                last_used_at: None,
            }),
            tcp: Some(localdb_target()),
        }
    }

    #[test]
    fn the_bundled_console_is_always_offered() {
        assert!(
            menu_for(&local_mysql(), |_| false)
                .iter()
                .any(|client| client.id == CLI_ID),
            "the menu must never be empty, even with no GUI client installed"
        );
    }

    #[test]
    fn localdb_offers_tableplus_dbeaver_and_ssms_but_no_mysql_console() {
        let ids: Vec<_> = menu_for(&localdb(), |_| true)
            .into_iter()
            .map(|client| client.id)
            .collect();
        assert_eq!(ids, ["tableplus", "dbeaver", "ssms"]);
    }

    /// Without the bridge there's no TCP endpoint, and only SSMS can still
    /// reach the pipe.
    #[test]
    fn without_a_tcp_endpoint_only_ssms_is_offered_for_sql_server() {
        let Target::Sqlserver { connection, .. } = localdb() else {
            unreachable!()
        };
        let no_bridge = Target::Sqlserver {
            connection,
            tcp: None,
        };
        let ids: Vec<_> = menu_for(&no_bridge, |_| true)
            .into_iter()
            .map(|client| client.id)
            .collect();
        assert_eq!(ids, ["ssms"]);
    }

    #[test]
    fn ssms_reaches_localdb_by_its_pipe_with_windows_authentication() {
        assert_eq!(
            args_for(&localdb(), "ssms", "testing"),
            ["-S", r"(localdb)\Rezure", "-d", "testing", "-E"]
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
        let args = args_for(&local_mysql(), "tableplus", "blog");
        assert_eq!(args, vec!["mysql://root@127.0.0.1:3306/blog"]);
    }

    #[test]
    fn dbeaver_gets_one_spec_argument_not_a_shell_string() {
        let args = args_for(&local_mysql(), "dbeaver", "shop_api");
        assert_eq!(args.len(), 2, "the spec must stay a single argv entry");
        assert_eq!(args[0], "-con");
        assert!(args[1].contains("database=shop_api"), "{}", args[1]);
        assert!(args[1].contains("save=false"), "{}", args[1]);
    }

    /// Only the clients checked against a real SQL Server are offered for
    /// one — a menu entry that opens the wrong thing is worse than none.
    #[test]
    fn only_the_checked_clients_are_offered_for_sql_server() {
        let for_sqlserver: Vec<_> = CANDIDATES
            .iter()
            .filter(|c| c.kinds.contains(&ServerKind::Sqlserver))
            .map(|c| c.id)
            .collect();
        assert_eq!(for_sqlserver, ["tableplus", "dbeaver", "ssms"]);
        let for_postgres: Vec<_> = CANDIDATES
            .iter()
            .filter(|c| c.kinds.contains(&ServerKind::Postgres))
            .map(|c| c.id)
            .collect();
        assert_eq!(for_postgres, ["tableplus", "dbeaver", "pgadmin"]);
        let ssms = CANDIDATES.iter().find(|c| c.id == "ssms").unwrap();
        assert!(!ssms.kinds.contains(&ServerKind::Mysql));
    }

    fn localdb_target() -> mssql::TcpTarget {
        mssql::TcpTarget {
            host: "127.0.0.1".to_string(),
            port: Some(14330),
            user: Some("rezure".to_string()),
            no_encryption: true,
            trust_certificate: false,
        }
    }

    /// The exact URL TablePlus for Windows was seen connecting with.
    #[test]
    fn tableplus_gets_a_sqlserver_url_with_the_login_and_database() {
        assert_eq!(
            tableplus_sqlserver_url(&localdb_target(), "testing").as_deref(),
            Some("sqlserver://rezure@127.0.0.1:14330/testing")
        );
        let windows = mssql::TcpTarget {
            user: None,
            ..localdb_target()
        };
        assert_eq!(tableplus_sqlserver_url(&windows, "testing"), None);
    }

    #[test]
    fn dbeaver_is_given_its_driver_id_and_told_not_to_insist_on_tls() {
        let spec = dbeaver_sqlserver_spec(&localdb_target(), "testing").unwrap();
        assert!(spec.starts_with("driver=microsoft|"), "{spec}");
        assert!(
            spec.contains("|port=14330|database=testing|user=rezure|"),
            "{spec}"
        );
        assert!(spec.ends_with("|prop.encrypt=false"), "{spec}");

        let named = mssql::TcpTarget {
            host: r"office\SQLEXPRESS".to_string(),
            port: None,
            no_encryption: false,
            trust_certificate: true,
            ..localdb_target()
        };
        let spec = dbeaver_sqlserver_spec(&named, "erp").unwrap();
        assert!(!spec.contains("port="), "{spec}");
        assert!(
            spec.ends_with("|prop.trustServerCertificate=true"),
            "{spec}"
        );

        let breaking = mssql::TcpTarget {
            user: Some("a|b".to_string()),
            ..localdb_target()
        };
        assert_eq!(dbeaver_sqlserver_spec(&breaking, "erp"), None);
    }

    #[test]
    fn heidisql_is_handed_the_database_as_its_own_argument() {
        let args = args_for(&local_mysql(), "heidisql", "shop_api");
        assert_eq!(args.last().map(String::as_str), Some("-db=shop_api"));
    }

    /// Workbench and Navicat can't be pointed at a schema, and say so —
    /// the UI relies on this flag to set expectations rather than opening
    /// something other than what was clicked.
    #[test]
    fn clients_that_cannot_preselect_a_schema_are_marked_as_such() {
        for id in ["workbench", "navicat"] {
            let candidate = CANDIDATES.iter().find(|c| c.id == id).unwrap();
            assert!(!candidate.opens_database, "{id}");
            assert!(
                !args_for(&local_mysql(), id, "blog")
                    .iter()
                    .any(|a| a.contains("blog")),
                "{id} must not claim to open a schema it can't"
            );
        }
    }

    /// The Laragon entries carry no `$VAR` prefix — they must come through
    /// `expand` untouched, not be dropped or re-rooted.
    #[test]
    fn a_literal_location_expands_to_itself() {
        let literal = r"C:\laragon\bin\heidisql\heidisql.exe";
        assert_eq!(expand(literal), Some(PathBuf::from(literal)));
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
