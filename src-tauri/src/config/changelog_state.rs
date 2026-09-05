//! Tracks which changelog entry the user has already seen, so the sidebar
//! badge clears once they've opened the page — same shape as
//! `config::device`: a tiny JSON file under `paths::etc()`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::utils::error::AppError;
use crate::utils::paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogState {
    pub last_seen_version: Option<String>,
}

fn path() -> Result<PathBuf, AppError> {
    Ok(paths::etc()?.join("changelog_state.json"))
}

pub fn load() -> ChangelogState {
    let path = match path() {
        Ok(path) => path,
        Err(err) => {
            log::warn!("could not resolve changelog state path, using defaults: {err}");
            return ChangelogState::default();
        }
    };
    load_from(&path)
}

fn load_from(path: &Path) -> ChangelogState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn save(state: &ChangelogState) -> Result<(), AppError> {
    save_to(&path()?, state)
}

fn save_to(path: &Path, state: &ChangelogState) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Io(format!("could not create {}: {e}", parent.display())))?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| AppError::Io(format!("could not serialize changelog state: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rezure-test-changelog-state-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_falls_back_to_none() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert_eq!(load_from(&path).last_seen_version, None);
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = temp_path("roundtrip");
        save_to(
            &path,
            &ChangelogState {
                last_seen_version: Some("1.4.0".to_string()),
            },
        )
        .unwrap();
        assert_eq!(load_from(&path).last_seen_version.as_deref(), Some("1.4.0"));
        std::fs::remove_file(&path).unwrap();
    }
}
