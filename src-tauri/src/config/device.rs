//! This device's stable identifier — a client-generated UUID, created once and
//! persisted for the life of the install. It's both the identifier and the
//! auth model for the telemetry/support API (see the API contract doc); a
//! changed `device_id` looks like a brand-new install to the backend, so it
//! must never be regenerated once written.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::utils::error::AppError;
use crate::utils::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceConfig {
    device_id: String,
}

/// `%APPDATA%\Rezure\device.json` (via `paths::etc()`).
fn path() -> Result<PathBuf, AppError> {
    Ok(paths::etc()?.join("device.json"))
}

/// Reads the persisted device id, generating and saving a new one on first
/// run (or if the file is missing/corrupt — losing this file is equivalent
/// to a fresh install, not a fatal error).
pub fn load() -> String {
    let path = match path() {
        Ok(path) => path,
        Err(err) => {
            log::warn!("could not resolve device id path, generating an unsaved one: {err}");
            return Uuid::new_v4().to_string();
        }
    };
    load_from(&path)
}

fn load_from(path: &Path) -> String {
    if let Some(existing) = std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<DeviceConfig>(&content).ok())
    {
        return existing.device_id;
    }

    let device_id = Uuid::new_v4().to_string();
    if let Err(err) = save_to(path, &device_id) {
        log::warn!("could not persist a new device id: {err}");
    }
    device_id
}

fn save_to(path: &Path, device_id: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Io(format!("could not create {}: {e}", parent.display())))?;
    }
    let json = serde_json::to_string_pretty(&DeviceConfig {
        device_id: device_id.to_string(),
    })
    .map_err(|e| AppError::Io(format!("could not serialize device id: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))
}

/// Tauri-managed state — a plain immutable value, unlike `SettingsState`,
/// since the device id never changes once generated.
pub struct DeviceIdState(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rezure-test-device-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_generates_and_persists_a_new_id() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);

        let id = load_from(&path);
        assert!(Uuid::parse_str(&id).is_ok());

        let reloaded = load_from(&path);
        assert_eq!(id, reloaded, "a second load must return the same id");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn corrupt_file_falls_back_to_a_freshly_generated_id() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not json").unwrap();

        let id = load_from(&path);
        assert!(Uuid::parse_str(&id).is_ok());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn existing_file_is_returned_unchanged() {
        let path = temp_path("existing");
        save_to(&path, "3fa85f64-5717-4562-b3fc-2c963f66afa6").unwrap();

        let id = load_from(&path);
        assert_eq!(id, "3fa85f64-5717-4562-b3fc-2c963f66afa6");
        std::fs::remove_file(&path).unwrap();
    }
}
