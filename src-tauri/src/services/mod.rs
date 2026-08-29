//! Core logic: process management, port scanning, service lifecycle.
//!
//! Every service (Apache/Nginx, MySQL, PHP-FPM, ...) implements the [`Service`]
//! trait so adding a new one never requires special-casing elsewhere.

pub mod binaries;
pub mod hosts;
pub mod php;
pub mod php_ini;
pub mod process;
pub mod projects;
pub mod scaffold;
pub mod vhosts;

use std::sync::Arc;

use serde::Serialize;

use crate::utils::error::AppError;

pub use process::real_services;

/// Number of samples kept for a service's CPU sparkline.
const CPU_HISTORY_LEN: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Running,
    Stopped,
    // Reserved for a future async spawn/shutdown step (e.g. waiting on a
    // service's own readiness probe before reporting it as running).
    #[allow(dead_code)]
    Starting,
    #[allow(dead_code)]
    Stopping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub status: ServiceStatus,
    pub version: String,
    pub port: u16,
    /// Current CPU usage, only reported while the service is running.
    pub cpu_percent: Option<u8>,
    /// Recent CPU samples driving the UI sparkline; empty while stopped.
    pub cpu_history: Vec<u8>,
}

/// Shared abstraction every service implements. Adding a new service type
/// means implementing this trait, not special-casing it elsewhere in the
/// codebase.
pub trait Service: Send + Sync {
    /// Stable identifier (matches the frontend's `ServiceInfo.id`) — cheap
    /// to call, unlike `info()`, so `ServiceManager::find` doesn't need to
    /// touch the process/CPU-sampling machinery just to match by id.
    fn id(&self) -> &str;
    fn info(&self) -> ServiceInfo;
    fn start(&self) -> Result<ServiceInfo, AppError>;
    fn stop(&self) -> Result<ServiceInfo, AppError>;
    fn restart(&self) -> Result<ServiceInfo, AppError> {
        self.stop()?;
        self.start()
    }
}

pub type ServiceHandle = Arc<dyn Service>;

/// Tauri-managed state holding every registered service.
pub struct ServiceManager {
    services: Vec<ServiceHandle>,
}

impl ServiceManager {
    pub fn new(services: Vec<ServiceHandle>) -> Self {
        Self { services }
    }

    pub fn list(&self) -> Vec<ServiceInfo> {
        self.services.iter().map(|s| s.info()).collect()
    }

    pub fn find(&self, id: &str) -> Result<ServiceHandle, AppError> {
        self.services
            .iter()
            .find(|s| s.id() == id)
            .cloned()
            .ok_or_else(|| AppError::ServiceNotFound(id.to_string()))
    }
}
