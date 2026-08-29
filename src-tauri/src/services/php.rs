//! PHP version switcher.
//!
//! Backed by a fixed list until portable binaries are bundled — the public
//! surface (`list` / `set_active`) is what the real implementation will keep.

use std::sync::Mutex;

use serde::Serialize;

use crate::utils::error::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct PhpVersion {
    pub id: String,
    pub version: String,
    pub active: bool,
}

/// Tauri-managed state tracking which installed PHP version is selected.
pub struct PhpVersionManager {
    installed: Vec<String>,
    active: Mutex<String>,
}

impl PhpVersionManager {
    pub fn list(&self) -> Vec<PhpVersion> {
        let active = self.active.lock().unwrap().clone();

        self.installed
            .iter()
            .map(|version| PhpVersion {
                id: version.clone(),
                version: version.clone(),
                active: *version == active,
            })
            .collect()
    }

    pub fn set_active(&self, id: &str) -> Result<Vec<PhpVersion>, AppError> {
        if !self.installed.iter().any(|v| v == id) {
            return Err(AppError::PhpVersionNotFound(id.to_string()));
        }

        *self.active.lock().unwrap() = id.to_string();
        Ok(self.list())
    }
}

/// Seed data mirroring `docs/UI-design` — replaced by a real scan of the
/// installed PHP binaries once bundling lands.
pub fn seed_php_versions() -> PhpVersionManager {
    let installed = vec![
        "8.3.2".to_string(),
        "8.2.15".to_string(),
        "8.1.27".to_string(),
        "8.0.30".to_string(),
        "7.4.33".to_string(),
    ];

    PhpVersionManager {
        active: Mutex::new(installed[0].clone()),
        installed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_version_is_active_by_default() {
        let list = seed_php_versions().list();

        assert_eq!(list.len(), 5);
        assert!(list[0].active);
        assert_eq!(list.iter().filter(|v| v.active).count(), 1);
    }

    #[test]
    fn set_active_moves_the_flag_to_the_requested_version() {
        let manager = seed_php_versions();
        let list = manager.set_active("8.1.27").unwrap();

        let active: Vec<_> = list.iter().filter(|v| v.active).map(|v| &v.id).collect();
        assert_eq!(active, vec!["8.1.27"]);
    }

    #[test]
    fn set_active_rejects_an_unknown_version() {
        let manager = seed_php_versions();

        assert!(manager.set_active("5.6.0").is_err());
        // The previous selection must survive a failed switch.
        assert!(manager.list()[0].active);
    }
}
