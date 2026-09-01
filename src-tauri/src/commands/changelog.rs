//! Thin glue between the Changelog page and `services::changelog` /
//! `config::changelog_state`.

use crate::config::changelog_state;
use crate::services::changelog::{self, ChangelogEntry};
use crate::utils::error::AppError;

#[tauri::command]
pub async fn fetch_changelog() -> Vec<ChangelogEntry> {
    changelog::fetch().await
}

#[tauri::command]
pub fn last_seen_changelog_version() -> Option<String> {
    changelog_state::load().last_seen_version
}

#[tauri::command]
pub fn mark_changelog_seen(version: String) -> Result<(), AppError> {
    changelog_state::save(&changelog_state::ChangelogState {
        last_seen_version: Some(version),
    })
}
