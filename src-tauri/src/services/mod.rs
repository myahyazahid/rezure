//! Core logic: process management, port scanning, service lifecycle.
//!
//! Every service (Apache/Nginx, MySQL, PHP-FPM, ...) implements the [`Service`]
//! trait so adding a new one never requires special-casing elsewhere.

pub mod binaries;
pub mod changelog;
pub mod connections;
pub mod database;
pub mod db_clients;
pub mod db_engine;
pub mod db_profiles;
pub mod doctor;
pub mod donate;
pub mod hosts;
pub mod launcher;
pub mod php;
pub mod php_catalog;
pub mod php_ext;
pub mod php_ini;
pub mod php_path;
pub mod php_pool;
pub mod ports;
pub mod process;
pub mod projects;
pub mod scaffold;
pub mod secrets;
pub mod share;
pub mod share_proxy;
pub mod support;
pub mod telemetry;
pub mod tunnel;
pub mod vhosts;

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

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

    /// Kills the process immediately, skipping whatever clean shutdown
    /// [`Service::stop`] would attempt.
    ///
    /// The escape hatch for a service that won't stop on its own — a hung
    /// database can otherwise hold the UI for the length of its shutdown
    /// timeout, with no way out. Defaults to a plain `stop` for services
    /// whose shutdown is already immediate; only implementations that wait
    /// for something need to override it.
    fn force_stop(&self) -> Result<ServiceInfo, AppError> {
        self.stop()
    }
}

pub type ServiceHandle = Arc<dyn Service>;

/// Builds a pooled PHP service handle for a specific version, listening on a
/// specific port — see [`ServiceManager::sync_php_pool`]. A closure rather
/// than a direct dependency on `services::process::ProcessService` so this
/// module doesn't need to know about any concrete `Service` implementation;
/// `process::real_services` is the only place that supplies one.
pub type PhpPoolFactory = Arc<dyn Fn(&str, u16) -> ServiceHandle + Send + Sync>;

/// Tauri-managed state holding every registered service.
///
/// The list is no longer fixed at construction: a project pinning a PHP
/// version distinct from the global default needs its own pooled service to
/// appear (and disappear again once nothing pins it anymore) while the app
/// is running — see [`sync_php_pool`](Self::sync_php_pool). Nginx, the
/// default PHP service and the database are registered once at startup and
/// never removed; only pooled `"php-<version>"` entries come and go.
pub struct ServiceManager {
    services: Mutex<Vec<ServiceHandle>>,
    php_factory: Option<PhpPoolFactory>,
}

impl ServiceManager {
    pub fn new(services: Vec<ServiceHandle>) -> Self {
        Self {
            services: Mutex::new(services),
            php_factory: None,
        }
    }

    /// Attaches the factory [`sync_php_pool`](Self::sync_php_pool) uses to
    /// build a pooled PHP service on demand. Separate from `new` so the
    /// sixteen-odd existing test call sites that construct a `ServiceManager`
    /// without caring about the pool don't all need updating.
    pub fn with_php_factory(mut self, factory: PhpPoolFactory) -> Self {
        self.php_factory = Some(factory);
        self
    }

    pub fn list(&self) -> Vec<ServiceInfo> {
        self.services
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.info())
            .collect()
    }

    pub fn find(&self, id: &str) -> Result<ServiceHandle, AppError> {
        self.services
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id() == id)
            .cloned()
            .ok_or_else(|| AppError::ServiceNotFound(id.to_string()))
    }

    /// Stops every running service — best-effort, on a normal app exit, so
    /// a closed Rezure window never leaves a service running as an orphan
    /// the next launch can't see (see `process::pid_file_path`). A failure
    /// stopping one service must not skip the rest, so errors are logged
    /// rather than propagated.
    pub fn stop_all(&self) {
        for service in self.services.lock().unwrap().iter() {
            if let Err(err) = service.stop() {
                log::warn!("failed to stop {} on exit: {err}", service.id());
            }
        }
    }

    /// Reconciles the pooled PHP services against `wanted` (version -> port,
    /// from [`php_pool::wanted`](php_pool::wanted)): registers a pooled
    /// instance for every version that doesn't have one yet, and stops and
    /// unregisters any pooled instance no project pins anymore.
    ///
    /// Called after every vhost sync (see `commands::projects`), so a
    /// project's override taking effect or being cleared shows up as a
    /// startable/removed service card immediately — it does not start the
    /// service itself, staying consistent with every other service here
    /// being manually started.
    ///
    /// A no-op if no factory was attached (every test `ServiceManager` and,
    /// in principle, any future headless use).
    pub fn sync_php_pool(&self, wanted: &BTreeMap<String, u16>) {
        let Some(factory) = &self.php_factory else {
            return;
        };

        // Stopping a process can take a moment (see `ProcessService::stop`),
        // so it happens with the lock released rather than held for the
        // duration — nothing else needs to observe the half-removed state.
        let stale: Vec<ServiceHandle> = {
            let mut services = self.services.lock().unwrap();
            let wanted_ids: HashSet<String> =
                wanted.keys().map(|v| php_pool::service_id(v)).collect();
            let mut stale = Vec::new();
            services.retain(|service| {
                let id = service.id();
                if id.starts_with("php-") && !wanted_ids.contains(id) {
                    stale.push(service.clone());
                    false
                } else {
                    true
                }
            });
            stale
        };
        for service in stale {
            if let Err(err) = service.stop() {
                log::warn!(
                    "failed to stop {} while removing it from the PHP pool: {err}",
                    service.id()
                );
            }
        }

        let mut services = self.services.lock().unwrap();
        let existing: HashSet<String> = services.iter().map(|s| s.id().to_string()).collect();
        for (version, port) in wanted {
            let id = php_pool::service_id(version);
            if !existing.contains(&id) {
                services.push(factory(version, *port));
            }
        }
    }
}
