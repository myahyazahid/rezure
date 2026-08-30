//! The live profile state behind the database switcher: which datadir the
//! one running server is pointed at, and whether a given switch is safe.
//!
//! Mirrors `services::php`'s shape — a process-wide `OnceLock<Mutex<_>>`
//! rather than state threaded through the `Service` trait — because
//! `ProcessService` has to resolve the datadir, port and binary at *spawn*
//! time for a switch to take effect, exactly the way it re-resolves PHP's
//! `php-cgi.exe`. Unlike PHP's, this state is persisted (`config::profiles`)
//! on every mutation.
//!
//! Everything that could destroy data lives here: [`check_can_switch_to`] is
//! the single gate a switch passes through, and it refuses far more often
//! than it allows.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use serde::Serialize;

use super::binaries::{self, InstalledRuntime};
use super::db_engine::{self, Engine, SERVER_EXE};
use crate::config::profiles::{self, Profile, ProfileSource, ProfileStore};
use crate::utils::error::AppError;

/// Where Rezure's own datadir lives — the seed profile points here, and it
/// stays the fallback a failed switch rolls back to.
///
/// Matches `services::process::runtime_dir("mariadb")/data`, the path the
/// bundled MariaDB has always used, so an install that predates profiles
/// keeps its existing databases instead of silently starting empty.
pub fn rezure_datadir() -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base
        .join("Rezure")
        .join("data")
        .join("mariadb")
        .join("data"))
}

fn store_cell() -> &'static Mutex<ProfileStore> {
    static STORE: OnceLock<Mutex<ProfileStore>> = OnceLock::new();
    STORE.get_or_init(|| {
        let mut store = profiles::load();

        // Seeded lazily rather than at startup so the very first read is
        // always against a store that has somewhere to run, however the
        // file on disk got there.
        let version = binaries::find("mariadb")
            .map(|pkg| pkg.version.to_string())
            .unwrap_or_default();
        if let Ok(datadir) = rezure_datadir() {
            if profiles::ensure_default(&mut store, &datadir, Engine::MariaDb, version, 3306) {
                if let Err(err) = profiles::save(&store) {
                    log::warn!("could not persist the seeded profile store: {err}");
                }
            }
        }
        Mutex::new(store)
    })
}

fn store() -> MutexGuard<'static, ProfileStore> {
    store_cell().lock().unwrap()
}

/// Persists the store while it's still locked, so what's on disk can't
/// disagree with what's in memory.
fn persist(store: &ProfileStore) {
    if let Err(err) = profiles::save(store) {
        log::warn!("could not persist database profiles: {err}");
    }
}

/// The profile the server runs against right now.
pub fn active() -> Option<Profile> {
    store().active().cloned()
}

pub fn list() -> Vec<ProfileStatus> {
    let store = store();
    let active_id = store.active_profile_id.clone();
    store
        .profiles
        .iter()
        .map(|profile| ProfileStatus {
            active: Some(&profile.id) == active_id.as_ref(),
            // Resolved per row so the switcher can grey out (and explain)
            // a profile whose binary isn't installed, instead of only
            // finding out when the switch fails.
            binary_available: resolve_server_exe(profile).is_ok(),
            profile: profile.clone(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStatus {
    #[serde(flatten)]
    pub profile: Profile,
    pub active: bool,
    /// Whether a compatible engine binary is installed for this profile.
    pub binary_available: bool,
}

/// How a profile's declared version lines up with an installed binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    /// Same `major.minor`; only the patch differs, if anything. Safe.
    Compatible,
    /// Same major, different minor (8.0 vs 8.4, MariaDB 10.4 vs 10.11).
    /// Starting anyway performs a **one-way** in-place upgrade, so this is
    /// refused rather than "logged and proceeded".
    NeedsUpgrade,
    /// Different major. Never openable.
    Incompatible,
}

/// Splits a version into `(major, minor)`, ignoring the patch.
fn major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version
        .split(['.', '-'])
        .filter_map(|p| p.parse::<u64>().ok());
    Some((parts.next()?, parts.next().unwrap_or(0)))
}

/// Compares a datadir's version against a candidate binary's.
///
/// Deliberately stricter than the original spec, which called "same major,
/// different minor" generally safe. That's true for a *patch* bump
/// (8.0.30 → 8.0.36) and it's what the spec's example actually shows — but
/// a minor bump like MySQL 8.0 → 8.4 or MariaDB 10.4 → 10.11 rewrites the
/// datadir on first start and cannot be undone. Since the whole premise
/// here is opening data another tool owns, an irreversible rewrite is not
/// something to do on a "log a note".
pub fn compatibility(datadir_version: &str, binary_version: &str) -> Compatibility {
    match (major_minor(datadir_version), major_minor(binary_version)) {
        (Some((dm, dn)), Some((bm, bn))) => {
            if dm != bm {
                Compatibility::Incompatible
            } else if dn != bn {
                Compatibility::NeedsUpgrade
            } else {
                Compatibility::Compatible
            }
        }
        // An unknown version can't be reasoned about; the caller treats
        // this as "ask the user", never as "go ahead".
        _ => Compatibility::Incompatible,
    }
}

/// Every installed binary for an engine, newest first.
pub fn installed_binaries(engine: Engine) -> Vec<InstalledRuntime> {
    binaries::discover(engine.family(), SERVER_EXE)
}

/// Picks the binary a profile should run on: the newest whose
/// `major.minor` matches the datadir's.
///
/// A profile with no recorded version falls back to the newest installed
/// build of its engine — the honest choice when there's nothing to match
/// against, and the reason the add-profile flow tries hard to detect one.
pub fn resolve_server_exe(profile: &Profile) -> Result<PathBuf, AppError> {
    let installed = installed_binaries(profile.engine);
    if installed.is_empty() {
        return Err(AppError::EngineBinaryMissing {
            engine: profile.engine.label().to_string(),
            version: if profile.version.is_empty() {
                "any version".to_string()
            } else {
                profile.version.clone()
            },
        });
    }

    if profile.version.is_empty() {
        return Ok(installed[0].exe.clone());
    }

    installed
        .iter()
        .find(|runtime| {
            compatibility(&profile.version, &runtime.version) == Compatibility::Compatible
        })
        .map(|runtime| runtime.exe.clone())
        .ok_or_else(|| AppError::EngineBinaryMissing {
            engine: profile.engine.label().to_string(),
            version: profile.version.clone(),
        })
}

/// Whether some *other* server already owns this datadir.
///
/// A running server writes a `.pid` file into its datadir and removes it on
/// a clean stop, so a pid file naming a live process means the folder is in
/// use — by Laragon, XAMPP, or a copy of Rezure that was force-closed.
/// Starting a second server against it is the one action that reliably
/// corrupts InnoDB, so this refuses on any live pid.
///
/// A stale pid file (process long gone) is not treated as in-use: that's
/// the normal leftover of a crash, and refusing forever on it would strand
/// the user with no way back into their own data.
fn datadir_holder(data_dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(data_dir).ok()?;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("pid") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(pid) = contents.trim().parse::<u32>() else {
            continue;
        };
        if is_server_process_alive(pid) {
            return Some(pid.to_string());
        }
    }
    None
}

/// Whether `pid` is a live database server — checked by name, so a recycled
/// pid belonging to something unrelated doesn't read as "the datadir is in
/// use" and block a legitimate switch forever.
fn is_server_process_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid)
        .and_then(|p| p.name().to_str().map(|n| n.to_lowercase()))
        .is_some_and(|name| name.contains("mysqld") || name.contains("mariadbd"))
}

/// The single gate every switch passes through.
///
/// Ordered cheapest-and-most-fatal first: a datadir written by the other
/// engine is unopenable no matter what else is true, so it's checked before
/// anything that touches the filesystem or process table.
pub fn check_can_switch_to(profile: &Profile) -> Result<(), AppError> {
    let data_dir = Path::new(&profile.datadir_path);

    // A datadir Rezure is about to create is exempt — there's nothing to
    // read an engine off, and bootstrap will write it correctly.
    if !db_engine::needs_bootstrap(data_dir) {
        if let Some(found) = Engine::detect_from_datadir(data_dir) {
            if found != profile.engine {
                return Err(AppError::EngineMismatch {
                    found: found.label().to_string(),
                    expected: profile.engine.label().to_string(),
                });
            }
        }
    }

    resolve_server_exe(profile)?;

    if let Some(pid) = datadir_holder(data_dir) {
        log::warn!(
            "{} is held by a live server process (pid {pid})",
            profile.datadir_path
        );
        return Err(AppError::DatadirInUse {
            name: match profile.source {
                ProfileSource::Laragon => "Laragon".to_string(),
                ProfileSource::Xampp => "XAMPP".to_string(),
                _ => "Another".to_string(),
            },
        });
    }

    Ok(())
}

/// Points the switcher at `id` after [`check_can_switch_to`] has passed.
/// Restarting the server is the caller's job — see `commands::db_profiles`.
pub fn set_active(id: &str) -> Result<Profile, AppError> {
    let mut store = store();
    let profile = store.find(id)?.clone();

    store.active_profile_id = Some(profile.id.clone());
    if let Some(entry) = store.profiles.iter_mut().find(|p| p.id == profile.id) {
        entry.last_used_at = Some(profiles::now_secs());
    }
    persist(&store);

    Ok(profile)
}

/// Registers an existing datadir as a profile. Reads the engine off the
/// folder rather than trusting the caller — see [`Engine::detect_from_datadir`].
pub fn add(
    name: String,
    datadir_path: String,
    engine: Option<Engine>,
    version: String,
    port: u16,
    source: ProfileSource,
) -> Result<Profile, AppError> {
    let data_dir = Path::new(&datadir_path);
    let detected = Engine::detect_from_datadir(data_dir);

    // What the folder says wins over what the form said: a user who picks
    // "MySQL" for a MariaDB datadir would otherwise have saved a profile
    // that corrupts on first start.
    let engine = match (detected, engine) {
        (Some(found), _) => found,
        (None, Some(chosen)) => chosen,
        (None, None) => {
            return Err(AppError::NotADatadir {
                path: datadir_path,
                engine: "MySQL or MariaDB".to_string(),
            })
        }
    };

    let mut store = store();
    let profile = profiles::add(
        &mut store,
        name,
        datadir_path,
        engine,
        version,
        port,
        source,
    )?;
    persist(&store);
    Ok(profile)
}

pub fn remove(id: &str) -> Result<(), AppError> {
    let mut store = store();
    profiles::remove(&mut store, id)?;
    persist(&store);
    Ok(())
}

/// A datadir found on the machine that isn't a profile yet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedDatadir {
    pub name: String,
    pub datadir_path: String,
    pub engine: Engine,
    pub version: String,
    pub source: ProfileSource,
    /// The engine binary that came with it, if one was found next to the
    /// config. Offered as a one-click "use this build" in the add flow,
    /// since a datadir is useless without a binary of its own major.minor.
    pub binary_dir: Option<String>,
}

/// Scans for other tools' MySQL/MariaDB data.
///
/// Read-only and never registers anything by itself — the caller shows what
/// was found and the user decides. Anything already registered as a profile
/// is filtered out so a second scan doesn't re-offer it.
pub fn detect() -> Vec<DetectedDatadir> {
    let store = store();
    let mut found = detect_laragon();
    found.extend(detect_xampp());
    found.retain(|candidate| {
        store
            .datadir_taken_by(&candidate.datadir_path, None)
            .is_none()
    });
    found
}

/// Laragon keeps one folder per installed server under `bin\mysql`, each
/// with its own `my.ini` naming the datadir it uses — which is *not*
/// derivable from the folder name (`mysql-8.4.3-winx64` uses
/// `data\mysql-8.4`), so the ini is read rather than guessed.
fn detect_laragon() -> Vec<DetectedDatadir> {
    let bin_root = Path::new(r"C:\laragon\bin\mysql");
    let Ok(entries) = std::fs::read_dir(bin_root) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let server_dir = entry.path();
        let Some(folder) = server_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(data_dir) = read_ini_datadir(&server_dir.join("my.ini")) else {
            continue;
        };
        let data_path = PathBuf::from(&data_dir);
        let Some(engine) = Engine::detect_from_datadir(&data_path) else {
            // No readable datadir behind this server — nothing to adopt.
            continue;
        };

        found.push(DetectedDatadir {
            name: format!("Laragon {}", version_from(folder)),
            datadir_path: data_path.display().to_string(),
            engine,
            version: version_from(folder),
            source: ProfileSource::Laragon,
            binary_dir: dir_with_server(&server_dir),
        });
    }
    found
}

/// XAMPP has a single fixed layout, so the datadir is known and only the
/// version has to be asked for — from the binary itself, since nothing in
/// the path carries it.
fn detect_xampp() -> Vec<DetectedDatadir> {
    let data_path = PathBuf::from(r"C:\xampp\mysql\data");
    let Some(engine) = Engine::detect_from_datadir(&data_path) else {
        return Vec::new();
    };
    let bin_dir = PathBuf::from(r"C:\xampp\mysql\bin");

    vec![DetectedDatadir {
        name: "XAMPP".to_string(),
        datadir_path: data_path.display().to_string(),
        engine,
        version: server_version(&bin_dir.join(SERVER_EXE)).unwrap_or_default(),
        source: ProfileSource::Xampp,
        binary_dir: dir_with_server(bin_dir.parent().unwrap_or(&bin_dir)),
    }]
}

/// Pulls `datadir=...` out of a `my.ini`, tolerating the quoting and
/// slash direction these files vary in.
fn read_ini_datadir(ini: &Path) -> Option<String> {
    let content = std::fs::read_to_string(ini).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some(rest) = line.strip_prefix("datadir") else {
            continue;
        };
        let value = rest
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .trim_matches('"');
        if !value.is_empty() {
            return Some(value.replace('/', "\\"));
        }
    }
    None
}

/// The first `1.2`/`1.2.3`-shaped token in a folder name, reusing the same
/// parser the PHP switcher uses so `mysql-8.4.3-winx64` reads as `8.4.3`.
fn version_from(folder: &str) -> String {
    binaries::version_from_folder_name(folder).unwrap_or_default()
}

/// The folder holding `mysqld.exe` — either this one or its `bin`.
fn dir_with_server(dir: &Path) -> Option<String> {
    for candidate in [dir.to_path_buf(), dir.join("bin")] {
        if candidate.join(SERVER_EXE).is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// Asks a server binary its own version, for datadirs whose path carries
/// none. Output looks like `mysqld  Ver 10.4.32-MariaDB for Win64`.
fn server_version(exe: &Path) -> Option<String> {
    let output = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let after_ver = text.split("Ver ").nth(1)?;
    let token = after_ver.split_whitespace().next()?;
    binaries::version_from_folder_name(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_bump_is_safe_but_a_minor_bump_is_not() {
        // The spec's own example: same major.minor, newer patch.
        assert_eq!(compatibility("8.0.30", "8.0.36"), Compatibility::Compatible);
        // MySQL 8.0 -> 8.4 rewrites the datadir one-way, despite both being "8".
        assert_eq!(
            compatibility("8.0.30", "8.4.3"),
            Compatibility::NeedsUpgrade
        );
        // MariaDB's equivalent.
        assert_eq!(
            compatibility("10.4.32", "10.11.6"),
            Compatibility::NeedsUpgrade
        );
    }

    #[test]
    fn a_different_major_is_never_openable() {
        // Laragon ships both of these; picking the wrong one must not start.
        assert_eq!(compatibility("8.4.3", "9.6.0"), Compatibility::Incompatible);
        assert_eq!(
            compatibility("10.4.32", "11.2.2"),
            Compatibility::Incompatible
        );
    }

    /// An unreadable version must never fall through to "compatible" — the
    /// failure mode there is starting the wrong binary against real data.
    #[test]
    fn an_unparseable_version_is_treated_as_incompatible() {
        assert_eq!(compatibility("", "8.4.3"), Compatibility::Incompatible);
        assert_eq!(
            compatibility("unknown", "8.4.3"),
            Compatibility::Incompatible
        );
    }

    #[test]
    fn a_bare_major_minor_matches_a_full_patch_version() {
        assert_eq!(compatibility("8.4", "8.4.3"), Compatibility::Compatible);
    }

    #[test]
    fn a_profile_whose_engine_has_no_binaries_reports_which_is_missing() {
        let profile = Profile {
            id: "test".to_string(),
            name: "Nothing installed".to_string(),
            datadir_path: r"C:\nowhere".to_string(),
            // Nothing discovers a "mysql" family install unless the user
            // added one, which is exactly the state this asserts on.
            engine: Engine::MySql,
            version: "8.4.3".to_string(),
            port: 3306,
            source: ProfileSource::Laragon,
            is_default: false,
            last_used_at: None,
        };

        if installed_binaries(Engine::MySql).is_empty() {
            let err = resolve_server_exe(&profile).unwrap_err();
            assert!(
                matches!(err, AppError::EngineBinaryMissing { ref engine, .. } if engine == "MySQL"),
                "got: {err}"
            );
        }
    }

    /// Refusing to open a MariaDB datadir with MySQL (or vice versa) is the
    /// single most important thing this module does.
    #[test]
    fn a_datadir_written_by_the_other_engine_is_refused() {
        let dir =
            std::env::temp_dir().join(format!("rezure-test-switchgate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A MariaDB datadir...
        std::fs::write(dir.join("aria_log_control"), b"").unwrap();

        // ...claimed by a profile that says MySQL.
        let profile = Profile {
            id: "test".to_string(),
            name: "Mislabelled".to_string(),
            datadir_path: dir.display().to_string(),
            engine: Engine::MySql,
            version: "8.4.3".to_string(),
            port: 3306,
            source: ProfileSource::Custom,
            is_default: false,
            last_used_at: None,
        };

        let err = check_can_switch_to(&profile).unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            matches!(err, AppError::EngineMismatch { .. }),
            "the engine mismatch must be caught before anything else, got: {err}"
        );
    }

    /// Reports what the real machine would allow. Run with:
    /// `cargo test --lib services::db_profiles::tests::print_real_state -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_real_state() {
        for engine in [Engine::MySql, Engine::MariaDb] {
            let found = installed_binaries(engine);
            println!(
                "{}: {} installed {:?}",
                engine.label(),
                found.len(),
                found.iter().map(|r| &r.version).collect::<Vec<_>>()
            );
        }
        for status in list() {
            println!(
                "profile {:12} {:8} {:10} active={:5} binary={:5} {}",
                status.profile.name,
                status.profile.engine.label(),
                status.profile.version,
                status.active,
                status.binary_available,
                status.profile.datadir_path,
            );
        }
        for found in detect() {
            println!(
                "detected {:16} {:8} {:10} {} (binaries: {:?})",
                found.name,
                found.engine.label(),
                found.version,
                found.datadir_path,
                found.binary_dir,
            );
        }
    }
}
