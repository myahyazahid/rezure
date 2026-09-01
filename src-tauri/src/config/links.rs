//! Projects that live outside `www` — folders the user pointed Rezure at
//! rather than moved into it.
//!
//! # Why this is config and not a database row
//!
//! `services::projects::scan_projects` is the one place that answers "what
//! projects exist", and five callers depend on it (the list, vhosts, the
//! hosts file, the launcher, and database-to-project matching). It's a plain
//! synchronous function; the SQLite connection lives in Tauri managed state
//! and can't be reached from `services/`. So the registry of linked folders
//! lives here, in JSON behind a `OnceLock`, the same way `config::profiles`
//! does — leaving SQLite to what it's for: history *about* projects, which
//! is derived and disposable, rather than the record of which ones exist.
//!
//! Linking never copies, moves or writes anything inside the folder. It
//! records a path; unlinking forgets it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::utils::error::AppError;
use crate::utils::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedProject {
    /// Stable across renames of `name`, and unique across roots — two
    /// folders called `api` in different places must not collide, since
    /// `services::launcher` resolves an id back to a folder to open.
    pub id: String,
    pub path: String,
    /// Display name. Defaults to the folder name, editable.
    pub name: String,
    /// The `.test` domain nginx serves it on. Chosen at link time so a
    /// clash with an existing project is settled there rather than
    /// surfacing later as two vhosts fighting over one name.
    pub domain: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LinkStore {
    pub links: Vec<LinkedProject>,
}

impl LinkStore {
    pub fn linked_at(&self, path: &str) -> Option<&LinkedProject> {
        self.links.iter().find(|link| same_path(&link.path, path))
    }
}

/// Windows path comparison: case-insensitive, slash direction and trailing
/// separator ignored. Without it the same folder could be linked twice
/// under two spellings and produce two vhosts.
pub fn same_path(a: &str, b: &str) -> bool {
    let normalize = |p: &str| {
        p.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    normalize(a) == normalize(b)
}

/// Whether `path` sits inside `ancestor` (or is it) — used to keep a link
/// from shadowing a folder `www` already scans.
pub fn is_inside(path: &str, ancestor: &Path) -> bool {
    let normalize = |p: &str| {
        p.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let path = normalize(path);
    let ancestor = normalize(&ancestor.display().to_string());
    path == ancestor || path.starts_with(&format!("{ancestor}\\"))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A lowercase, hyphenated form of `name`, safe in a domain and a filename.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = true; // trims leading separators

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

/// `<slug>-<6 hex of the path>`.
///
/// The hash is what makes two folders named `api`, in different roots,
/// distinguishable — and it's derived from the path rather than random so
/// re-linking the same folder produces the same id, and its history
/// (`db::projects`) is picked back up instead of starting over.
pub fn id_for(path: &str, name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        path.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
            .as_bytes(),
    );
    let digest = hex::encode(hasher.finalize());
    let slug = slugify(name);
    let slug = if slug.is_empty() { "project" } else { &slug };
    format!("{slug}-{}", &digest[..6])
}

/// `%APPDATA%\Rezure\links.json`.
pub fn path() -> Result<PathBuf, AppError> {
    Ok(paths::etc()?.join("links.json"))
}

/// Reads the registry, falling back to empty if it's missing or unreadable
/// — a corrupt file must not stop the project list from loading at all.
pub fn load() -> LinkStore {
    let Ok(path) = path() else {
        return LinkStore::default();
    };
    load_from(&path)
}

fn load_from(path: &Path) -> LinkStore {
    let Ok(content) = std::fs::read_to_string(path) else {
        return LinkStore::default();
    };
    serde_json::from_str(&content).unwrap_or_else(|err| {
        log::warn!(
            "linked-projects file at {} is unreadable, starting empty: {err}",
            path.display()
        );
        LinkStore::default()
    })
}

pub fn save(store: &LinkStore) -> Result<(), AppError> {
    save_to(&path()?, store)
}

fn save_to(path: &Path, store: &LinkStore) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Settings(format!("could not create {}: {e}", parent.display()))
        })?;
    }
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| AppError::Settings(format!("could not serialize linked projects: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| AppError::Settings(format!("could not write {}: {e}", path.display())))
}

/// Adds a link. The caller has already validated the path and settled the
/// domain — see `services::projects::prepare_link`.
pub fn add(store: &mut LinkStore, path: String, name: String, domain: String) -> LinkedProject {
    let link = LinkedProject {
        id: id_for(&path, &name),
        path,
        name,
        domain,
        added_at: now_secs(),
    };
    store.links.push(link.clone());
    link
}

/// Forgets a link. Touches nothing on disk beyond this registry.
pub fn remove(store: &mut LinkStore, id: &str) -> Result<(), AppError> {
    let before = store.links.len();
    store.links.retain(|link| link.id != id);
    if store.links.len() == before {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_domain_safe() {
        assert_eq!(slugify("ORDO"), "ordo");
        assert_eq!(slugify("My Project"), "my-project");
        assert_eq!(slugify("laravel_api"), "laravel-api");
        assert_eq!(slugify("--weird__name--"), "weird-name");
        assert_eq!(slugify("!!!"), "");
    }

    /// Two folders with the same name in different places must not collide
    /// — the launcher resolves an id straight back to a folder to open.
    #[test]
    fn the_same_folder_name_in_two_places_gets_two_ids() {
        let a = id_for(r"C:\repository\ORDO\api", "api");
        let b = id_for(r"C:\Users\me\rezure\www\api", "api");

        assert_ne!(a, b);
        assert!(a.starts_with("api-"));
        assert!(b.starts_with("api-"));
    }

    /// Derived from the path, not random: re-linking a folder has to land on
    /// the same id so its recorded history comes back with it.
    #[test]
    fn relinking_the_same_folder_reproduces_its_id() {
        let first = id_for(r"C:\repository\ORDO\api", "api");
        // Same folder, differently spelled.
        let again = id_for(r"c:/repository/ORDO/api/", "api");

        assert_eq!(first, again);
    }

    #[test]
    fn a_folder_with_no_usable_name_still_gets_an_id() {
        let id = id_for(r"C:\weird\!!!", "!!!");
        assert!(id.starts_with("project-"), "got {id}");
    }

    #[test]
    fn paths_compare_ignoring_case_and_slash_direction() {
        assert!(same_path(r"C:\repository\ORDO", "c:/repository/ordo/"));
        assert!(!same_path(r"C:\repository\ORDO", r"C:\repository\OTHER"));
    }

    #[test]
    fn a_path_inside_www_is_recognized_as_inside() {
        let www = Path::new(r"C:\Users\me\rezure\www");

        assert!(is_inside(r"C:\Users\me\rezure\www\blog", www));
        assert!(is_inside(r"c:/users/me/rezure/www", www));
        assert!(!is_inside(r"C:\repository\ORDO", www));
        // A sibling that merely shares a prefix is not inside it.
        assert!(!is_inside(r"C:\Users\me\rezure\www-other", www));
    }

    #[test]
    fn a_linked_path_is_found_however_it_is_spelled() {
        let mut store = LinkStore::default();
        add(
            &mut store,
            r"C:\repository\ORDO".to_string(),
            "ordo".to_string(),
            "ordo.test".to_string(),
        );

        assert!(store.linked_at("c:/repository/ordo/").is_some());
        assert!(store.linked_at(r"C:\repository\OTHER").is_none());
    }

    #[test]
    fn removing_an_unknown_link_is_an_error_not_a_silent_no_op() {
        let mut store = LinkStore::default();
        assert!(matches!(
            remove(&mut store, "nope"),
            Err(AppError::ProjectNotFound(_))
        ));
    }

    #[test]
    fn save_then_load_round_trips() {
        let path =
            std::env::temp_dir().join(format!("rezure-test-links-{}.json", std::process::id()));
        let mut store = LinkStore::default();
        let added = add(
            &mut store,
            r"C:\repository\ORDO\project".to_string(),
            "ordo".to_string(),
            "ordo.test".to_string(),
        );

        save_to(&path, &store).unwrap();
        let loaded = load_from(&path);

        assert_eq!(loaded.links.len(), 1);
        assert_eq!(loaded.links[0].id, added.id);
        assert_eq!(loaded.links[0].domain, "ordo.test");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_instead_of_panicking() {
        let path = std::env::temp_dir().join(format!(
            "rezure-test-links-corrupt-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "{ not json").unwrap();

        assert!(load_from(&path).links.is_empty());
        std::fs::remove_file(&path).unwrap();
    }
}
