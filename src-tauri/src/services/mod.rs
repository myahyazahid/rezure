//! Core logic: process management, port scanning, service lifecycle.
//!
//! Every service (Apache/Nginx, MySQL, PHP-FPM, ...) implements the [`Service`]
//! trait so adding a new one never requires special-casing elsewhere.

pub mod binaries;
mod mock;
pub mod php;

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::utils::error::AppError;

pub use mock::seed_services;

/// Number of samples kept for a service's CPU sparkline.
const CPU_HISTORY_LEN: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Running,
    Stopped,
    // Reserved for real (non-mock) services with an async spawn/shutdown step.
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

/// Shared abstraction every service (real or mock) implements.
/// Adding a new service type means implementing this trait, not
/// special-casing it elsewhere in the codebase.
pub trait Service: Send + Sync {
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
            .find(|s| s.info().id == id)
            .cloned()
            .ok_or_else(|| AppError::ServiceNotFound(id.to_string()))
    }
}

/// In-memory service backed by no real process — stands in for a real
/// `Service` implementation (spawned process + port check) in Phase 2's
/// UI/architecture pass, before portable binaries are bundled.
pub struct MockService {
    id: String,
    name: String,
    category: String,
    version: String,
    port: u16,
    status: Mutex<ServiceStatus>,
    cpu_history: Vec<u8>,
}

impl MockService {
    pub fn new(
        id: &str,
        name: &str,
        category: &str,
        version: &str,
        port: u16,
        status: ServiceStatus,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            category: category.to_string(),
            version: version.to_string(),
            port,
            status: Mutex::new(status),
            cpu_history: mock_cpu_history(id),
        }
    }
}

/// Deterministic pseudo-random walk so each mock service gets a stable,
/// plausible-looking CPU curve instead of a flat line.
fn mock_cpu_history(seed_source: &str) -> Vec<u8> {
    let mut state = seed_source
        .bytes()
        .fold(1u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let mut value = 35i32;

    (0..CPU_HISTORY_LEN)
        .map(|_| {
            // Linear congruential generator (Numerical Recipes constants).
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let delta = ((state >> 16) % 15) as i32 - 7;
            value = (value + delta).clamp(12, 68);
            value as u8
        })
        .collect()
}

impl Service for MockService {
    fn info(&self) -> ServiceInfo {
        let status = *self.status.lock().unwrap();
        let running = status == ServiceStatus::Running;

        ServiceInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            category: self.category.clone(),
            status,
            version: self.version.clone(),
            port: self.port,
            cpu_percent: running.then(|| self.cpu_history.last().copied().unwrap_or(0)),
            cpu_history: if running {
                self.cpu_history.clone()
            } else {
                Vec::new()
            },
        }
    }

    fn start(&self) -> Result<ServiceInfo, AppError> {
        *self.status.lock().unwrap() = ServiceStatus::Running;
        Ok(self.info())
    }

    fn stop(&self) -> Result<ServiceInfo, AppError> {
        *self.status.lock().unwrap() = ServiceStatus::Stopped;
        Ok(self.info())
    }
}
