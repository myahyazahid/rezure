//! The Microsoft ODBC Driver for SQL Server — the one piece of SQL Server
//! support that has to be installed into Windows itself.
//!
//! Two things in Rezure talk to SQL Server, and both go through it:
//!
//! * PHP's `sqlsrv` / `pdo_sqlsrv` extensions (`services::php_ext`). The DLLs
//!   load fine without the driver; the failure only arrives on the first
//!   connect, as `This extension requires the Microsoft ODBC Driver for SQL
//!   Server` — which is why `services::doctor` checks for it separately.
//! * The Databases page itself (`services::mssql`), which speaks to SQL
//!   Server through ODBC rather than through a client binary.
//!
//! # Why only 17 and 18
//!
//! Every Windows install ships a driver simply called `SQL Server`
//! (`sqlsrv32.dll`), frozen at SQL Server 2000's feature set. Microsoft's PHP
//! drivers refuse it, and it can't speak TLS 1.2, so finding it means nothing
//! here. Versions 17 and 18 are the two current lines; a machine whose IT
//! department installed 17 is served by it rather than being asked to add 18
//! beside it — many office laptops give their user no admin rights at all,
//! and 17 is enough for everything Rezure does.
//!
//! Installing the driver is `services::msi`'s job; this module only reads
//! what Windows already has registered.

use crate::utils::error::AppError;

/// A usable driver, as registered with the ODBC driver manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OdbcDriver {
    /// Exactly as registered — `ODBC Driver 18 for SQL Server` — and exactly
    /// what goes into a connection string's `Driver={…}`.
    pub name: String,
    pub major: u32,
}

impl OdbcDriver {
    /// 18 changed the default to `Encrypt=yes`, so a server with a
    /// self-signed certificate that 17 connected to silently now refuses —
    /// the case `services::mssql` and `services::doctor` explain.
    pub fn encrypts_by_default(&self) -> bool {
        self.major >= 18
    }
}

/// The oldest line accepted — see the module docs.
const MIN_MAJOR: u32 = 17;

/// `ODBC Driver 18 for SQL Server` → 18. Anything else — including the
/// built-in `SQL Server` driver and the long-retired `SQL Server Native
/// Client 11.0` — reads as `None`.
fn major_of(name: &str) -> Option<u32> {
    let version = name
        .strip_prefix("ODBC Driver ")?
        .strip_suffix(" for SQL Server")?;
    version.trim().parse().ok()
}

/// The newest acceptable driver among `names`.
fn best_of<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<OdbcDriver> {
    names
        .into_iter()
        .filter_map(|name| {
            major_of(name)
                .filter(|major| *major >= MIN_MAJOR)
                .map(|major| OdbcDriver {
                    name: name.to_string(),
                    major,
                })
        })
        .max_by_key(|driver| driver.major)
}

/// The driver to use, or `None` when no acceptable one is registered.
///
/// Asks the ODBC driver manager directly (`SQLDrivers`), in-process — the
/// same list `odbcad32.exe` shows — so a driver installed a moment ago, by
/// Rezure or anyone else, is seen on the next call without a restart.
pub fn detect() -> Option<OdbcDriver> {
    let environment = match odbc_api::environment() {
        Ok(environment) => environment,
        Err(err) => {
            log::warn!("could not open the ODBC environment: {err}");
            return None;
        }
    };
    let drivers = match environment.drivers() {
        Ok(drivers) => drivers,
        Err(err) => {
            log::warn!("could not list ODBC drivers: {err}");
            return None;
        }
    };
    best_of(drivers.iter().map(|driver| driver.description.as_str()))
}

/// [`detect`], as an error the caller can return when there is no driver.
pub fn require() -> Result<OdbcDriver, AppError> {
    detect().ok_or(AppError::OdbcDriverMissing)
}

pub fn is_installed() -> bool {
    detect().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_current_driver_lines_are_recognised() {
        assert_eq!(major_of("ODBC Driver 18 for SQL Server"), Some(18));
        assert_eq!(major_of("ODBC Driver 17 for SQL Server"), Some(17));
        // Shipped with every Windows, and useless for sqlsrv.
        assert_eq!(major_of("SQL Server"), None);
        assert_eq!(major_of("SQL Server Native Client 11.0"), None);
        assert_eq!(major_of("MySQL ODBC 8.0 Unicode Driver"), None);
    }

    #[test]
    fn the_newest_driver_wins_and_old_ones_are_refused() {
        let best = best_of([
            "SQL Server",
            "ODBC Driver 17 for SQL Server",
            "ODBC Driver 18 for SQL Server",
        ])
        .unwrap();
        assert_eq!(best.name, "ODBC Driver 18 for SQL Server");
        assert!(best.encrypts_by_default());

        assert!(best_of(["SQL Server", "ODBC Driver 13 for SQL Server"]).is_none());
    }

    #[test]
    fn a_machine_with_only_17_is_served_by_it() {
        let best = best_of(["ODBC Driver 17 for SQL Server"]).unwrap();
        assert_eq!(best.major, 17);
        assert!(!best.encrypts_by_default());
    }

    /// Prints what this machine has registered. Run with:
    /// `cargo test --lib services::odbc::tests::print_detected -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_detected() {
        let environment = odbc_api::environment().unwrap();
        for driver in environment.drivers().unwrap() {
            println!("registered: {}", driver.description);
        }
        println!("chosen: {:?}", detect());
    }
}
