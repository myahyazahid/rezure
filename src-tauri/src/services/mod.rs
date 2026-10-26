//! Core logic: process management, port scanning, service lifecycle.
//!
//! Every service (Apache/Nginx, MySQL, PHP-FPM, ...) implements the [`Service`]
//! trait so adding a new one never requires special-casing elsewhere.

pub mod agent_docs;
pub mod binaries;
pub mod ca_bundle;
pub mod changelog;
pub mod composer;
pub mod composer_catalog;
pub mod connections;
pub mod database;
pub mod db_clients;
pub mod db_engine;
pub mod db_profiles;
pub mod dev_servers;
pub mod doctor;
pub mod donate;
pub mod donate_images;
pub mod frameworks;
pub mod hosts;
pub mod launcher;
pub mod localdb_bridge;
pub mod mariadb_catalog;
pub mod msi;
pub mod mssql;
pub mod mssql_discovery;
pub mod mssql_localdb;
pub mod node;
pub mod node_catalog;
pub mod odbc;
pub mod php;
pub mod php_catalog;
pub mod php_ext;
pub mod php_ext_toggle;
pub mod php_ini;
pub mod php_path;
pub mod php_pool;
pub mod ports;
pub mod postgres;
pub mod postgres_catalog;
pub mod postgres_client;
pub mod process;
pub mod projects;
pub mod scaffold;
pub mod secrets;
pub mod service_visibility;
pub mod share;
pub mod share_proxy;
pub mod sticker_catalog;
pub mod sticker_library;
pub mod supervisor;
pub mod support;
pub mod telemetry;
pub mod tunnel;
pub mod upgrade_notice;
pub mod vhosts;

use std::collections::{BTreeMap, BTreeSet, HashSet};
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
    /// Spawned, but a database still waiting to accept connections.
    Starting,
    /// Asked to shut down and not gone yet.
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
    /// The port the service binds — for a service with several workers, the
    /// first of them (see [`ServiceInfo::workers`]).
    pub port: u16,
    /// Current CPU usage, only reported while the service is running.
    pub cpu_percent: Option<u8>,
    /// Recent CPU samples driving the UI sparkline; empty while stopped.
    pub cpu_history: Vec<u8>,
    /// Set for a service that runs as several identical processes on
    /// consecutive ports starting at `port` (PHP — see `php_pool`), `None`
    /// for a single-process one.
    pub workers: Option<WorkerCount>,
    /// Every port the service binds while running — each worker's, plus any
    /// second listener (Mailpit's web UI). What a "port in use" failure has
    /// to be traced against, since the conflict can be on any of them.
    pub ports: Vec<u16>,
    /// Whether the binary this service runs is on disk. A service that
    /// isn't can't be started, and "Start all" skips it rather than failing.
    pub installed: bool,
    /// The `binaries::MANIFEST` package that installs this service when it
    /// isn't installed yet, for services one download makes startable. `None`
    /// where installing is a choice made elsewhere (PHP and database
    /// versions, on the Switch and Databases pages).
    pub install_id: Option<String>,
    /// A web UI the service serves, to open in the browser while it runs.
    pub web_url: Option<String>,
    /// How clients reach a service that listens on no TCP port — SQL Server
    /// LocalDB answers on a named pipe, addressed as `(localdb)\Rezure`.
    /// Shown in place of the port; `None` for every port-bound service.
    pub endpoint: Option<String>,
}

/// How many of a multi-process service's workers are up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerCount {
    pub running: u16,
    pub total: u16,
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

    /// True once the service — or, for one with several workers, any one of
    /// them — has exited on its own — neither `stop()` nor `force_stop()`
    /// took it down — and stays true until the next successful `start()` or
    /// any `stop()`. A `start()` on a service with only some workers down
    /// brings back just those, leaving the rest serving. Checking it may itself be what
    /// notices the exit, so implementations should poll their process here
    /// rather than wait for `info()` to be called. Defaults to never.
    fn crashed(&self) -> bool {
        false
    }

    /// Whether [`supervisor`] may start this service again by itself after a
    /// crash. Opt-in, and off by default: only right for a service that
    /// holds no state a blind restart could make worse — `php-cgi`, not a
    /// database mid-crash-recovery.
    fn restarts_on_crash(&self) -> bool {
        false
    }
}

pub type ServiceHandle = Arc<dyn Service>;

/// Builds a pooled PHP service handle for a specific version, listening on a
/// specific port block (its first port) — see [`ServiceManager::sync_php_pool`]. A closure rather
/// than a direct dependency on `services::process::ProcessService` so this
/// module doesn't need to know about any concrete `Service` implementation;
/// `process::real_services` is the only place that supplies one.
pub type PhpPoolFactory = Arc<dyn Fn(&str, u16) -> ServiceHandle + Send + Sync>;

/// Builds a project's dev server from its spec — see
/// [`ServiceManager::sync_dev_servers`]. A closure for the same reason as
/// [`PhpPoolFactory`].
pub type DevServerFactory = Arc<dyn Fn(&dev_servers::DevSpec) -> ServiceHandle + Send + Sync>;

/// Tauri-managed state holding every registered service.
///
/// The list is no longer fixed at construction: a project pinning a PHP
/// version distinct from the global default needs its own pooled service to
/// appear (and disappear again once nothing pins it anymore) while the app
/// is running — see [`sync_php_pool`](Self::sync_php_pool). Nginx, the
/// default PHP service and the database are registered once at startup and
/// never removed; pooled `"php-<version>"` entries and projects' dev servers
/// (`"dev-<project>"`, see [`sync_dev_servers`](Self::sync_dev_servers)) come
/// and go.
pub struct ServiceManager {
    services: Mutex<Vec<ServiceHandle>>,
    php_factory: Option<PhpPoolFactory>,
    dev_factory: Option<DevServerFactory>,
    /// The spec each registered dev server was built from, to tell a changed
    /// one from an unchanged one without asking the service.
    dev_specs: Mutex<BTreeMap<String, dev_servers::DevSpec>>,
}

impl ServiceManager {
    pub fn new(services: Vec<ServiceHandle>) -> Self {
        Self {
            services: Mutex::new(services),
            php_factory: None,
            dev_factory: None,
            dev_specs: Mutex::new(BTreeMap::new()),
        }
    }

    /// Attaches the factory [`sync_dev_servers`](Self::sync_dev_servers)
    /// builds a project's dev server with.
    pub fn with_dev_factory(mut self, factory: DevServerFactory) -> Self {
        self.dev_factory = Some(factory);
        self
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

    /// A snapshot of every registered service, for callers that act on each
    /// one without holding the list's lock for the duration (a start can
    /// take real time — see [`supervisor`]).
    pub fn handles(&self) -> Vec<ServiceHandle> {
        self.services.lock().unwrap().clone()
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

    /// Reconciles the pooled PHP services against `wanted` (the versions at
    /// least one project currently pins, from
    /// [`php_pool::wanted_versions`](php_pool::wanted_versions)) and returns
    /// the version -> port-block assignment that resulted (each block's
    /// first port, see [`php_pool::worker_ports`]).
    ///
    /// This is the **one place** a pooled version's port is decided — not
    /// `services::vhosts`, even though it's the one writing `fastcgi_pass`
    /// lines. Only `ServiceManager` actually knows which port an
    /// already-registered pooled service is bound to, so only it can keep
    /// [`php_pool::assign_ports`] from moving a still-wanted version's port
    /// out from under its already-running process — which is exactly what
    /// happened when port assignment used to be computed independently,
    /// position-in-a-sorted-set style, wherever a vhost needed one: pinning
    /// a *third* project could silently reindex a *second*, already-running
    /// one's port, leaving its vhost pointing at a port nothing was
    /// listening on (a 502 for a project nobody had touched). Callers must
    /// use the returned map — not recompute their own — for exactly that
    /// reason.
    ///
    /// Registers a pooled instance for every version newly in `wanted`, and
    /// stops and unregisters any pooled instance no project pins anymore —
    /// it does not start a newly registered instance itself, staying
    /// consistent with every other service here being manually started.
    ///
    /// A no-op (empty result) if no factory was attached (every test
    /// `ServiceManager` and, in principle, any future headless use).
    pub fn sync_php_pool(&self, wanted: &BTreeSet<String>) -> BTreeMap<String, u16> {
        let Some(factory) = &self.php_factory else {
            return BTreeMap::new();
        };

        // What's already registered and the live port each is actually
        // bound to — the only source `assign_ports` is allowed to keep a
        // still-wanted version's port from.
        let existing: BTreeMap<String, u16> = {
            let services = self.services.lock().unwrap();
            services
                .iter()
                .filter_map(|s| {
                    s.id()
                        .strip_prefix("php-")
                        .map(|version| (version.to_string(), s.info().port))
                })
                .collect()
        };
        let assignment = php_pool::assign_ports(wanted, &existing);

        // Stopping a process can take a moment (see `ProcessService::stop`),
        // so it happens with the lock released rather than held for the
        // duration — nothing else needs to observe the half-removed state.
        let stale: Vec<ServiceHandle> = {
            let mut services = self.services.lock().unwrap();
            let mut stale = Vec::new();
            services.retain(|service| {
                let Some(version) = service.id().strip_prefix("php-") else {
                    return true;
                };
                if wanted.contains(version) {
                    true
                } else {
                    stale.push(service.clone());
                    false
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
        let already_registered: HashSet<String> =
            services.iter().map(|s| s.id().to_string()).collect();
        for (version, port) in &assignment {
            let id = php_pool::service_id(version);
            if !already_registered.contains(&id) {
                services.push(factory(version, *port));
            }
        }

        assignment
    }

    /// Reconciles the registered dev servers against `specs` — one per
    /// `proxy` project, from [`dev_servers::specs`].
    ///
    /// A project that's gone, or no longer `proxy`, has its dev server
    /// stopped and unregistered. One whose spec changed (another port,
    /// script, Node.js version) is rebuilt — but only while it's stopped: a
    /// running dev server keeps the spec it was started with until it stops,
    /// and the next sync after that picks the change up. A new one is
    /// registered and, like every other service here, left stopped.
    ///
    /// A no-op if no factory was attached.
    pub fn sync_dev_servers(&self, specs: &[dev_servers::DevSpec]) {
        let Some(factory) = &self.dev_factory else {
            return;
        };
        let wanted: BTreeMap<String, &dev_servers::DevSpec> = specs
            .iter()
            .map(|spec| (dev_servers::service_id(&spec.project_id), spec))
            .collect();

        let stale: Vec<ServiceHandle> = {
            let mut services = self.services.lock().unwrap();
            let mut stale = Vec::new();
            services.retain(|service| {
                if !dev_servers::is_dev_service(service.id()) || wanted.contains_key(service.id()) {
                    true
                } else {
                    stale.push(service.clone());
                    false
                }
            });
            stale
        };
        {
            let mut known = self.dev_specs.lock().unwrap();
            known.retain(|id, _| wanted.contains_key(id));
        }
        for service in stale {
            if let Err(err) = service.stop() {
                log::warn!(
                    "failed to stop {} while removing its dev server: {err}",
                    service.id()
                );
            }
        }

        let mut services = self.services.lock().unwrap();
        let mut known = self.dev_specs.lock().unwrap();
        for (id, spec) in wanted {
            if known.get(&id) == Some(spec) {
                continue;
            }
            match services.iter().position(|service| service.id() == id) {
                Some(index) => {
                    if services[index].info().status != ServiceStatus::Stopped {
                        continue;
                    }
                    services[index] = factory(spec);
                }
                None => services.push(factory(spec)),
            }
            known.insert(id, spec.clone());
        }
    }

    /// The spec a project's registered dev server was built from.
    pub fn dev_spec(&self, project_id: &str) -> Option<dev_servers::DevSpec> {
        self.dev_specs
            .lock()
            .unwrap()
            .get(&dev_servers::service_id(project_id))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::frameworks::PortMode;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// A service that records what was done to it.
    struct Fake {
        id: String,
        port: u16,
        running: AtomicBool,
        stops: Arc<AtomicUsize>,
    }

    impl Service for Fake {
        fn id(&self) -> &str {
            &self.id
        }
        fn info(&self) -> ServiceInfo {
            ServiceInfo {
                id: self.id.clone(),
                name: self.id.clone(),
                category: "Test".to_string(),
                status: if self.running.load(Ordering::SeqCst) {
                    ServiceStatus::Running
                } else {
                    ServiceStatus::Stopped
                },
                version: String::new(),
                port: self.port,
                cpu_percent: None,
                cpu_history: Vec::new(),
                workers: None,
                ports: vec![self.port],
                installed: true,
                install_id: None,
                web_url: None,
                endpoint: None,
            }
        }
        fn start(&self) -> Result<ServiceInfo, AppError> {
            self.running.store(true, Ordering::SeqCst);
            Ok(self.info())
        }
        fn stop(&self) -> Result<ServiceInfo, AppError> {
            self.running.store(false, Ordering::SeqCst);
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(self.info())
        }
    }

    fn spec(project: &str, port: u16) -> dev_servers::DevSpec {
        dev_servers::DevSpec {
            project_id: project.to_string(),
            name: project.to_string(),
            domain: format!("{project}.test"),
            framework: "Astro".to_string(),
            dir: PathBuf::from(format!(r"C:\nowhere\{project}")),
            port,
            port_mode: PortMode::Flag,
            port_mode_setting: None,
            default_port_mode: PortMode::Flag,
            script: Some("dev".to_string()),
            script_setting: None,
            scripts: vec!["dev".to_string()],
            package_manager: dev_servers::PackageManager::Npm,
            node_pin: None,
            auto_start: true,
        }
    }

    fn manager(stops: &Arc<AtomicUsize>) -> ServiceManager {
        let nginx: ServiceHandle = Arc::new(Fake {
            id: "nginx".to_string(),
            port: 80,
            running: AtomicBool::new(true),
            stops: Arc::clone(stops),
        });
        let stops = Arc::clone(stops);
        ServiceManager::new(vec![nginx]).with_dev_factory(Arc::new(move |spec| {
            Arc::new(Fake {
                id: dev_servers::service_id(&spec.project_id),
                port: spec.port,
                running: AtomicBool::new(false),
                stops: Arc::clone(&stops),
            }) as ServiceHandle
        }))
    }

    fn ids(manager: &ServiceManager) -> Vec<String> {
        manager
            .handles()
            .iter()
            .map(|s| s.id().to_string())
            .collect()
    }

    #[test]
    fn dev_servers_come_and_go_with_their_projects() {
        let stops = Arc::new(AtomicUsize::new(0));
        let manager = manager(&stops);

        manager.sync_dev_servers(&[spec("a", 4400), spec("b", 4401)]);
        assert_eq!(ids(&manager), ["nginx", "dev-a", "dev-b"]);

        // "b" is gone (unlinked, or no longer proxied): stopped and dropped.
        manager.find("dev-b").unwrap().start().unwrap();
        manager.sync_dev_servers(&[spec("a", 4400)]);
        assert_eq!(ids(&manager), ["nginx", "dev-a"]);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
        // Rezure's own services are never touched.
        assert_eq!(
            manager.find("nginx").unwrap().info().status,
            ServiceStatus::Running
        );
    }

    #[test]
    fn a_changed_spec_waits_for_a_running_dev_server_to_stop() {
        let stops = Arc::new(AtomicUsize::new(0));
        let manager = manager(&stops);
        manager.sync_dev_servers(&[spec("a", 4400)]);
        let running = manager.find("dev-a").unwrap();
        running.start().unwrap();

        // Moved to another port while running: it keeps the one it has.
        manager.sync_dev_servers(&[spec("a", 4410)]);
        assert_eq!(manager.find("dev-a").unwrap().info().port, 4400);
        assert_eq!(manager.dev_spec("a").unwrap().port, 4400);

        // Once stopped, the next sync rebuilds it with the new one.
        running.stop().unwrap();
        manager.sync_dev_servers(&[spec("a", 4410)]);
        assert_eq!(manager.find("dev-a").unwrap().info().port, 4410);
        assert_eq!(manager.dev_spec("a").unwrap().port, 4410);
        assert_eq!(ids(&manager), ["nginx", "dev-a"]);
    }

    #[test]
    fn without_a_factory_nothing_is_registered() {
        let manager = ServiceManager::new(Vec::new());
        manager.sync_dev_servers(&[spec("a", 4400)]);
        assert!(manager.handles().is_empty());
    }
}
