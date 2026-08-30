//! Database profiles — which datadir the one `mysqld` Rezure runs is
//! currently pointed at.
//!
//! # Why profiles at all
//!
//! A single server process can only own one datadir at a time; two
//! processes against the same folder corrupt it. So Rezure keeps running
//! exactly one server and makes the *datadir* the thing that switches. A
//! profile is a saved answer to "which data, which engine, which port".
//!
//! Nothing here ever reads, writes, copies or migrates a datadir. A profile
//! records a path and the engine that wrote it — adopting someone else's
//! data must be non-destructive, or the feature is worse than useless.
//!
//! Stored beside `settings.json` in `%APPDATA%\Rezure\profiles.json`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::services::db_engine::Engine;
use crate::utils::error::AppError;

/// Where a profile came from — label and icon only, never behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSource {
    Rezure,
    Laragon,
    Xampp,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Stable across renames and path edits, so the active-profile pointer
    /// never has to be rewritten when a profile is edited.
    pub id: String,
    pub name: String,
    pub datadir_path: String,
    /// Which engine wrote this datadir. Detected from the folder where
    /// possible (`Engine::detect_from_datadir`) rather than assumed —
    /// opening a datadir with the wrong engine is the corrupting mistake.
    pub engine: Engine,
    /// The `major.minor[.patch]` that created it, used to pick a compatible
    /// binary. Empty when unknown and the user declined to say.
    pub version: String,
    pub port: u16,
    pub source: ProfileSource,
    /// A specific server build to run this profile on, when Rezure shouldn't
    /// pick one itself.
    ///
    /// Set when adopting another tool's data: Laragon's MySQL 8.4 datadir
    /// needs Laragon's own MySQL 8.4 binary, and that build is already on
    /// disk — pointing at it costs nothing, while copying it would duplicate
    /// a quarter of a gigabyte to no end. It's also consistent with what a
    /// profile already is: a pointer to somebody else's folder. If that tool
    /// is uninstalled its datadir goes with it, so an owned copy of the
    /// binary would have outlived the only data it could open.
    ///
    /// `None` means "resolve from the versions Rezure knows about" — the
    /// path Rezure's own profile takes.
    #[serde(default)]
    pub binary_dir: Option<String>,
    /// The `my.ini` this install's datadir depends on.
    ///
    /// Not optional detail — a datadir is only readable under the config it
    /// was created with. XAMPP's sets `plugin_dir`, and without it the
    /// server can't load Aria, so the `mysql.db` privilege table (an Aria
    /// table) reads as `Incorrect file format 'db'` and startup aborts.
    /// Laragon and XAMPP both launch their servers with `--defaults-file`
    /// for exactly this reason, so an adopted profile has to as well.
    ///
    /// `None` for a datadir Rezure created, which depends on no config
    /// beyond the flags it's started with.
    #[serde(default)]
    pub defaults_file: Option<String>,
    /// True for the datadir Rezure created and owns. Exactly one profile has
    /// it, and it's the fallback when an adopted profile can't be started.
    pub is_default: bool,
    /// Unix seconds, for ordering the switcher by recency.
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStore {
    pub profiles: Vec<Profile>,
    pub active_profile_id: Option<String>,
}

impl ProfileStore {
    pub fn active(&self) -> Option<&Profile> {
        let id = self.active_profile_id.as_deref()?;
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn find(&self, id: &str) -> Result<&Profile, AppError> {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::ProfileNotFound(id.to_string()))
    }

    pub fn default_profile(&self) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.is_default)
    }

    /// Rejects a second profile pointing at a datadir already registered —
    /// two profiles over one folder is how the "only one process per
    /// datadir" rule gets broken by accident.
    pub fn datadir_taken_by(&self, datadir: &str, excluding_id: Option<&str>) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|p| same_path(&p.datadir_path, datadir) && Some(p.id.as_str()) != excluding_id)
    }
}

/// Windows path comparison: case-insensitive, trailing separator ignored,
/// slashes normalized — `C:/laragon/data` and `C:\Laragon\data\` are the
/// same folder, and treating them as different would let two profiles claim
/// one datadir.
pub fn same_path(a: &str, b: &str) -> bool {
    let normalize = |p: &str| {
        p.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    normalize(a) == normalize(b)
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Unique enough for a hand-maintained local list: the creation timestamp in
/// nanoseconds, hex-encoded. No `uuid` dependency for something only ever
/// generated one profile at a time, by a human clicking a button.
fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// `%APPDATA%\Rezure\profiles.json`.
pub fn path() -> Result<PathBuf, AppError> {
    let base = dirs::config_dir()
        .ok_or_else(|| AppError::Settings("could not resolve the config directory".to_string()))?;
    Ok(base.join("Rezure").join("profiles.json"))
}

/// Reads the store, falling back to an empty one if it's missing or
/// unreadable — a corrupt profiles file must not block startup, and
/// [`ensure_default`] re-seeds the Rezure-owned profile straight after.
pub fn load() -> ProfileStore {
    let Ok(path) = path() else {
        return ProfileStore::default();
    };
    load_from(&path)
}

fn load_from(path: &Path) -> ProfileStore {
    let Ok(content) = std::fs::read_to_string(path) else {
        return ProfileStore::default();
    };
    serde_json::from_str(&content).unwrap_or_else(|err| {
        log::warn!(
            "profiles file at {} is unreadable, starting fresh: {err}",
            path.display()
        );
        ProfileStore::default()
    })
}

pub fn save(store: &ProfileStore) -> Result<(), AppError> {
    save_to(&path()?, store)
}

fn save_to(path: &Path, store: &ProfileStore) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Settings(format!("could not create {}: {e}", parent.display()))
        })?;
    }
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| AppError::Settings(format!("could not serialize profiles: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| AppError::Settings(format!("could not write {}: {e}", path.display())))
}

/// Guarantees the store always has the Rezure-owned profile, marked active
/// if nothing else is. Called once at startup so a fresh install — or a
/// store that lost its default to a hand-edit — still has somewhere to run.
pub fn ensure_default(
    store: &mut ProfileStore,
    rezure_datadir: &Path,
    engine: Engine,
    version: String,
    port: u16,
) -> bool {
    let mut changed = false;

    if store.default_profile().is_none() {
        store.profiles.insert(
            0,
            Profile {
                id: new_id(),
                name: "Rezure".to_string(),
                datadir_path: rezure_datadir.display().to_string(),
                engine,
                version,
                port,
                source: ProfileSource::Rezure,
                binary_dir: None,
                defaults_file: None,
                is_default: true,
                last_used_at: None,
            },
        );
        changed = true;
    }

    // An active pointer at a profile that no longer exists reads as "no
    // active profile" everywhere else, so it's healed rather than trusted.
    let active_valid = store
        .active_profile_id
        .as_deref()
        .is_some_and(|id| store.profiles.iter().any(|p| p.id == id));
    if !active_valid {
        store.active_profile_id = store.default_profile().map(|p| p.id.clone());
        changed = true;
    }

    changed
}

/// Everything needed to register a datadir, minus the bookkeeping
/// (`id`, `is_default`, `last_used_at`) this module fills in itself.
pub struct NewProfile {
    pub name: String,
    pub datadir_path: String,
    pub engine: Engine,
    pub version: String,
    pub port: u16,
    pub source: ProfileSource,
    pub binary_dir: Option<String>,
    pub defaults_file: Option<String>,
}

/// Adds a profile for an existing datadir. Records the path and nothing
/// else — the folder itself is never touched.
pub fn add(store: &mut ProfileStore, new: NewProfile) -> Result<Profile, AppError> {
    if let Some(existing) = store.datadir_taken_by(&new.datadir_path, None) {
        return Err(AppError::DatadirAlreadyRegistered {
            path: new.datadir_path,
            name: existing.name.clone(),
        });
    }

    let profile = Profile {
        id: new_id(),
        name: new.name,
        datadir_path: new.datadir_path,
        engine: new.engine,
        version: new.version,
        port: new.port,
        source: new.source,
        binary_dir: new.binary_dir,
        defaults_file: new.defaults_file,
        is_default: false,
        last_used_at: None,
    };
    store.profiles.push(profile.clone());
    Ok(profile)
}

/// Removes a profile. The Rezure-owned default can't go — it's the fallback
/// every failed switch rolls back to — and neither can the active one,
/// which would leave the running server unaccounted for.
pub fn remove(store: &mut ProfileStore, id: &str) -> Result<(), AppError> {
    let profile = store.find(id)?;
    if profile.is_default {
        return Err(AppError::ProfileUndeletable(
            "it's the profile Rezure falls back to".to_string(),
        ));
    }
    if store.active_profile_id.as_deref() == Some(id) {
        return Err(AppError::ProfileUndeletable(
            "it's the profile that's currently active — switch away from it first".to_string(),
        ));
    }
    store.profiles.retain(|p| p.id != id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_default() -> ProfileStore {
        let mut store = ProfileStore::default();
        ensure_default(
            &mut store,
            Path::new(r"C:\Rezure\data"),
            Engine::MariaDb,
            "11.2.2".to_string(),
            3306,
        );
        store
    }

    #[test]
    fn a_fresh_store_is_seeded_with_an_active_rezure_profile() {
        let store = store_with_default();

        assert_eq!(store.profiles.len(), 1);
        let default = store.default_profile().unwrap();
        assert_eq!(default.source, ProfileSource::Rezure);
        assert_eq!(
            store.active().map(|p| p.id.clone()),
            Some(default.id.clone())
        );
    }

    #[test]
    fn seeding_is_idempotent() {
        let mut store = store_with_default();
        let before = store.profiles.len();

        let changed = ensure_default(
            &mut store,
            Path::new(r"C:\Rezure\data"),
            Engine::MariaDb,
            "11.2.2".to_string(),
            3306,
        );

        assert!(!changed, "nothing to fix on a healthy store");
        assert_eq!(store.profiles.len(), before);
    }

    /// A store whose active pointer went stale (hand-edited file, deleted
    /// profile) must heal rather than report no active profile at all.
    #[test]
    fn a_dangling_active_pointer_falls_back_to_the_default() {
        let mut store = store_with_default();
        store.active_profile_id = Some("does-not-exist".to_string());

        assert!(ensure_default(
            &mut store,
            Path::new(r"C:\Rezure\data"),
            Engine::MariaDb,
            "11.2.2".to_string(),
            3306,
        ));
        assert_eq!(
            store.active().map(|p| p.id.clone()),
            store.default_profile().map(|p| p.id.clone())
        );
    }

    /// The rule the whole design rests on: never two profiles over one
    /// datadir, however the path happens to be spelled.
    #[test]
    fn a_second_profile_cannot_claim_the_same_datadir() {
        let mut store = store_with_default();
        add(
            &mut store,
            NewProfile {
                name: "Laragon".to_string(),
                datadir_path: r"C:\laragon\data\mysql-8.4".to_string(),
                engine: Engine::MySql,
                version: "8.4.3".to_string(),
                port: 3306,
                source: ProfileSource::Laragon,
                binary_dir: None,
                defaults_file: None,
            },
        )
        .unwrap();

        let err = add(
            &mut store,
            NewProfile {
                name: "Laragon again".to_string(),
                // Same folder, different spelling — forward slashes, trailing
                // separator, different case.
                datadir_path: "c:/Laragon/Data/mysql-8.4/".to_string(),
                engine: Engine::MySql,
                version: "8.4.3".to_string(),
                port: 3307,
                source: ProfileSource::Custom,
                binary_dir: None,
                defaults_file: None,
            },
        )
        .unwrap_err();

        assert!(matches!(err, AppError::DatadirAlreadyRegistered { .. }));
        assert_eq!(store.profiles.len(), 2);
    }

    #[test]
    fn windows_paths_compare_ignoring_case_slashes_and_trailing_separators() {
        assert!(same_path(r"C:\laragon\data", "c:/LARAGON/data/"));
        assert!(same_path(r"C:\a\b\", r"C:\a\b"));
        assert!(!same_path(r"C:\a\b", r"C:\a\c"));
    }

    #[test]
    fn the_default_profile_cannot_be_deleted() {
        let mut store = store_with_default();
        let default_id = store.default_profile().unwrap().id.clone();

        let err = remove(&mut store, &default_id).unwrap_err();
        assert!(matches!(err, AppError::ProfileUndeletable(_)));
        assert_eq!(store.profiles.len(), 1);
    }

    #[test]
    fn the_active_profile_cannot_be_deleted() {
        let mut store = store_with_default();
        let added = add(
            &mut store,
            NewProfile {
                name: "Laragon".to_string(),
                datadir_path: r"C:\laragon\data\mysql-8.4".to_string(),
                engine: Engine::MySql,
                version: "8.4.3".to_string(),
                port: 3306,
                source: ProfileSource::Laragon,
                binary_dir: None,
                defaults_file: None,
            },
        )
        .unwrap();
        store.active_profile_id = Some(added.id.clone());

        assert!(matches!(
            remove(&mut store, &added.id),
            Err(AppError::ProfileUndeletable(_))
        ));
    }

    #[test]
    fn a_non_active_profile_is_removable() {
        let mut store = store_with_default();
        let added = add(
            &mut store,
            NewProfile {
                name: "XAMPP".to_string(),
                datadir_path: r"C:\xampp\mysql\data".to_string(),
                engine: Engine::MariaDb,
                version: "10.4.32".to_string(),
                port: 3306,
                source: ProfileSource::Xampp,
                binary_dir: None,
                defaults_file: None,
            },
        )
        .unwrap();

        remove(&mut store, &added.id).unwrap();
        assert_eq!(store.profiles.len(), 1);
    }

    #[test]
    fn save_then_load_round_trips_every_field() {
        let path =
            std::env::temp_dir().join(format!("rezure-test-profiles-{}.json", std::process::id()));
        let mut store = store_with_default();
        add(
            &mut store,
            NewProfile {
                name: "Laragon".to_string(),
                datadir_path: r"C:\laragon\data\mysql-8.4".to_string(),
                engine: Engine::MySql,
                version: "8.4.3".to_string(),
                port: 3307,
                source: ProfileSource::Laragon,
                binary_dir: None,
                defaults_file: None,
            },
        )
        .unwrap();

        save_to(&path, &store).unwrap();
        let loaded = load_from(&path);

        assert_eq!(loaded.profiles.len(), 2);
        let laragon = loaded
            .profiles
            .iter()
            .find(|p| p.name == "Laragon")
            .unwrap();
        assert_eq!(laragon.engine, Engine::MySql);
        assert_eq!(laragon.port, 3307);
        assert_eq!(laragon.source, ProfileSource::Laragon);
        assert_eq!(loaded.active_profile_id, store.active_profile_id);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_corrupt_profiles_file_reads_as_an_empty_store_instead_of_panicking() {
        let path = std::env::temp_dir().join(format!(
            "rezure-test-profiles-corrupt-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "{ not json").unwrap();

        assert!(load_from(&path).profiles.is_empty());
        std::fs::remove_file(&path).unwrap();
    }
}
