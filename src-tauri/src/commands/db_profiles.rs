//! Thin glue between the Databases page's profile switcher and
//! `services::db_profiles`.
//!
//! The one command here with real logic is [`switch_db_profile`], because a
//! switch is a sequence that can fail halfway and must not leave the user
//! with no database at all.

use tauri::State;

use crate::config::profiles::{Profile, ProfileSource};
use crate::services::db_engine::Engine;
use crate::services::db_profiles::{self, AddProfile, DetectedDatadir, ProfileStatus};
use crate::services::{ServiceManager, ServiceStatus};
use crate::utils::error::AppError;

/// The database service's id in [`ServiceManager`] — see
/// `ProcessService::mariadb`. Still `mariadb` for continuity even though
/// the profile behind it may be MySQL.
const DB_SERVICE: &str = "mariadb";

fn joined(e: tokio::task::JoinError) -> AppError {
    AppError::Database(format!("background task panicked: {e}"))
}

#[tauri::command]
pub fn list_db_profiles() -> Vec<ProfileStatus> {
    db_profiles::list()
}

#[tauri::command]
pub fn active_db_profile() -> Option<Profile> {
    db_profiles::active()
}

/// Datadirs belonging to other tools that aren't profiles yet. Read-only —
/// nothing is registered until the user says so.
#[tauri::command]
pub async fn detect_db_profiles() -> Result<Vec<DetectedDatadir>, AppError> {
    tokio::task::spawn_blocking(db_profiles::detect)
        .await
        .map_err(joined)
}

/// The add-profile form, as one payload — the frontend sends it under a
/// single `request` key.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProfileRequest {
    pub name: String,
    pub datadir_path: String,
    pub engine: Option<Engine>,
    pub version: String,
    pub port: u16,
    pub source: Option<ProfileSource>,
    pub binary_dir: Option<String>,
    pub defaults_file: Option<String>,
}

#[tauri::command]
pub fn add_db_profile(request: AddProfileRequest) -> Result<Vec<ProfileStatus>, AppError> {
    db_profiles::add(AddProfile {
        name: request.name,
        datadir_path: request.datadir_path,
        engine: request.engine,
        version: request.version,
        port: request.port,
        source: request.source.unwrap_or(ProfileSource::Custom),
        binary_dir: request.binary_dir,
        defaults_file: request.defaults_file,
    })?;
    Ok(db_profiles::list())
}

#[tauri::command]
pub fn remove_db_profile(id: String) -> Result<Vec<ProfileStatus>, AppError> {
    db_profiles::remove(&id)?;
    Ok(db_profiles::list())
}

/// What a switch did, beyond changing which profile is active.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub profiles: Vec<ProfileStatus>,
    /// True when the server was running and came back up on the new
    /// profile. False when it was stopped to begin with — the switch still
    /// took, it just takes effect on the next start.
    pub restarted: bool,
}

/// Points the one running server at a different datadir.
///
/// The sequence, and why it's in this order:
///
/// 1. **Gate first.** `check_can_switch_to` refuses a datadir written by the
///    other engine, one with no compatible binary installed, or one another
///    application still has open. All three are checked *before* anything
///    is stopped, so a refused switch costs the user nothing.
/// 2. **Stop cleanly.** `ProcessService::stop` asks the server to shut down
///    and only force-kills if it won't — see its doc comment.
/// 3. **Point and start.** The datadir, port and binary are all re-resolved
///    from the now-active profile at spawn time.
/// 4. **Roll back on failure.** If the new profile won't start, the previous
///    one is made active again and restarted, so a failed switch leaves a
///    working database rather than none. The error says both things.
#[tauri::command]
pub async fn switch_db_profile(
    id: String,
    manager: State<'_, ServiceManager>,
) -> Result<SwitchResult, AppError> {
    let target = db_profiles::list()
        .into_iter()
        .find(|status| status.profile.id == id)
        .ok_or_else(|| AppError::ProfileNotFound(id.clone()))?
        .profile;

    db_profiles::check_can_switch_to(&target)?;

    let previous = db_profiles::active();
    let service = manager.find(DB_SERVICE)?;
    let was_running = service.info().status == ServiceStatus::Running;

    // Every step below blocks — a clean shutdown of a large datadir can take
    // seconds — so the whole sequence goes to a blocking task rather than
    // stalling the UI thread.
    let switched = tokio::task::spawn_blocking(move || {
        if was_running {
            service.stop()?;
        }

        db_profiles::set_active(&id)?;

        if !was_running {
            return Ok(false);
        }

        match service.start() {
            Ok(_) => Ok(true),
            Err(start_err) => {
                // The new profile won't come up. Put the old one back and
                // restart it, so the user isn't left with nothing running.
                let restored = previous.as_ref().map(|p| p.name.clone());
                if let Some(previous) = &previous {
                    let _ = db_profiles::set_active(&previous.id);
                    if let Err(err) = service.start() {
                        log::error!("rollback failed to restart the previous profile: {err}");
                    }
                }
                Err(AppError::SwitchRolledBack {
                    restored: restored.unwrap_or_else(|| "the previous profile".to_string()),
                    reason: start_err.to_string(),
                })
            }
        }
    })
    .await
    .map_err(joined)?;

    Ok(SwitchResult {
        profiles: db_profiles::list(),
        restarted: switched?,
    })
}
