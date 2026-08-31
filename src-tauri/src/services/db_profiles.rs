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
use crate::config::profiles::{self, NewProfile, Profile, ProfileSource, ProfileStore};
use crate::utils::command::HiddenWindow;
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
        let mut changed = false;
        if let Ok(datadir) = rezure_datadir() {
            changed =
                profiles::ensure_default(&mut store, &datadir, Engine::MariaDb, version, 3306);
        }
        changed |= heal_missing_defaults_file(&mut store);

        if changed {
            if let Err(err) = profiles::save(&store) {
                log::warn!("could not persist the seeded profile store: {err}");
            }
        }
        Mutex::new(store)
    })
}

fn store() -> MutexGuard<'static, ProfileStore> {
    store_cell().lock().unwrap()
}

/// Fills in `defaults_file` for adopted profiles saved before Rezure knew
/// to record it.
///
/// Those profiles start their server with no `--defaults-file` and fail on
/// the privilege tables — a real failure that shipped, so healing them is
/// worth more than asking the user to delete and re-add. The `my.ini` sits
/// next to the binary already recorded, which is exactly where the tool
/// that owns it keeps one.
fn heal_missing_defaults_file(store: &mut ProfileStore) -> bool {
    let mut changed = false;

    for profile in &mut store.profiles {
        if profile.defaults_file.is_some() {
            continue;
        }
        let Some(binary_dir) = &profile.binary_dir else {
            continue;
        };

        // Both layouts the two tools actually use: XAMPP keeps `my.ini`
        // beside the binaries in `mysql\bin`, Laragon one level up in the
        // server folder that contains `bin`.
        let dir = Path::new(binary_dir);
        let found = [Some(dir.to_path_buf()), dir.parent().map(Path::to_path_buf)]
            .into_iter()
            .flatten()
            .flat_map(|base| [base.join("my.ini"), base.join("my.cnf")])
            .find(|candidate| candidate.is_file());

        if let Some(ini) = found {
            log::info!(
                "profile \"{}\" had no config recorded; adopting {}",
                profile.name,
                ini.display()
            );
            profile.defaults_file = Some(ini.display().to_string());
            changed = true;
        }
    }
    changed
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
    // A profile that names its own build wins outright — that's the whole
    // point of adopting one, and silently falling back to a different
    // version would start the wrong binary against real data.
    if let Some(dir) = &profile.binary_dir {
        return server_exe_in(Path::new(dir)).ok_or_else(|| AppError::EngineBinaryMissing {
            engine: profile.engine.label().to_string(),
            version: format!("{} (at {dir})", profile.version),
        });
    }

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

/// `mysqld.exe` inside `dir`, or inside its `bin` — the two shapes a
/// server folder comes in, matching what [`detect`] reports.
fn server_exe_in(dir: &Path) -> Option<PathBuf> {
    [dir.join(SERVER_EXE), dir.join("bin").join(SERVER_EXE)]
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// Asks a server binary which engine and version it really is.
///
/// `mysqld --version` prints e.g. `mysqld  Ver 10.4.32-MariaDB for Win64`
/// or `mysqld  Ver 8.4.3 for Win64`. Trusting the folder name instead is
/// how a profile ends up pointed at a build that can't open its datadir.
fn identify_server(exe: &Path) -> Option<(Engine, String)> {
    let output = std::process::Command::new(exe)
        .arg("--version")
        .hidden()
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();

    let engine = if text.to_lowercase().contains("mariadb") {
        Engine::MariaDb
    } else {
        Engine::MySql
    };
    let version = binaries::version_from_folder_name(text.split("Ver ").nth(1)?)?;
    Some((engine, version))
}

/// Whether a live server process already owns this datadir.
///
/// Starting a second server against a datadir another one has open is the
/// single action that reliably corrupts InnoDB, so this is the check the
/// whole feature's safety rests on.
///
/// # Why the running processes, and not the pid file
///
/// A server writes a `<hostname>.pid` into its datadir and removes it on a
/// clean stop, which makes it a tempting thing to read. It isn't reliable:
/// on this machine Laragon's datadir held a `.pid` naming process 24496
/// while nothing of the sort was running, and — worse for us — a datadir
/// whose owner was force-killed keeps a pid file that outlives it. Trusting
/// it fails in *both* directions: refusing a switch that's perfectly safe,
/// and allowing one that isn't when a live server's pid file is missing or
/// out of date.
///
/// So the authority is the process table: every live `mysqld`/`mariadbd` is
/// asked what `--datadir` it was started with, and the answer is compared as
/// a path. That's the same fact the pid file was only ever an approximation
/// of, read from the thing that actually knows it.
fn datadir_holder(data_dir: &Path) -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );

    let target = data_dir.display().to_string();

    for (pid, process) in sys.processes() {
        let name = process.name().to_string_lossy().to_lowercase();
        if !name.contains("mysqld") && !name.contains("mariadbd") {
            continue;
        }

        let holds_it = process.cmd().iter().any(|arg| {
            arg.to_string_lossy()
                .strip_prefix("--datadir=")
                .is_some_and(|dir| profiles::same_path(dir, &target))
        });
        if holds_it {
            return Some(pid.as_u32());
        }
    }
    None
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

    // Last, because it's the only check that has to walk the process table.
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

/// What the add-profile flow supplies. `engine` is a *suggestion* — the
/// datadir's own markers win where they exist.
pub struct AddProfile {
    pub name: String,
    pub datadir_path: String,
    pub engine: Option<Engine>,
    pub version: String,
    pub port: u16,
    pub source: ProfileSource,
    pub binary_dir: Option<String>,
    pub defaults_file: Option<String>,
}

/// Registers an existing datadir as a profile. Reads the engine off the
/// folder rather than trusting the caller — see [`Engine::detect_from_datadir`].
pub fn add(request: AddProfile) -> Result<Profile, AppError> {
    let AddProfile {
        name,
        datadir_path,
        engine,
        version,
        port,
        source,
        binary_dir,
        defaults_file,
    } = request;
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

    // A named build is checked against the datadir it's being paired with
    // *before* the profile is saved, so a mismatch surfaces here rather
    // than as a failed switch — or worse, a successful one onto the wrong
    // engine. The binary's own `--version` is the authority, not the path.
    let version = match &binary_dir {
        Some(dir) => {
            let exe = server_exe_in(Path::new(dir)).ok_or_else(|| AppError::NotADatadir {
                path: dir.clone(),
                engine: format!("a folder holding {SERVER_EXE}"),
            })?;
            let (binary_engine, binary_version) =
                identify_server(&exe).ok_or_else(|| AppError::NotADatadir {
                    path: dir.clone(),
                    engine: "a working database server".to_string(),
                })?;

            if binary_engine != engine {
                return Err(AppError::EngineMismatch {
                    found: engine.label().to_string(),
                    expected: binary_engine.label().to_string(),
                });
            }
            // Trust the binary over whatever version the form carried.
            binary_version
        }
        None => version,
    };

    let mut store = store();
    let profile = profiles::add(
        &mut store,
        NewProfile {
            name,
            datadir_path,
            engine,
            version,
            port,
            source,
            binary_dir,
            defaults_file,
        },
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
    /// The `my.ini` that build launches with — carried through to the
    /// profile because the datadir depends on it, not merely prefers it.
    pub defaults_file: Option<String>,
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
        let ini = server_dir.join("my.ini");
        let Some(data_dir) = read_ini_datadir(&ini) else {
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
            defaults_file: Some(ini.display().to_string()),
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
        defaults_file: Some(bin_dir.join("my.ini"))
            .filter(|ini| ini.is_file())
            .map(|ini| ini.display().to_string()),
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
        .hidden()
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
            binary_dir: None,
            defaults_file: None,
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
            binary_dir: None,
            defaults_file: None,
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

    /// The whole adoption path against this machine's real Laragon install:
    /// detect it, register it as a profile, ask whether it could be switched
    /// to, then remove it again.
    ///
    /// Deliberately makes no claim about *which* answer the gate gives — that
    /// depends on whether Laragon's own server happens to be running right
    /// now. What it asserts is that the answer is a reasoned one: either the
    /// switch is allowed, or it's refused for a stated, recoverable reason,
    /// never an unexplained failure. Run with:
    /// `cargo test --lib services::db_profiles::tests::adopt_a_real_install -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn adopt_a_real_install() {
        let Some(found) = detect().into_iter().next() else {
            println!("nothing detected on this machine — nothing to adopt");
            return;
        };
        println!(
            "adopting {} ({} {}) at {}",
            found.name,
            found.engine.label(),
            found.version,
            found.datadir_path
        );

        let profile = add(AddProfile {
            name: found.name.clone(),
            datadir_path: found.datadir_path.clone(),
            engine: Some(found.engine),
            version: found.version.clone(),
            port: 3306,
            source: found.source,
            binary_dir: found.binary_dir.clone(),
            defaults_file: found.defaults_file.clone(),
        })
        .expect("adopting a detected datadir must succeed");

        // The binary has to resolve, or the profile is unusable on sight.
        let exe = resolve_server_exe(&profile);
        println!("resolved binary: {exe:?}");
        assert!(
            exe.is_ok(),
            "an adopted profile must resolve the build it was registered with"
        );

        match check_can_switch_to(&profile) {
            Ok(()) => println!("switch would be ALLOWED"),
            Err(err) => {
                println!("switch would be REFUSED: {err}");
                // Whatever the reason, it has to be one of the ones the UI
                // knows how to explain — not a bare io error.
                assert!(
                    matches!(
                        err,
                        AppError::DatadirInUse { .. }
                            | AppError::EngineMismatch { .. }
                            | AppError::EngineBinaryMissing { .. }
                    ),
                    "a refusal must be a stated reason, got: {err}"
                );
            }
        }

        remove(&profile.id).expect("cleanup");
        assert!(
            store().find(&profile.id).is_err(),
            "the test profile must not be left behind"
        );
    }

    /// Proves the in-use check reads the process table rather than the pid
    /// file, against whatever server is running right now.
    ///
    /// The case that motivated it: this machine had a `.pid` in Laragon's
    /// datadir naming a long-dead process, while a *different* live server
    /// held a different datadir. A pid-file check got both answers wrong.
    /// Run with:
    /// `cargo test --lib services::db_profiles::tests::in_use_follows_the_running_process -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn in_use_follows_the_running_process() {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );

        let mut checked = 0;
        for (pid, process) in sys.processes() {
            let name = process.name().to_string_lossy().to_lowercase();
            if !name.contains("mysqld") && !name.contains("mariadbd") {
                continue;
            }
            let Some(dir) = process.cmd().iter().find_map(|arg| {
                arg.to_string_lossy()
                    .strip_prefix("--datadir=")
                    .map(str::to_string)
            }) else {
                continue;
            };

            println!("live server {pid} holds {dir}");
            assert_eq!(
                datadir_holder(Path::new(&dir)),
                Some(pid.as_u32()),
                "a datadir held by a running server must read as in use"
            );
            checked += 1;
        }

        if checked == 0 {
            println!("no database server running — nothing to check");
            return;
        }

        // And a folder nobody has open must not read as in use, or every
        // switch would be blocked forever.
        assert_eq!(
            datadir_holder(Path::new(r"C:\definitely\not\a\datadir")),
            None
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
            // What `add` would actually record — the binary's own answer,
            // which has to agree with the datadir it's being paired with.
            if let Some(dir) = &found.binary_dir {
                let identified =
                    server_exe_in(Path::new(dir)).and_then(|exe| identify_server(&exe));
                println!("   binary identifies itself as {identified:?}");
                if let Some((engine, _)) = identified {
                    assert_eq!(
                        engine, found.engine,
                        "the binary's engine must match the datadir's"
                    );
                }
            }
        }
    }
}
