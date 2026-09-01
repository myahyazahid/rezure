//! Reads and manages the schemas inside whichever database profile is
//! currently active — see `services::db_profiles`.
//!
//! There's no MySQL driver crate in the dependency tree on purpose: every
//! server build Rezure runs ships its own client binaries (`mysql.exe`,
//! `mysqldump.exe`) next to it, so this module drives those instead of
//! adding a second, redundant way to speak the protocol. `--batch` makes
//! the client emit tab-separated rows with no box drawing, which is what
//! every read here parses.
//!
//! Both the port and the client binaries are resolved from the active
//! profile rather than fixed, so after a switch this module queries the
//! server that's actually running, using that build's own client.
//!
//! Rezure's own datadir is bootstrapped without a root password — it binds
//! to 127.0.0.1 only, and asking a developer to invent a password for a
//! throwaway local server just moves the secret into a config file.
//! [`server_info`] states that plainly rather than hiding it. An adopted
//! profile keeps whatever credentials its owner set; where those aren't
//! passwordless, its own client will say so.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::db_engine;
use super::db_profiles;
use super::projects::scan_projects;
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;
use crate::utils::paths;

pub const HOST: &str = "127.0.0.1";
pub const USER: &str = "root";
/// Used only when no profile is resolvable — the real port comes from
/// whichever profile is active.
pub const DEFAULT_PORT: u16 = 3306;

/// The port the currently active profile's server listens on.
///
/// A function rather than a constant because the profile switcher lets each
/// profile carry its own port: reading a fixed 3306 here would point every
/// query on the Databases page at whatever happened to own that port,
/// which after a switch may not be Rezure's server at all.
pub fn port() -> u16 {
    db_profiles::active()
        .map(|profile| profile.port)
        .unwrap_or(DEFAULT_PORT)
}

/// Schemas MariaDB itself owns — never listed, never droppable.
const SYSTEM_SCHEMAS: [&str; 4] = ["mysql", "information_schema", "performance_schema", "sys"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub name: String,
    pub collation: String,
    pub table_count: u64,
    /// Data + index bytes, as `information_schema` reports them. Zero for
    /// an empty schema, and approximate for InnoDB by nature — it's a
    /// storage estimate, not a byte count.
    pub size_bytes: u64,
    /// The project domain this database appears to belong to, matched by
    /// name — see [`used_by`].
    pub used_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Always false today, but stated explicitly so the UI shows the real
    /// state instead of hard-coding "no password" into a label.
    pub has_password: bool,
    /// Ready to paste into a client that takes a connection string.
    pub dsn: String,
}

pub fn server_info() -> ServerInfo {
    ServerInfo {
        host: HOST.to_string(),
        port: port(),
        user: USER.to_string(),
        has_password: false,
        dsn: format!("mysql://{USER}@{HOST}:{}", port()),
    }
}

/// A schema or collation name that's safe to splice into SQL.
///
/// Identifiers can't be bound as parameters — `CREATE DATABASE ?` isn't a
/// thing — so the only defence is to refuse anything that isn't a plain
/// identifier in the first place. Deliberately stricter than MySQL's own
/// rules (which allow almost anything inside backticks): nothing a local
/// dev database legitimately needs is excluded, and no quoting or escaping
/// question can arise downstream.
fn validate_identifier(name: &str, kind: &str) -> Result<(), AppError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidDatabaseName {
            name: name.to_string(),
            kind: kind.to_string(),
        })
    }
}

/// The `bin` folder of the build actually serving right now.
///
/// Resolved through the active profile rather than the pinned MariaDB
/// manifest entry, so that after a switch to a MySQL profile the client
/// binaries used to query it come from that same MySQL build — mixing a
/// MariaDB client with a MySQL server is a source of confusing protocol
/// and authentication errors.
fn bin_dir() -> Result<PathBuf, AppError> {
    let profile = db_profiles::active()
        .ok_or_else(|| AppError::BinaryNotInstalled("Database".to_string()))?;
    let exe = db_profiles::resolve_server_exe(&profile)?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Io("the database server has no parent directory".to_string()))
}

/// One of the client binaries that ships alongside the server. The names
/// are engine-neutral — see `services::db_engine`.
fn client(name: &str) -> Result<PathBuf, AppError> {
    let exe = bin_dir()?.join(name);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(AppError::BinaryNotInstalled(format!("the server's {name}")))
    }
}

/// The interactive console client, for handing a developer a shell that's
/// already connected — see `db_clients::open_console`.
pub fn console_client() -> Result<PathBuf, AppError> {
    client(db_engine::CLIENT_EXE)
}

fn base_args() -> Vec<String> {
    vec![
        "-h".to_string(),
        HOST.to_string(),
        "-P".to_string(),
        port().to_string(),
        "-u".to_string(),
        USER.to_string(),
    ]
}

/// Turns a failed client run into an error carrying the server's own
/// message — `ERROR 1049 (42000): Unknown database 'x'` is far more useful
/// to show than "command failed with exit code 1".
fn client_error(stderr: &[u8], fallback: &str) -> AppError {
    let message = String::from_utf8_lossy(stderr);
    let message = message
        .lines()
        .find(|line| line.contains("ERROR"))
        .unwrap_or_else(|| message.trim())
        .trim();
    AppError::DatabaseQueryFailed(if message.is_empty() {
        fallback.to_string()
    } else {
        message.to_string()
    })
}

/// Runs `sql` and returns its rows already split on tabs.
///
/// `--skip-column-names` drops the header, so a caller's row indexes line
/// up with its `SELECT` list and nothing has to be skipped.
fn query(sql: &str) -> Result<Vec<Vec<String>>, AppError> {
    let output = Command::new(client(db_engine::CLIENT_EXE)?)
        .args(base_args())
        .args(["--batch", "--skip-column-names", "-e", sql])
        .hidden()
        .output()
        .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(client_error(&output.stderr, "the query failed"));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect())
}

/// Runs a statement that returns no rows.
fn execute(sql: &str) -> Result<(), AppError> {
    query(sql).map(|_| ())
}

/// Matches a schema to a project by name, so the list can show which site
/// a database belongs to.
///
/// Underscores and hyphens are treated as the same character: a folder
/// can't be named `shop_api` and produce `shop-api.test`, while a schema
/// named `shop-api` needs backticks in every query a developer writes — so
/// in practice the two spellings drift apart for the same project, and
/// matching them up is the whole point of the column.
fn used_by(schema: &str, projects: &[(String, String)]) -> Option<String> {
    let normalize = |s: &str| s.to_lowercase().replace('-', "_");
    let schema = normalize(schema);
    projects
        .iter()
        .find(|(id, _)| normalize(id) == schema)
        .map(|(_, domain)| domain.clone())
}

pub fn list_databases() -> Result<Vec<DatabaseInfo>, AppError> {
    // Best-effort: a project-scan hiccup should cost the "used by" column,
    // not the whole database list.
    let projects: Vec<(String, String)> = scan_projects()
        .map(|found| found.into_iter().map(|p| (p.id, p.domain)).collect())
        .unwrap_or_default();

    let excluded = SYSTEM_SCHEMAS
        .map(|schema| format!("'{schema}'"))
        .join(", ");

    // LEFT JOIN, not an inner one: a schema with no tables yet still has to
    // appear in the list (with a count of 0) rather than vanish from it.
    let rows = query(&format!(
        "SELECT s.schema_name, s.default_collation_name, COUNT(t.table_name), \
         COALESCE(SUM(t.data_length + t.index_length), 0) \
         FROM information_schema.schemata s \
         LEFT JOIN information_schema.tables t ON t.table_schema = s.schema_name \
         WHERE s.schema_name NOT IN ({excluded}) \
         GROUP BY s.schema_name, s.default_collation_name \
         ORDER BY s.schema_name"
    ))?;

    Ok(rows
        .into_iter()
        .filter(|row| row.len() >= 4)
        .map(|row| DatabaseInfo {
            used_by: used_by(&row[0], &projects),
            name: row[0].clone(),
            collation: row[1].clone(),
            table_count: row[2].parse().unwrap_or(0),
            size_bytes: row[3].parse().unwrap_or(0),
        })
        .collect())
}

/// The collations offered in the "New database" dialog, newest-friendly
/// first. Read from the server rather than hard-coded, so switching the
/// bundled MariaDB version can't leave a stale list behind.
pub fn list_collations() -> Result<Vec<String>, AppError> {
    let rows = query(
        "SELECT collation_name FROM information_schema.collations \
         WHERE character_set_name IN ('utf8mb4', 'utf8mb3') \
         ORDER BY character_set_name DESC, collation_name",
    )?;
    Ok(rows
        .into_iter()
        .filter_map(|row| row.into_iter().next())
        .collect())
}

pub fn create_database(name: &str, collation: &str) -> Result<(), AppError> {
    validate_identifier(name, "database")?;
    validate_identifier(collation, "collation")?;
    execute(&format!("CREATE DATABASE `{name}` COLLATE {collation}"))
}

pub fn drop_database(name: &str) -> Result<(), AppError> {
    validate_identifier(name, "database")?;
    if SYSTEM_SCHEMAS.contains(&name) {
        return Err(AppError::DatabaseQueryFailed(format!(
            "`{name}` is one of MariaDB's own schemas and can't be dropped"
        )));
    }
    execute(&format!("DROP DATABASE `{name}`"))
}

/// `%USERPROFILE%\rezure\dumps` — where exports land.
///
/// A fixed, documented folder rather than a save dialog: an export is
/// usually one step of "dump it, then do something with the file", and a
/// predictable path is easier to reach from a terminal afterwards than a
/// location the user has to remember choosing.
pub fn dumps_dir() -> Result<PathBuf, AppError> {
    paths::dumps()
}

/// Dumps `name` to a timestamped `.sql` file and returns its path.
pub fn export_database(name: &str) -> Result<PathBuf, AppError> {
    validate_identifier(name, "database")?;

    let dir = dumps_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    let dest = dir.join(format!("{name}-{}.sql", timestamp()));

    // `mariadb-dump` writes the dump to stdout, so this redirects it into
    // the file rather than passing a path the client would have to quote.
    let file = std::fs::File::create(&dest)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dest.display())))?;
    let output = Command::new(client(db_engine::DUMP_EXE)?)
        .args(base_args())
        .args(["--databases", name])
        .stdout(file)
        .hidden()
        .output()
        .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;

    if !output.status.success() {
        // Don't leave a half-written or empty .sql behind looking like a
        // successful export.
        let _ = std::fs::remove_file(&dest);
        return Err(client_error(&output.stderr, "the export failed"));
    }
    Ok(dest)
}

/// Pipes a `.sql` file into `name`, creating the database if it isn't
/// there yet — importing a dump into a database you have to remember to
/// create first is a papercut with no upside.
pub fn import_sql(name: &str, file: &Path) -> Result<(), AppError> {
    validate_identifier(name, "database")?;
    if !file.is_file() {
        return Err(AppError::Io(format!("no such file: {}", file.display())));
    }
    execute(&format!("CREATE DATABASE IF NOT EXISTS `{name}`"))?;

    let input = std::fs::File::open(file)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", file.display())))?;
    let output = Command::new(client(db_engine::CLIENT_EXE)?)
        .args(base_args())
        .arg(name)
        .stdin(input)
        .hidden()
        .output()
        .map_err(|e| AppError::DatabaseQueryFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(client_error(&output.stderr, "the import failed"));
    }
    Ok(())
}

/// `YYYYMMDD-HHMMSS` in UTC, for export filenames.
///
/// Hand-rolled from a Unix timestamp rather than pulling in a date crate
/// for one format string — it's only ever read as "which dump is newer".
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// Days-since-epoch to a calendar date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_identifiers_are_accepted() {
        for name in ["blog", "shop_api", "client-cms", "db2"] {
            assert!(validate_identifier(name, "database").is_ok(), "{name}");
        }
    }

    /// The whole point of the check — an identifier can't be bound as a
    /// parameter, so anything that could end a statement and start another
    /// has to be refused before it reaches the client.
    #[test]
    fn identifiers_that_could_break_out_of_a_statement_are_refused() {
        for name in [
            "blog`; DROP DATABASE `mysql",
            "blog; SELECT 1",
            "blog'",
            "blog db",
            "",
            "café",
        ] {
            assert!(
                validate_identifier(name, "database").is_err(),
                "{name:?} must be refused"
            );
        }
    }

    #[test]
    fn an_over_long_identifier_is_refused() {
        assert!(validate_identifier(&"a".repeat(65), "database").is_err());
    }

    #[test]
    fn dropping_a_system_schema_is_refused_before_it_reaches_the_server() {
        for schema in SYSTEM_SCHEMAS {
            assert!(drop_database(schema).is_err(), "{schema}");
        }
    }

    #[test]
    fn used_by_matches_a_project_across_underscores_and_hyphens() {
        let projects = vec![
            ("shop-api".to_string(), "shop-api.test".to_string()),
            ("blog".to_string(), "blog.test".to_string()),
        ];
        assert_eq!(
            used_by("shop_api", &projects).as_deref(),
            Some("shop-api.test")
        );
        assert_eq!(used_by("blog", &projects).as_deref(), Some("blog.test"));
        assert_eq!(used_by("sandbox", &projects), None);
    }

    #[test]
    fn timestamps_are_sortable_and_the_right_shape() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), 15, "{stamp}");
        assert_eq!(&stamp[8..9], "-");
        assert!(stamp.chars().filter(|c| c.is_ascii_digit()).count() == 14);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // A leap day, the case an off-by-one in the algorithm would break.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    /// Talks to a really-running MariaDB. Start it in Rezure first, then:
    /// `cargo test --lib services::database::tests::round_trip_against_a_real_server -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn round_trip_against_a_real_server() {
        let name = "rezure_selftest_db";
        let _ = drop_database(name);

        create_database(name, "utf8mb4_unicode_ci").unwrap();
        let found = list_databases().unwrap();
        let created = found
            .iter()
            .find(|db| db.name == name)
            .expect("the new database must show up in the list");
        assert_eq!(created.collation, "utf8mb4_unicode_ci");
        assert_eq!(created.table_count, 0);

        let dump = export_database(name).unwrap();
        assert!(dump.is_file(), "export must leave a real file behind");
        println!("dumped to {}", dump.display());

        drop_database(name).unwrap();
        assert!(
            !list_databases().unwrap().iter().any(|db| db.name == name),
            "the dropped database must be gone from the list"
        );
        let _ = std::fs::remove_file(dump);
    }
}
