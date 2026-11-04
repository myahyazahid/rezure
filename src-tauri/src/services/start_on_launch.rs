//! Services the user wants running as soon as Rezure opens.
//!
//! Rezure starts nothing by itself, on purpose: a database holds hundreds of
//! MB, and most sessions need only some of what's installed. But some things
//! outside Rezure expect a service to be there from the start — an app
//! installed as a Windows service (Odoo) connects to PostgreSQL the moment
//! Windows boots, a queue worker in a terminal expects Redis. Ticking a
//! service here, with Start with Windows on, covers that. Per service rather
//! than one switch for all, since it's usually one database that's needed.

use crate::config::settings::{self, SettingsState};
use crate::services::{dev_servers, service_visibility, ServiceManager, ServiceStatus};
use crate::utils::error::AppError;

/// Marks `id` to start, or not, when Rezure opens. Only the ids the
/// Services page lists by themselves: a pooled PHP version starts with PHP,
/// and a dev server only ever for its project.
pub fn set(
    manager: &ServiceManager,
    state: &SettingsState,
    id: &str,
    on: bool,
) -> Result<settings::Settings, AppError> {
    if service_visibility::group_of(id) != id
        || dev_servers::is_dev_service(id)
        || manager.find(id).is_err()
    {
        return Err(AppError::ServiceNotFound(id.to_string()));
    }

    let mut current = state.0.lock().unwrap();
    let mut next = current.clone();
    next.start_on_launch.retain(|s| s != id);
    if on {
        next.start_on_launch.push(id.to_string());
    }
    settings::save(&next)?;
    *current = next.clone();
    Ok(next)
}

/// Starts every service marked to, skipping any taken off the Services page,
/// not installed, or already running (a previous session's orphan is
/// `start`'s to deal with). A failure is logged and doesn't stop the rest —
/// the Services page shows each one's state either way.
pub fn start_marked(manager: &ServiceManager, marked: &[String], hidden: &[String]) {
    for id in marked {
        if hidden.contains(id) {
            continue;
        }
        let Ok(service) = manager.find(id) else {
            log::warn!("{id} is set to start with Rezure but no longer exists");
            continue;
        };
        let info = service.info();
        if !info.installed || info.status != ServiceStatus::Stopped {
            continue;
        }
        match service.start() {
            Ok(_) => log::info!("started {id} on launch"),
            Err(err) => log::warn!("could not start {id} on launch: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{Service, ServiceHandle, ServiceInfo};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Fake {
        id: &'static str,
        installed: bool,
        running: AtomicBool,
        starts: Arc<AtomicUsize>,
    }

    impl Service for Fake {
        fn id(&self) -> &str {
            self.id
        }
        fn info(&self) -> ServiceInfo {
            ServiceInfo {
                id: self.id.to_string(),
                name: self.id.to_string(),
                category: "Test".to_string(),
                status: if self.running.load(Ordering::SeqCst) {
                    ServiceStatus::Running
                } else {
                    ServiceStatus::Stopped
                },
                version: String::new(),
                port: 0,
                cpu_percent: None,
                cpu_history: Vec::new(),
                workers: None,
                ports: Vec::new(),
                installed: self.installed,
                install_id: None,
                web_url: None,
                endpoint: None,
            }
        }
        fn start(&self) -> Result<ServiceInfo, AppError> {
            self.running.store(true, Ordering::SeqCst);
            self.starts.fetch_add(1, Ordering::SeqCst);
            Ok(self.info())
        }
        fn stop(&self) -> Result<ServiceInfo, AppError> {
            self.running.store(false, Ordering::SeqCst);
            Ok(self.info())
        }
    }

    fn fake(
        id: &'static str,
        installed: bool,
        running: bool,
        starts: &Arc<AtomicUsize>,
    ) -> ServiceHandle {
        Arc::new(Fake {
            id,
            installed,
            running: AtomicBool::new(running),
            starts: Arc::clone(starts),
        })
    }

    #[test]
    fn only_marked_installed_shown_and_stopped_services_are_started() {
        let starts = Arc::new(AtomicUsize::new(0));
        let manager = ServiceManager::new(vec![
            fake("postgres", true, false, &starts),
            fake("mariadb", true, false, &starts),
            fake("mailpit", false, false, &starts),
            fake("nginx", true, true, &starts),
            fake("sqlserver", true, false, &starts),
        ]);
        let marked: Vec<String> = ["postgres", "mailpit", "nginx", "sqlserver", "gone"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        start_marked(&manager, &marked, &["sqlserver".to_string()]);

        // postgres only: mariadb isn't marked, mailpit isn't installed, nginx
        // already runs, sqlserver is hidden, and "gone" doesn't exist.
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            manager.find("postgres").unwrap().info().status,
            ServiceStatus::Running
        );
        assert_eq!(
            manager.find("mariadb").unwrap().info().status,
            ServiceStatus::Stopped
        );
    }
}
