//! How each `proxy` project's dev server is run: the port nginx passes its
//! requests to, and what to start on it.
//!
//! Keyed by project id, like `config::project_frameworks`, and for the same
//! reasons a file in `etc\` rather than a database row: `scan_projects` and
//! the vhost sync read it without a SQLite connection, and a port is a
//! declaration — if `rezure.db` were reset, `astro.test` must not quietly
//! move to another port.
//!
//! A separate file from the framework overrides on purpose. Those only exist
//! for a project whose framework the user corrected, and going back to Auto
//! deletes the entry; a dev server's port is needed by every `proxy`
//! project, detected right or not, and must survive a framework change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::services::frameworks::PortMode;
use crate::utils::error::AppError;
use crate::utils::paths;

fn default_auto_start() -> bool {
    true
}

/// One project's dev server settings. Only `port` is always there — Rezure
/// picks one the first time the project needs it; everything else falls
/// back to the framework's defaults when left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevServerConfig {
    pub port: u16,
    /// Overrides the framework's `dev.port`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_mode: Option<PortMode>,
    /// Overrides the framework's `dev.script`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Whether Open starts the dev server when it isn't running.
    #[serde(default = "default_auto_start")]
    pub auto_start: bool,
}

impl DevServerConfig {
    /// A fresh entry: just the port, everything else the framework's.
    pub fn with_port(port: u16) -> Self {
        Self {
            port,
            port_mode: None,
            script: None,
            auto_start: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevServerStore {
    pub projects: BTreeMap<String, DevServerConfig>,
}

impl DevServerStore {
    pub fn get(&self, project_id: &str) -> Option<&DevServerConfig> {
        self.projects.get(project_id)
    }

    /// Sets, or with `None` clears, a project's entry. Returns whether
    /// anything changed, so a caller can skip writing a file for nothing.
    pub fn set(&mut self, project_id: &str, config: Option<DevServerConfig>) -> bool {
        match config {
            Some(config) => {
                self.projects.insert(project_id.to_string(), config.clone()) != Some(config)
            }
            None => self.projects.remove(project_id).is_some(),
        }
    }

    /// Which project, if any, already has `port`.
    pub fn owner_of(&self, port: u16) -> Option<&str> {
        self.projects
            .iter()
            .find(|(_, config)| config.port == port)
            .map(|(id, _)| id.as_str())
    }
}

/// Read entry by entry, so one hand-edited mistake costs that one project
/// its settings rather than every project theirs.
#[derive(Deserialize)]
struct OnDisk {
    #[serde(default)]
    projects: BTreeMap<String, Value>,
}

#[derive(Serialize)]
struct OnDiskRef<'a> {
    projects: &'a BTreeMap<String, DevServerConfig>,
}

/// `C:\rezure\etc\dev-servers.json`.
pub fn path() -> Result<PathBuf, AppError> {
    Ok(paths::etc()?.join("dev-servers.json"))
}

/// Reads every entry, falling back to none if the file is missing or
/// unreadable — a corrupt file must not stop the project list from loading.
pub fn load() -> DevServerStore {
    let Ok(path) = path() else {
        return DevServerStore::default();
    };
    load_from(&path)
}

fn load_from(path: &Path) -> DevServerStore {
    let Ok(content) = std::fs::read_to_string(path) else {
        return DevServerStore::default();
    };
    // Notepad saves with a byte-order mark, which JSON parsers refuse.
    let on_disk: OnDisk = match serde_json::from_str(content.trim_start_matches('\u{feff}')) {
        Ok(on_disk) => on_disk,
        Err(err) => {
            log::warn!(
                "dev server settings at {} are unreadable, ignoring them: {err}",
                path.display()
            );
            return DevServerStore::default();
        }
    };

    let mut store = DevServerStore::default();
    for (project_id, raw) in on_disk.projects {
        match serde_json::from_value::<DevServerConfig>(raw) {
            Ok(config) => {
                store.projects.insert(project_id, config);
            }
            Err(err) => log::warn!(
                "ignoring the dev server settings for {project_id} in {}: {err}",
                path.display()
            ),
        }
    }
    store
}

pub fn save(store: &DevServerStore) -> Result<(), AppError> {
    save_to(&path()?, store)
}

fn save_to(path: &Path, store: &DevServerStore) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Settings(format!("could not create {}: {e}", parent.display()))
        })?;
    }
    let json = serde_json::to_string_pretty(&OnDiskRef {
        projects: &store.projects,
    })
    .map_err(|e| AppError::Settings(format!("could not serialize dev server settings: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| AppError::Settings(format!("could not write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rezure-test-dev-servers-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_defaults_out() {
        let path = temp_file("round-trip");
        let mut store = DevServerStore::default();
        store.set("astro-web", Some(DevServerConfig::with_port(4400)));
        store.set(
            "9router-a1b2c3",
            Some(DevServerConfig {
                port: 20128,
                port_mode: Some(PortMode::Fixed),
                script: Some("start".to_string()),
                auto_start: false,
            }),
        );

        save_to(&path, &store).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains(r#""portMode": "fixed""#), "{written}");
        assert!(written.contains(r#""autoStart": false"#), "{written}");
        assert_eq!(load_from(&path), store);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_minimal_entry_starts_on_open_by_default() {
        let path = temp_file("minimal");
        std::fs::write(
            &path,
            r#"{ "projects": { "astro-web": { "port": 4400 } } }"#,
        )
        .unwrap();
        let store = load_from(&path);
        assert_eq!(
            store.get("astro-web"),
            Some(&DevServerConfig::with_port(4400))
        );
        assert!(store.get("astro-web").unwrap().auto_start);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_instead_of_panicking() {
        let path = temp_file("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path), DevServerStore::default());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_bad_entry_is_skipped_and_the_rest_kept() {
        let path = temp_file("bad-entry");
        std::fs::write(
            &path,
            "\u{feff}{ \"projects\": {\n\
               \"good\": { \"port\": 4400 },\n\
               \"no-port\": { \"script\": \"dev\" },\n\
               \"typo\": { \"port\": 4401, \"prot\": 1 },\n\
               \"too-big\": { \"port\": 70000 }\n\
             } }",
        )
        .unwrap();

        let store = load_from(&path);
        assert_eq!(store.projects.len(), 1, "{store:?}");
        assert_eq!(store.get("good").unwrap().port, 4400);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn set_reports_whether_anything_changed_and_owner_finds_a_port() {
        let mut store = DevServerStore::default();
        assert!(store.set("a", Some(DevServerConfig::with_port(4400))));
        assert!(!store.set("a", Some(DevServerConfig::with_port(4400))));
        assert_eq!(store.owner_of(4400), Some("a"));
        assert_eq!(store.owner_of(4401), None);
        assert!(store.set("a", None));
        assert!(!store.set("a", None));
    }
}
