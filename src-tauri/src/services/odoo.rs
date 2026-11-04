//! Odoo installed on this PC, connected to Rezure's PostgreSQL.
//!
//! Odoo's Windows installer bundles its own PostgreSQL, but the box for it
//! can't always be ticked — and then Odoo installs with no database server
//! at all. Rezure's PostgreSQL fits it: Odoo 20 ships PostgreSQL 16, Rezure's
//! catalog has 16 too, and Rezure's clusters are `UTF8 --no-locale`, which is
//! what Odoo creates its databases with. What's missing is the wiring, which
//! is all this module does:
//!
//! - a login of its own (`odoo`, `CREATEDB`) — Odoo refuses to run as
//!   `postgres` (`services::postgres_roles::ensure`);
//! - `odoo.conf` pointed at it: `db_host`, `db_port`, `db_user`,
//!   `db_password`, and `pg_path`, without which Odoo's database manager
//!   can't find `pg_dump` for backups;
//! - Odoo's service restarted to read it, and Odoo's own PostgreSQL service
//!   (if there is one) stopped and set to Manual so it doesn't take 5432 back.
//!
//! `odoo.conf` lives under Program Files, so the write and the service
//! changes go through one UAC prompt (`utils::elevation::run_logged`). The
//! original is kept beside it as `odoo.conf.rezure-backup`, and [`restore`]
//! puts everything back.
//!
//! Detection only reads: folders named `Odoo*` under Program Files holding
//! `server\odoo.conf`, and services named `odoo*`. Every action re-detects
//! rather than trusting a path from the frontend.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::windows_service_control;
use super::windows_services::{self, ServiceState};
use super::{binaries, postgres, postgres_roles, ServiceManager, ServiceStatus};
use crate::utils::command::HiddenWindow;
use crate::utils::elevation::{self, Logged};
use crate::utils::error::AppError;
use crate::utils::paths;
use crate::utils::powershell::quote_ps;

/// The login Odoo connects as.
pub const ODOO_ROLE: &str = "odoo";
/// Odoo's own default web port, when `odoo.conf` doesn't set one.
const DEFAULT_HTTP_PORT: u16 = 8069;
const BACKUP_SUFFIX: &str = ".rezure-backup";
const APP_NAME: &str = "Odoo";

/// How Odoo's master password — `admin_passwd`, what its database manager
/// asks for before creating, backing up or dropping a database — is set.
/// Never the value itself: Rezure only says where it is and what state it's
/// in, because "bad admin password" is the first wall right after connecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MasterPassword {
    /// No `admin_passwd`: Odoo falls back to `admin`.
    Missing,
    /// Readable in `odoo.conf` as written.
    PlainText,
    /// Odoo has hashed it (`$pbkdf2-sha512$…`); the original can't be read
    /// back, only replaced.
    Hashed,
}

fn master_password(options: &HashMap<String, String>) -> MasterPassword {
    match options.get("admin_passwd").map(|v| v.trim()) {
        None | Some("") | Some("False") => MasterPassword::Missing,
        Some(value) if value.starts_with("$pbkdf2") => MasterPassword::Hashed,
        Some(_) => MasterPassword::PlainText,
    }
}

/// An Odoo installation, and how its database settings stand.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OdooInstall {
    /// `C:\Program Files\Odoo 20.0.20261102` — also its id for the actions.
    pub dir: String,
    /// `20.0`, from the folder name; `None` if it doesn't say.
    pub version: Option<String>,
    pub conf_path: String,
    /// Odoo's own Windows service (`odoo-server-20.0`).
    pub service: Option<ServiceState>,
    /// The PostgreSQL service Odoo's installer added, if it did.
    pub bundled_postgres: Option<ServiceState>,
    /// The PostgreSQL version Odoo's installer bundled, if it's on disk —
    /// the closest match to pick on Rezure's Switch page.
    pub bundled_postgres_version: Option<String>,
    pub db_host: Option<String>,
    pub db_port: Option<String>,
    pub db_user: Option<String>,
    pub pg_path: Option<String>,
    pub http_port: u16,
    /// What connecting would set `pg_path` to — the active PostgreSQL's
    /// `bin` — and that version, so the dialog can show the change before
    /// it's made. `None` while no PostgreSQL is installed.
    pub rezure_pg_path: Option<String>,
    /// How `admin_passwd` is set — see [`MasterPassword`].
    pub master_password: MasterPassword,
    /// Odoo's own icon, read out of this install as a `data:` URL — the way
    /// the SQL clients' icons come out of their `.exe`, so no copy of Odoo's
    /// logo ships with Rezure. `None` if the install doesn't have it.
    pub icon: Option<String>,
    /// This account may start and stop Odoo's service without an admin
    /// prompt — see [`allow_control`].
    pub control_allowed: bool,
    /// Its settings point at Rezure's PostgreSQL, but the login they name
    /// isn't there — every page then fails with `role "odoo" does not exist`.
    /// Only true when that's known: a stopped PostgreSQL can't be asked.
    pub login_missing: bool,
    pub rezure_postgres_version: Option<String>,
    /// Its settings point at Rezure's PostgreSQL.
    pub uses_rezure: bool,
    /// The original `odoo.conf` is kept, so [`restore`] can put it back.
    pub can_restore: bool,
}

/// What [`connect`] does besides the settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectOptions {
    /// Stop Odoo's own PostgreSQL service and set it to Manual, so it doesn't
    /// hold 5432. Required while it's running.
    pub stop_bundled_postgres: bool,
}

/// Program Files, both of them, without repeats.
fn program_files() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    roots.dedup();
    roots
}

/// `20.0` from `Odoo 20.0.20261102`.
fn version_from_folder(name: &str) -> Option<String> {
    let token = name.split_whitespace().nth(1)?;
    let mut parts = token.split('.');
    let major = parts
        .next()
        .filter(|p| p.chars().all(|c| c.is_ascii_digit()))?;
    let minor = parts
        .next()
        .filter(|p| p.chars().all(|c| c.is_ascii_digit()))?;
    Some(format!("{major}.{minor}"))
}

/// The `[options]` keys of an `odoo.conf`, lowercased. Odoo's file has only
/// that section; anything in another is ignored.
fn parse_options(content: &str) -> HashMap<String, String> {
    let mut options = HashMap::new();
    let mut in_options = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_options = line.eq_ignore_ascii_case("[options]");
            continue;
        }
        if !in_options || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some((key, value)) = line.split_once(['=', ':']) {
            options.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }
    options
}

/// `content` with each of `values` set in `[options]`: an existing line
/// rewritten in place, a missing key added at the end of the section, every
/// other line — comments, addons paths, the admin password — untouched.
/// Keeps the file's own line endings.
fn set_options(content: &str, values: &[(&str, String)]) -> String {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut section: Option<(usize, usize)> = None; // [options] header, last line
    let mut in_options = false;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_options = trimmed.eq_ignore_ascii_case("[options]");
            if in_options {
                section = Some((index, index));
            }
        } else if in_options && !trimmed.is_empty() {
            if let Some((start, _)) = section {
                section = Some((start, index));
            }
        }
    }
    if section.is_none() {
        lines.insert(0, "[options]".to_string());
        section = Some((0, 0));
    }
    let (start, mut end) = section.unwrap_or((0, 0));

    for (key, value) in values {
        let existing = lines[start + 1..=end.max(start)]
            .iter()
            .position(|line| {
                let trimmed = line.trim();
                !trimmed.starts_with(['#', ';'])
                    && trimmed
                        .split_once(['=', ':'])
                        .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
            })
            .map(|offset| start + 1 + offset);
        let line = format!("{key} = {value}");
        match existing {
            Some(index) => lines[index] = line,
            None => {
                end += 1;
                lines.insert(end, line);
            }
        }
    }

    let mut out = lines.join(newline);
    out.push_str(newline);
    out
}

/// The version `postgres.exe --version` reports, `16.14` from
/// `postgres (PostgreSQL) 16.14`.
fn postgres_version(exe: &Path) -> Option<String> {
    if !exe.is_file() {
        return None;
    }
    let output = Command::new(exe).arg("--version").hidden().output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .last()
        .map(str::to_string)
}

/// Whether `pg_path` is one of Rezure's PostgreSQL builds.
fn is_rezure_pg_path(pg_path: &str) -> bool {
    let pg_path = pg_path.to_lowercase();
    [binaries::install_root(), binaries::user_bin_root()]
        .into_iter()
        .flatten()
        .map(|root| {
            root.join(postgres::FAMILY)
                .display()
                .to_string()
                .to_lowercase()
        })
        .any(|root| pg_path.starts_with(&root))
}

fn backup_path(conf: &Path) -> PathBuf {
    let mut name = conf.as_os_str().to_owned();
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// Every Odoo installed under Program Files.
pub fn detect() -> Vec<OdooInstall> {
    let mut dirs: Vec<PathBuf> = program_files()
        .iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .starts_with("odoo")
        })
        .map(|entry| entry.path())
        .filter(|dir| dir.join("server").join("odoo.conf").is_file())
        .collect();
    dirs.sort();
    dirs.dedup();
    if dirs.is_empty() {
        return Vec::new();
    }

    // One SCM pass for every install.
    let services = windows_services::find(|name, display| {
        let text = format!("{name} {display}").to_lowercase();
        text.contains("odoo")
    });
    dirs.iter().map(|dir| describe(dir, &services)).collect()
}

fn describe(dir: &Path, services: &[ServiceState]) -> OdooInstall {
    let conf = dir.join("server").join("odoo.conf");
    let options = fs::read_to_string(&conf)
        .map(|content| parse_options(&content))
        .unwrap_or_default();
    let version = dir
        .file_name()
        .and_then(|name| version_from_folder(&name.to_string_lossy()));
    let is_postgres = |s: &&ServiceState| {
        format!("{} {}", s.name, s.display_name)
            .to_lowercase()
            .contains("postgres")
    };
    // With several Odoo versions installed, the service named for this one.
    let pick = |candidates: Vec<&ServiceState>| {
        candidates
            .iter()
            .find(|s| version.as_deref().is_some_and(|v| s.name.contains(v)))
            .or(candidates.first())
            .map(|s| (*s).clone())
    };
    let service = pick(services.iter().filter(|s| !is_postgres(s)).collect());
    let bundled_postgres = pick(services.iter().filter(is_postgres).collect());
    let control_allowed = service
        .as_ref()
        .is_some_and(|s| windows_service_control::can_control(&s.name));

    let get = |key: &str| options.get(key).filter(|v| !v.is_empty()).cloned();
    let db_host = get("db_host");
    let db_port = get("db_port");
    let pg_path = get("pg_path");
    let uses_rezure = db_host
        .as_deref()
        .is_some_and(|h| h == postgres::HOST || h.eq_ignore_ascii_case("localhost"))
        && db_port.as_deref() == Some(&postgres::PORT.to_string())
        && pg_path.as_deref().is_some_and(is_rezure_pg_path);
    let login_missing = uses_rezure
        && get("db_user").is_some_and(|user| {
            postgres_roles::list()
                .ok()
                .is_some_and(|roles| roles.iter().all(|role| role.name != user))
        });

    OdooInstall {
        dir: dir.display().to_string(),
        version,
        conf_path: conf.display().to_string(),
        service,
        bundled_postgres,
        bundled_postgres_version: postgres_version(
            &dir.join("PostgreSQL")
                .join("bin")
                .join(postgres::SERVER_EXE),
        ),
        db_user: get("db_user"),
        http_port: get("http_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_HTTP_PORT),
        can_restore: backup_path(&conf).is_file(),
        rezure_pg_path: rezure_pg_bin().ok().map(|bin| bin.display().to_string()),
        master_password: master_password(&options),
        icon: icon(dir),
        control_allowed,
        login_missing,
        rezure_postgres_version: Some(postgres::active_id()).filter(|v| !v.is_empty()),
        db_host,
        db_port,
        pg_path,
        uses_rezure,
    }
}

/// The active PostgreSQL's `bin` folder — Odoo's `pg_path`.
fn rezure_pg_bin() -> Result<PathBuf, AppError> {
    postgres::active_tool(postgres::CLIENT_EXE)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Io("PostgreSQL's bin folder couldn't be found".to_string()))
}

/// Where Odoo keeps its app icon, relative to the install folder.
const ICON_RELATIVE: [&str; 7] = [
    "server",
    "odoo",
    "addons",
    "web",
    "static",
    "img",
    "odoo-icon-192x192.png",
];

/// Odoo's own icon from the install at `dir`, as a PNG `data:` URL.
fn icon(dir: &Path) -> Option<String> {
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;

    let path = ICON_RELATIVE
        .iter()
        .fold(dir.to_path_buf(), |p, part| p.join(part));
    let bytes = fs::read(path).ok()?;
    // A PNG, and a small one: anything else isn't the file this expects.
    if bytes.len() > 256 * 1024 || !bytes.starts_with(b"\x89PNG") {
        return None;
    }
    Some(format!("data:image/png;base64,{}", BASE64.encode(bytes)))
}

/// The install at `dir`, freshly detected.
fn find(dir: &str) -> Result<OdooInstall, AppError> {
    detect()
        .into_iter()
        .find(|install| install.dir.eq_ignore_ascii_case(dir))
        .ok_or_else(|| AppError::ExternalAppNotFound(APP_NAME.to_string()))
}

fn staging_dir() -> Result<PathBuf, AppError> {
    Ok(paths::data()?.join("odoo"))
}

/// Remembers which bundled PostgreSQL service [`connect`] turned off, per
/// `odoo.conf`, so [`restore`] turns that one — and only that one — back on.
fn state_path() -> Result<PathBuf, AppError> {
    Ok(staging_dir()?.join("state.json"))
}

fn read_state() -> HashMap<String, String> {
    state_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_state(state: &HashMap<String, String>) {
    let Ok(path) = state_path() else { return };
    if let Ok(text) = serde_json::to_string_pretty(state) {
        if let Err(err) = fs::write(&path, text) {
            log::warn!("could not record Odoo's state: {err}");
        }
    }
}

/// A password for the `odoo` login — never shown or kept by Rezure; it
/// lives in `odoo.conf`, where Odoo reads it.
fn generate_password() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn run_elevated(stem: &str, statements: &str) -> Result<(), AppError> {
    let failed = |reason: String| AppError::ExternalAppSetupFailed {
        app: APP_NAME.to_string(),
        reason,
    };
    match elevation::run_logged(&staging_dir()?, stem, statements)
        .map_err(|e| failed(e.to_string()))?
    {
        Logged::Done => Ok(()),
        Logged::Declined => Err(AppError::ExternalAppSetupCancelled(APP_NAME.to_string())),
        Logged::Failed(reason) => Err(failed(reason)),
    }
}

/// Points Odoo at Rezure's PostgreSQL — see the module docs for what that
/// takes. Safe to run again: the role is brought in line rather than
/// re-created, and the first backup of `odoo.conf` is the one kept.
pub fn connect(
    manager: &ServiceManager,
    dir: &str,
    options: &ConnectOptions,
) -> Result<Vec<OdooInstall>, AppError> {
    let install = find(dir)?;
    let failed = |reason: String| AppError::ExternalAppSetupFailed {
        app: APP_NAME.to_string(),
        reason,
    };
    let pg_bin = rezure_pg_bin()?;
    let bundled = install.bundled_postgres.as_ref();
    if bundled.is_some_and(|s| s.running) && !options.stop_bundled_postgres {
        return Err(failed(
            "Odoo's own PostgreSQL is running and holds port 5432 - let Rezure stop it, or stop it yourself first".to_string(),
        ));
    }

    let conf = PathBuf::from(&install.conf_path);
    let content = fs::read_to_string(&conf)
        .map_err(|e| failed(format!("couldn't read {}: {e}", conf.display())))?;
    let password = generate_password();
    let next = set_options(
        &content,
        &[
            ("db_host", postgres::HOST.to_string()),
            ("db_port", postgres::PORT.to_string()),
            ("db_user", ODOO_ROLE.to_string()),
            ("db_password", password.clone()),
            ("pg_path", pg_bin.display().to_string()),
        ],
    );

    let staging = staging_dir()?;
    fs::create_dir_all(&staging)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", staging.display())))?;
    let staged = staging.join("odoo.conf");
    fs::write(&staged, &next)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", staged.display())))?;

    let mut statements = String::new();
    let stopping = bundled.filter(|_| options.stop_bundled_postgres);
    if let Some(service) = stopping {
        statements.push_str(&windows_services::stop_statements(&service.name, true));
    }
    let backup = backup_path(&conf);
    if !backup.is_file() {
        statements.push_str(&format!(
            "Copy-Item -LiteralPath {} -Destination {} -ErrorAction Stop\n",
            quote_ps(&conf.display().to_string()),
            quote_ps(&backup.display().to_string())
        ));
    }
    statements.push_str(&format!(
        "Copy-Item -LiteralPath {} -Destination {} -Force -ErrorAction Stop\n",
        quote_ps(&staged.display().to_string()),
        quote_ps(&conf.display().to_string())
    ));
    // Only one that's already up: starting an Odoo the user had stopped is
    // their call, and it reads the new settings whenever it does start.
    if let Some(service) = install.service.as_ref().filter(|s| s.running) {
        statements.push_str(&format!(
            "Restart-Service -Name {} -ErrorAction Stop\n",
            quote_ps(&service.name)
        ));
    }

    log::info!("connecting {} to Rezure's PostgreSQL", install.dir);
    let result = run_elevated("connect-odoo", &statements);
    // The staged copy holds the password; it has no reason to outlive this.
    let _ = fs::remove_file(&staged);
    result?;

    if fs::read_to_string(&conf).ok().as_deref() != Some(next.as_str()) {
        return Err(failed(format!(
            "{} wasn't updated - was the admin prompt approved?",
            conf.display()
        )));
    }
    if let Some(service) = stopping {
        let mut state = read_state();
        state.insert(install.conf_path.clone(), service.name.clone());
        write_state(&state);
    }

    // Now that 5432 is free of Odoo's own server.
    let server = manager.find(postgres::SERVICE_ID)?;
    if server.info().status == ServiceStatus::Stopped {
        server.start()?;
    }
    postgres_roles::ensure(ODOO_ROLE, Some(&password), true)?;
    Ok(detect())
}

/// Puts back the `odoo.conf` [`connect`] replaced, and Odoo's own PostgreSQL
/// service if it turned that off — stopping Rezure's PostgreSQL first, since
/// both want port 5432.
pub fn restore(manager: &ServiceManager, dir: &str) -> Result<Vec<OdooInstall>, AppError> {
    let install = find(dir)?;
    let conf = PathBuf::from(&install.conf_path);
    let backup = backup_path(&conf);
    if !backup.is_file() {
        return Err(AppError::ExternalAppSetupFailed {
            app: APP_NAME.to_string(),
            reason: "there's no saved original odoo.conf to put back".to_string(),
        });
    }

    let mut state = read_state();
    let reenable = state.get(&install.conf_path).cloned();
    if reenable.is_some() {
        let server = manager.find(postgres::SERVICE_ID)?;
        if server.info().status != ServiceStatus::Stopped {
            server.stop()?;
        }
    }

    let mut statements = format!(
        "Copy-Item -LiteralPath {} -Destination {} -Force -ErrorAction Stop\nRemove-Item -LiteralPath {} -ErrorAction Stop\n",
        quote_ps(&backup.display().to_string()),
        quote_ps(&conf.display().to_string()),
        quote_ps(&backup.display().to_string())
    );
    if let Some(name) = &reenable {
        statements.push_str(&format!(
            "Set-Service -Name {name} -StartupType Automatic -ErrorAction Stop\nStart-Service -Name {name} -ErrorAction Stop\n",
            name = quote_ps(name)
        ));
    }
    if let Some(service) = &install.service {
        if service.running {
            statements.push_str(&format!(
                "Restart-Service -Name {} -ErrorAction Stop\n",
                quote_ps(&service.name)
            ));
        }
        // Everything Rezure changed goes back, the start/stop permission
        // included.
        if let Some(revoke) = windows_service_control::disallow_statements(&service.name) {
            statements.push_str(&revoke);
        }
    }

    log::info!("restoring {}'s own database settings", install.dir);
    run_elevated("restore-odoo", &statements)?;
    state.remove(&install.conf_path);
    write_state(&state);
    Ok(detect())
}

/// Odoo's Windows service, or the error that says there isn't one.
fn service_of(install: &OdooInstall) -> Result<ServiceState, AppError> {
    install
        .service
        .clone()
        .ok_or_else(|| AppError::ExternalAppSetupFailed {
            app: APP_NAME.to_string(),
            reason: "no Windows service was found for it - start and stop Odoo the way it was installed to run"
                .to_string(),
        })
}

fn outcome(result: std::io::Result<windows_service_control::Outcome>) -> Result<(), AppError> {
    use windows_service_control::Outcome;
    let failed = |reason: String| AppError::ExternalAppSetupFailed {
        app: APP_NAME.to_string(),
        reason,
    };
    match result.map_err(|e| failed(e.to_string()))? {
        Outcome::Done => Ok(()),
        Outcome::Declined => Err(AppError::ExternalAppSetupCancelled(APP_NAME.to_string())),
        Outcome::Failed(reason) => Err(failed(reason)),
    }
}

/// Starts, stops or restarts Odoo's service — restart being what makes it
/// read odoo.conf again after an edit. Without a prompt once [`allow_control`]
/// has run, through UAC otherwise.
///
/// Starting brings Rezure's PostgreSQL up first when Odoo is connected to
/// it: an Odoo with no database only answers every page with an error.
pub fn control(
    manager: &ServiceManager,
    dir: &str,
    action: windows_service_control::Action,
) -> Result<Vec<OdooInstall>, AppError> {
    use windows_service_control::Action;
    let install = find(dir)?;
    let service = service_of(&install)?;
    if install.uses_rezure && matches!(action, Action::Start | Action::Restart) {
        start_postgres(manager)?;
        // Cheap and idempotent, and the one thing that otherwise leaves a
        // started Odoo answering every page with "role does not exist".
        if let Err(err) = ensure_login(&install) {
            log::warn!("couldn't make sure Odoo's login exists: {err}");
        }
    }
    outcome(windows_service_control::control(
        &service.name,
        action,
        "control-odoo",
    ))?;
    Ok(detect())
}

fn start_postgres(manager: &ServiceManager) -> Result<(), AppError> {
    let server = manager.find(postgres::SERVICE_ID)?;
    if server.info().status == ServiceStatus::Stopped {
        server.start()?;
    }
    Ok(())
}

/// Makes sure the login `odoo.conf` names exists on Rezure's PostgreSQL,
/// with the password it names — creating it, or bringing it in line.
fn ensure_login(install: &OdooInstall) -> Result<(), AppError> {
    let options = fs::read_to_string(&install.conf_path)
        .map(|content| parse_options(&content))
        .map_err(|e| AppError::Io(format!("couldn't read {}: {e}", install.conf_path)))?;
    let user = options
        .get("db_user")
        .map(String::as_str)
        .unwrap_or(ODOO_ROLE);
    let password = options
        .get("db_password")
        .map(String::as_str)
        .filter(|p| !p.is_empty() && *p != "False");
    postgres_roles::ensure(user, password, true)
}

/// Recreates Odoo's login on Rezure's PostgreSQL when it has gone missing
/// (see [`OdooInstall::login_missing`]) — from `odoo.conf` as it stands, so
/// nothing under Program Files changes and no admin prompt is needed.
pub fn repair_login(manager: &ServiceManager, dir: &str) -> Result<Vec<OdooInstall>, AppError> {
    let install = find(dir)?;
    start_postgres(manager)?;
    ensure_login(&install)?;
    log::info!("recreated Odoo's login on Rezure's PostgreSQL");
    Ok(detect())
}

/// Lets this Windows account start and stop Odoo's service without an
/// admin prompt from now on — one UAC prompt, this service only. See
/// `windows_service_control`.
pub fn allow_control(dir: &str) -> Result<Vec<OdooInstall>, AppError> {
    let install = find(dir)?;
    let service = service_of(&install)?;
    outcome(windows_service_control::allow(&service.name, "allow-odoo"))?;
    if !windows_service_control::can_control(&service.name) {
        return Err(AppError::ExternalAppSetupFailed {
            app: APP_NAME.to_string(),
            reason: "the permission wasn't saved - was the admin prompt approved?".to_string(),
        });
    }
    Ok(detect())
}

/// Opens Odoo's web UI on the port its settings name.
pub fn open(dir: &str) -> Result<(), AppError> {
    let install = find(dir)?;
    let url = format!("http://localhost:{}", install.http_port);
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| AppError::OpenFailed {
        target: url,
        reason: e.to_string(),
    })
}

/// Opens the install's `odoo.conf` in Notepad run as administrator — it's
/// under Program Files, so a Notepad without admin rights could show it but
/// not save it. For what Rezure leaves to the user: the master password.
pub fn open_conf(dir: &str) -> Result<(), AppError> {
    let install = find(dir)?;
    let target = install.conf_path.clone();
    match elevation::open_elevated("notepad.exe", Path::new(&install.conf_path)) {
        Ok(elevation::Elevated::Ran) => Ok(()),
        Ok(elevation::Elevated::Declined) => Err(AppError::OpenFailed {
            target,
            reason: "the admin prompt was declined - Notepad needs it to save the file".to_string(),
        }),
        Err(e) => Err(AppError::OpenFailed {
            target,
            reason: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_comes_from_the_folder_name() {
        assert_eq!(
            version_from_folder("Odoo 20.0.20261102").as_deref(),
            Some("20.0")
        );
        assert_eq!(version_from_folder("Odoo 17.0").as_deref(), Some("17.0"));
        assert_eq!(version_from_folder("Odoo"), None);
        assert_eq!(version_from_folder("Odoo latest"), None);
    }

    const CONF: &str = "[options]\r\naddons_path = C:\\Program Files\\Odoo\\server\\odoo\\addons\r\nadmin_passwd = keep-me\r\ndb_host = localhost\r\n; db_user = commented\r\ndb_user = openpg\r\npg_path = C:\\Program Files\\Odoo\\PostgreSQL\\bin\r\n";

    #[test]
    fn options_are_read_from_the_options_section() {
        let options = parse_options(CONF);
        assert_eq!(options["db_user"], "openpg");
        assert_eq!(options["db_host"], "localhost");
        assert!(!options.contains_key("db_password"));
    }

    /// Existing keys rewritten in place, missing ones added at the end of
    /// the section, everything else — the admin password, comments, line
    /// endings — exactly as it was.
    #[test]
    fn setting_options_touches_only_those_keys() {
        let next = set_options(
            CONF,
            &[
                ("db_host", "127.0.0.1".to_string()),
                ("db_user", "odoo".to_string()),
                ("db_password", "secret".to_string()),
            ],
        );
        assert_eq!(
            next,
            "[options]\r\naddons_path = C:\\Program Files\\Odoo\\server\\odoo\\addons\r\nadmin_passwd = keep-me\r\ndb_host = 127.0.0.1\r\n; db_user = commented\r\ndb_user = odoo\r\npg_path = C:\\Program Files\\Odoo\\PostgreSQL\\bin\r\ndb_password = secret\r\n"
        );
        let options = parse_options(&next);
        assert_eq!(options["db_user"], "odoo");
        assert_eq!(options["db_password"], "secret");
    }

    #[test]
    fn a_file_without_an_options_section_gets_one() {
        let next = set_options("", &[("db_port", "5432".to_string())]);
        assert_eq!(parse_options(&next)["db_port"], "5432");
        assert!(next.starts_with("[options]\n"));
    }

    #[test]
    fn the_master_password_state_is_read_without_its_value() {
        let options = |line: &str| parse_options(&format!("[options]\n{line}\n"));
        assert_eq!(master_password(&options("")), MasterPassword::Missing);
        assert_eq!(
            master_password(&options("admin_passwd = False")),
            MasterPassword::Missing
        );
        assert_eq!(
            master_password(&options("admin_passwd = s3cret")),
            MasterPassword::PlainText
        );
        assert_eq!(
            master_password(&options("admin_passwd = $pbkdf2-sha512$600000$abc$def")),
            MasterPassword::Hashed
        );
    }

    #[test]
    fn the_backup_sits_beside_the_original() {
        assert_eq!(
            backup_path(Path::new(r"C:\Odoo\server\odoo.conf")),
            PathBuf::from(r"C:\Odoo\server\odoo.conf.rezure-backup")
        );
    }

    /// What's installed on this machine. Run with
    /// `cargo test --lib services::odoo::tests::print_detected -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_detected() {
        for install in detect() {
            println!("{install:#?}");
        }
    }
}
