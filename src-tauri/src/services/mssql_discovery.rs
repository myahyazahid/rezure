//! SQL Server instances already on this machine — the SQL Server counterpart
//! of `db_profiles::detect`, which finds Laragon's and XAMPP's MariaDB data.
//!
//! # Why this is a list of *connections*, not data directories
//!
//! A MariaDB datadir can be adopted: Rezure starts its own `mysqld` on it. A
//! SQL Server install can't — it runs as a Windows service Rezure doesn't own
//! and won't start or stop, and its files are locked while it does. So what a
//! scan can usefully offer is the way in: "this instance exists, here is its
//! address", one click away from the Add connection form with the host
//! already filled in.
//!
//! # Where they come from
//!
//! * **Installed editions** (Express, Developer, Standard…) — the registry
//!   lists every installed instance under `Instance Names\SQL`, and each one
//!   is a Windows service: `MSSQLSERVER` for the default instance,
//!   `MSSQL$<NAME>` for a named one.
//! * **LocalDB** — instances other than Rezure's own, from `SqlLocalDB info`.
//!
//! Read-only throughout: nothing is started, created or registered. A scan
//! only reports; the user decides, and the connection is still tested before
//! it can be saved.

use std::process::Command;

use serde::Serialize;

use super::{connections, mssql_localdb};
use crate::utils::command::HiddenWindow;

/// Every installed instance's name, each mapped to its internal id.
const INSTANCE_NAMES_KEY: &str = r"HKLM\SOFTWARE\Microsoft\Microsoft SQL Server\Instance Names\SQL";

/// The name of the unnamed instance. Reached as the bare host, where every
/// other instance is `host\NAME`.
const DEFAULT_INSTANCE: &str = "MSSQLSERVER";

/// `ERROR_SERVICE_DOES_NOT_EXIST`, which `sc.exe` returns as its exit code.
const SERVICE_MISSING: i32 = 1060;

/// `SERVICE_RUNNING` in `sc query`'s `STATE` line.
const STATE_RUNNING: u32 = 4;

/// Where an instance was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// An installed edition, running as a Windows service.
    Service,
    /// A LocalDB instance.
    Localdb,
}

/// One instance found on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedSqlServer {
    /// A suggested name for the connection.
    pub name: String,
    /// What goes in the form's Host box. The port is always left empty:
    /// locally the instance is reached by name, and its port may be dynamic.
    pub host: String,
    pub source: Source,
    /// `None` when the service state couldn't be read — not the same as
    /// stopped, and not worth guessing at.
    pub running: Option<bool>,
}

/// Scans for SQL Server instances, leaving out any already saved as a
/// connection so a second scan doesn't offer them again.
pub fn detect() -> Vec<DetectedSqlServer> {
    let mut found = installed_services();
    found.extend(localdb_instances());
    without_saved(found, &connections::saved_sqlserver_hosts())
}

fn without_saved(found: Vec<DetectedSqlServer>, saved: &[String]) -> Vec<DetectedSqlServer> {
    found
        .into_iter()
        .filter(|instance| {
            !saved
                .iter()
                .any(|host| host.eq_ignore_ascii_case(&instance.host))
        })
        .collect()
}

fn localdb_instances() -> Vec<DetectedSqlServer> {
    mssql_localdb::other_instances()
        .into_iter()
        .map(|instance| DetectedSqlServer {
            name: format!("LocalDB ({})", instance.name),
            host: format!(r"(localdb)\{}", instance.name),
            source: Source::Localdb,
            running: Some(instance.running),
        })
        .collect()
}

fn installed_services() -> Vec<DetectedSqlServer> {
    let mut found = Vec::new();
    for (instance, id) in registered_instances() {
        let running = match service_state(&service_name(&instance)) {
            // The registry outlives an uninstall that left it behind. With
            // no service there is nothing to connect to.
            ServiceState::Missing => continue,
            ServiceState::Running => Some(true),
            ServiceState::Stopped => Some(false),
            ServiceState::Unknown => None,
        };
        found.push(DetectedSqlServer {
            name: display_name(&instance, &id),
            host: host_for(&instance),
            source: Source::Service,
            running,
        });
    }
    found
}

/// `(instance name, internal id)` for every installed instance, from
/// `reg query`. Empty when SQL Server was never installed, which is when the
/// key doesn't exist.
fn registered_instances() -> Vec<(String, String)> {
    let Ok(output) = Command::new("reg")
        .args(["query", INSTANCE_NAMES_KEY])
        .hidden()
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_registered_instances(&String::from_utf8_lossy(&output.stdout))
}

/// Reads `reg query`'s `    NAME    REG_SZ    MSSQL16.NAME` lines. The key's
/// own path line and the blank ones carry no `REG_SZ` and are skipped.
fn parse_registered_instances(stdout: &str) -> Vec<(String, String)> {
    stdout
        .lines()
        .filter_map(|line| {
            let (name, id) = line.split_once("REG_SZ")?;
            let (name, id) = (name.trim(), id.trim());
            (!name.is_empty() && !id.is_empty()).then(|| (name.to_string(), id.to_string()))
        })
        .collect()
}

/// The Windows service behind an instance.
fn service_name(instance: &str) -> String {
    if instance.eq_ignore_ascii_case(DEFAULT_INSTANCE) {
        DEFAULT_INSTANCE.to_string()
    } else {
        format!("MSSQL${instance}")
    }
}

/// What the Host box needs to reach an instance from this machine.
fn host_for(instance: &str) -> String {
    if instance.eq_ignore_ascii_case(DEFAULT_INSTANCE) {
        "localhost".to_string()
    } else {
        format!(r"localhost\{instance}")
    }
}

/// `SQL Server 2022 (SQLEXPRESS)`, from the instance and its id
/// (`MSSQL16.SQLEXPRESS`). The release is left out when it isn't one this
/// knows, rather than guessed.
fn display_name(instance: &str, id: &str) -> String {
    let release = id
        .strip_prefix("MSSQL")
        .and_then(|rest| rest.split_once('.'))
        .and_then(|(major, _)| mssql_localdb::release_for_major(major));
    let base = match release {
        Some(release) => format!("SQL Server {release}"),
        None => "SQL Server".to_string(),
    };
    if instance.eq_ignore_ascii_case(DEFAULT_INSTANCE) {
        base
    } else {
        format!("{base} ({instance})")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceState {
    Running,
    Stopped,
    /// No such service.
    Missing,
    /// `sc` couldn't be run, or said something this can't read.
    Unknown,
}

fn service_state(service: &str) -> ServiceState {
    let Ok(output) = Command::new("sc.exe")
        .args(["query", service])
        .hidden()
        .output()
    else {
        return ServiceState::Unknown;
    };
    if output.status.code() == Some(SERVICE_MISSING) {
        return ServiceState::Missing;
    }
    match parse_service_state(&String::from_utf8_lossy(&output.stdout)) {
        Some(STATE_RUNNING) => ServiceState::Running,
        Some(_) => ServiceState::Stopped,
        None => ServiceState::Unknown,
    }
}

/// The numeric code from `sc query`'s `STATE : 4  RUNNING` line. The number
/// rather than the word, which a localized Windows may translate.
fn parse_service_state(stdout: &str) -> Option<u32> {
    stdout.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if !key.trim().eq_ignore_ascii_case("STATE") {
            return None;
        }
        value.split_whitespace().next()?.parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `reg query` prints: the key's path, then one line per value.
    const REG_OUTPUT: &str = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Microsoft SQL Server\\Instance Names\\SQL\r\n    MSSQLSERVER    REG_SZ    MSSQL16.MSSQLSERVER\r\n    SQLEXPRESS    REG_SZ    MSSQL15.SQLEXPRESS\r\n\r\n";

    #[test]
    fn registered_instances_are_read_from_reg_query() {
        assert_eq!(
            parse_registered_instances(REG_OUTPUT),
            vec![
                ("MSSQLSERVER".to_string(), "MSSQL16.MSSQLSERVER".to_string()),
                ("SQLEXPRESS".to_string(), "MSSQL15.SQLEXPRESS".to_string()),
            ]
        );
    }

    #[test]
    fn nothing_registered_reads_as_no_instances() {
        assert!(parse_registered_instances("").is_empty());
        assert!(parse_registered_instances("ERROR: The system was unable to find").is_empty());
    }

    #[test]
    fn the_default_instance_is_the_bare_host_and_a_named_one_is_not() {
        assert_eq!(host_for("MSSQLSERVER"), "localhost");
        assert_eq!(host_for("SQLEXPRESS"), r"localhost\SQLEXPRESS");
        assert_eq!(service_name("MSSQLSERVER"), "MSSQLSERVER");
        assert_eq!(service_name("SQLEXPRESS"), "MSSQL$SQLEXPRESS");
    }

    #[test]
    fn instance_names_carry_the_release_when_it_is_known() {
        assert_eq!(
            display_name("SQLEXPRESS", "MSSQL16.SQLEXPRESS"),
            "SQL Server 2022 (SQLEXPRESS)"
        );
        assert_eq!(
            display_name("MSSQLSERVER", "MSSQL15.MSSQLSERVER"),
            "SQL Server 2019"
        );
        assert_eq!(
            display_name("SQLEXPRESS", "MSSQL10_50.SQLEXPRESS"),
            "SQL Server 2008 R2 (SQLEXPRESS)"
        );
        // A release that doesn't exist yet is left unnamed, not mislabelled.
        assert_eq!(
            display_name("SQLEXPRESS", "MSSQL99.SQLEXPRESS"),
            "SQL Server (SQLEXPRESS)"
        );
    }

    #[test]
    fn a_service_state_is_read_by_its_number() {
        let running = "SERVICE_NAME: MSSQL$SQLEXPRESS \r\n        TYPE               : 10  WIN32_OWN_PROCESS  \r\n        STATE              : 4  RUNNING \r\n                                (STOPPABLE, PAUSABLE, ACCEPTS_SHUTDOWN)\r\n        WIN32_EXIT_CODE    : 0  (0x0)\r\n";
        assert_eq!(parse_service_state(running), Some(STATE_RUNNING));
        assert_eq!(
            parse_service_state(&running.replace(": 4  RUNNING", ": 1  STOPPED")),
            Some(1)
        );
        // Not the TYPE or WIN32_EXIT_CODE lines, which are numbers too.
        assert_eq!(
            parse_service_state("TYPE : 10  X\r\nWIN32_EXIT_CODE : 0"),
            None
        );
        assert_eq!(parse_service_state(""), None);
    }

    fn instance(host: &str) -> DetectedSqlServer {
        DetectedSqlServer {
            name: host.to_string(),
            host: host.to_string(),
            source: Source::Service,
            running: Some(true),
        }
    }

    #[test]
    fn an_instance_already_saved_is_not_offered_again() {
        let found = vec![
            instance(r"localhost\SQLEXPRESS"),
            instance("localhost"),
            instance(r"(localdb)\MSSQLLocalDB"),
        ];
        let saved = vec![r"LOCALHOST\sqlexpress".to_string()];
        let hosts: Vec<_> = without_saved(found, &saved)
            .into_iter()
            .map(|i| i.host)
            .collect();
        assert_eq!(hosts, vec!["localhost", r"(localdb)\MSSQLLocalDB"]);
    }

    /// Prints what this machine reports. Run with
    /// `cargo test --lib prints_this_machines_instances -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads this machine's SQL Server installs"]
    fn prints_this_machines_instances() {
        println!("registry: {:?}", registered_instances());
        println!("localdb:  {:?}", mssql_localdb::other_instances());
        println!("detected: {:#?}", detect());
    }
}
