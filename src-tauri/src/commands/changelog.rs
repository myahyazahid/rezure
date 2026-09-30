//! Thin glue between the Changelog page and `services::changelog` /
//! `services::upgrade_notice` / `config::changelog_state`.

use tauri::AppHandle;

use crate::config::changelog_state;
use crate::services::changelog::{self, ChangelogEntry};
use crate::services::upgrade_notice::{self, UpgradeNotice};
use crate::utils::error::AppError;

#[tauri::command]
pub async fn fetch_changelog() -> Vec<ChangelogEntry> {
    changelog::fetch().await
}

/// "A newer major is out" for this install's own version — `None` when
/// there's nothing to announce or the API couldn't be reached.
#[tauri::command]
pub async fn fetch_upgrade_notice(app: AppHandle) -> Option<UpgradeNotice> {
    upgrade_notice::fetch(&app.package_info().version.to_string()).await
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
