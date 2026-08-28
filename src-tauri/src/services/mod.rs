//! Core logic: process management, port scanning, service lifecycle.
//!
//! Every service (Apache/Nginx, MySQL, PHP-FPM, ...) implements the [`Service`]
//! trait so adding a new one never requires special-casing elsewhere.

mod mock;

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::utils::error::AppError;

pub use mock::seed_services;

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
pub struct ServiceInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub status: ServiceStatus,
    pub version: String,
    pub port: u16,
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
        }
    }
}

impl Service for MockService {
    fn info(&self) -> ServiceInfo {
        ServiceInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            category: self.category.clone(),
            status: *self.status.lock().unwrap(),
            version: self.version.clone(),
            port: self.port,
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
