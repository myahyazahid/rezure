//! The framework a user chose for a project, overriding what was detected.
//!
//! Keyed by project id and shared by scanned and linked projects alike, so
//! `services::projects::scan_projects` applies every correction in one place
//! and each consumer of the scan (vhosts, the list, the launcher, `AGENTS.md`)
//! gets the corrected result without asking.
//!
//! # Why this is config and not a database row
//!
//! Same reasoning as `config::links`: `scan_projects` is synchronous and has
//! no way to reach the SQLite connection in Tauri's managed state. And a
//! choice like this is a declaration, not history — if `rezure.db` were
//! reset, a project must not quietly go back to being served from the wrong
//! folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::utils::error::AppError;
use crate::utils::paths;

/// What a project is served as instead of what detection says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameworkOverride {
    /// A framework from the registry, by id — its docroot and serve profile
    /// come with it.
    Framework(String),
    /// No framework at all: serve this subfolder of the project ("Custom").
    Docroot(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverrideStore {
    pub overrides: BTreeMap<String, FrameworkOverride>,
}

impl OverrideStore {
    pub fn get(&self, project_id: &str) -> Option<&FrameworkOverride> {
        self.overrides.get(project_id)
    }

    /// Sets, or with `None` clears, a project's override. Returns whether
    /// anything changed, so a caller can skip writing a file for nothing.
    pub fn set(&mut self, project_id: &str, choice: Option<FrameworkOverride>) -> bool {
        match choice {
            Some(choice) => {
                self.overrides
                    .insert(project_id.to_string(), choice.clone())
                    != Some(choice)
            }
            None => self.overrides.remove(project_id).is_some(),
        }
    }
}

/// Read entry by entry, so one hand-edited mistake costs that one project
/// its override rather than every project theirs.
#[derive(Deserialize)]
struct OnDisk {
    #[serde(default)]
    overrides: BTreeMap<String, Value>,
}

#[derive(Serialize)]
struct OnDiskRef<'a> {
    overrides: &'a BTreeMap<String, FrameworkOverride>,
}

/// `C:\rezure\etc\project-frameworks.json`.
pub fn path() -> Result<PathBuf, AppError> {
    Ok(paths::etc()?.join("project-frameworks.json"))
}

/// Reads every override, falling back to none if the file is missing or
/// unreadable — a corrupt file must not stop the project list from loading.
pub fn load() -> OverrideStore {
    let Ok(path) = path() else {
        return OverrideStore::default();
    };
    load_from(&path)
}

fn load_from(path: &Path) -> OverrideStore {
    let Ok(content) = std::fs::read_to_string(path) else {
        return OverrideStore::default();
    };
    // Notepad saves with a byte-order mark, which JSON parsers refuse.
    let on_disk: OnDisk = match serde_json::from_str(content.trim_start_matches('\u{feff}')) {
        Ok(on_disk) => on_disk,
        Err(err) => {
            log::warn!(
                "project framework overrides at {} are unreadable, ignoring them: {err}",
                path.display()
            );
            return OverrideStore::default();
        }
    };

    let mut store = OverrideStore::default();
    for (project_id, raw) in on_disk.overrides {
        match serde_json::from_value::<FrameworkOverride>(raw) {
            Ok(choice) => {
                store.overrides.insert(project_id, choice);
            }
            Err(err) => log::warn!(
                "ignoring the framework override for {project_id} in {}: {err}",
                path.display()
            ),
        }
    }
    store
}

pub fn save(store: &OverrideStore) -> Result<(), AppError> {
    save_to(&path()?, store)
}

fn save_to(path: &Path, store: &OverrideStore) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Settings(format!("could not create {}: {e}", parent.display()))
        })?;
    }
    let json = serde_json::to_string_pretty(&OnDiskRef {
        overrides: &store.overrides,
    })
    .map_err(|e| AppError::Settings(format!("could not serialize framework overrides: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| AppError::Settings(format!("could not write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rezure-test-project-frameworks-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn save_then_load_round_trips_both_kinds() {
        let path = temp_file("round-trip");
        let mut store = OverrideStore::default();
        store.set(
            "melisa-topup-3dff0a",
            Some(FrameworkOverride::Framework("codeigniter4".to_string())),
        );
        store.set(
            "legacy",
            Some(FrameworkOverride::Docroot("htdocs".to_string())),
        );

        save_to(&path, &store).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains(r#""framework": "codeigniter4""#),
            "{written}"
        );
        assert!(written.contains(r#""docroot": "htdocs""#), "{written}");
        assert_eq!(load_from(&path), store);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn set_reports_whether_anything_changed() {
        let mut store = OverrideStore::default();
        let choice = Some(FrameworkOverride::Framework("laravel".to_string()));

        assert!(store.set("blog", choice.clone()));
        assert!(
            !store.set("blog", choice),
            "the same choice again is no change"
        );
        assert!(store.set("blog", None));
        assert!(!store.set("blog", None), "clearing twice is no change");
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_instead_of_panicking() {
        let path = temp_file("corrupt");
        std::fs::write(&path, "{ not json").unwrap();

        assert_eq!(load_from(&path), OverrideStore::default());
        std::fs::remove_file(&path).unwrap();
    }

    /// One hand-edited entry with both keys (or a typo) mustn't cost every
    /// other project its override.
    #[test]
    fn a_bad_entry_is_skipped_and_the_rest_kept() {
        let path = temp_file("bad-entry");
        std::fs::write(
            &path,
            "\u{feff}{ \"overrides\": {\n\
               \"good\": { \"framework\": \"laravel\" },\n\
               \"both\": { \"framework\": \"laravel\", \"docroot\": \"public\" },\n\
               \"typo\": { \"framwork\": \"laravel\" }\n\
             } }",
        )
        .unwrap();

        let store = load_from(&path);
        assert_eq!(store.overrides.len(), 1);
        assert_eq!(
            store.get("good"),
            Some(&FrameworkOverride::Framework("laravel".to_string()))
        );
        std::fs::remove_file(&path).unwrap();
    }
}
