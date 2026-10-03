//! Which services the Services page shows — the Manage services choice.
//!
//! Rezure registers every service it knows (`process::real_services`), but
//! not every user needs every one: someone on MySQL has no use for a SQL
//! Server card, and someone on SQL Server has none for MariaDB's. Removing a
//! service here takes it off the Services page, and with it out of Start
//! all, Restart all and Stop all, which act on what the page lists. Nothing
//! is uninstalled and no data is touched; adding it back restores the card
//! as it was.
//!
//! The choice is stored as the ids the user removed
//! ([`Settings::hidden_services`](crate::config::settings::Settings::hidden_services)),
//! not the ones they kept, so a service a later Rezure adds shows up instead
//! of staying invisible until someone thinks to look for it here.

use serde::Serialize;

use super::{ServiceInfo, ServiceManager, ServiceStatus};
use crate::config::settings::{self, SettingsState};
use crate::utils::error::AppError;

/// The services project sites are served through. Removing one is allowed —
/// someone may only want Rezure for its databases — but the Manage services
/// list says what it costs.
const SERVES_SITES: &[&str] = &["nginx", "php"];

/// One row of the Manage services list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedService {
    pub id: String,
    pub name: String,
    pub category: String,
    /// On the Services page.
    pub shown: bool,
    pub installed: bool,
    /// Running, or for PHP any of its pooled versions is. Removing the
    /// service stops it first.
    pub running: bool,
    /// Project sites stop loading without it.
    pub serves_sites: bool,
}

/// The id a service is shown or hidden under. Pooled PHP instances
/// (`php-8.3.0`, see `php_pool`) come and go as projects pin versions, so
/// they aren't listed on their own: they follow PHP.
pub fn group_of(id: &str) -> &str {
    if id.starts_with("php-") {
        "php"
    } else {
        id
    }
}

fn is_hidden(hidden: &[String], id: &str) -> bool {
    let group = group_of(id);
    hidden.iter().any(|h| h == group)
}

/// The ids the user has removed, as currently in memory.
pub fn hidden(state: &SettingsState) -> Vec<String> {
    state.0.lock().unwrap().hidden_services.clone()
}

/// What the Services page lists: every service that hasn't been removed.
///
/// A removed service that is running anyway stays listed until it stops.
/// Removing one stops it, but something else can start it again — the
/// project requirements check starts Mailpit — and a running service with no
/// row would hold its port with no way to stop it from the page.
pub fn visible(services: Vec<ServiceInfo>, hidden: &[String]) -> Vec<ServiceInfo> {
    services
        .into_iter()
        .filter(|s| !is_hidden(hidden, &s.id) || s.status != ServiceStatus::Stopped)
        .collect()
}

/// The Manage services list: one row per registered service, in the
/// Services page's order, with pooled PHP versions folded into PHP.
pub fn catalog(services: &[ServiceInfo], hidden: &[String]) -> Vec<ManagedService> {
    services
        .iter()
        .filter(|s| group_of(&s.id) == s.id)
        .map(|s| ManagedService {
            id: s.id.clone(),
            name: s.name.clone(),
            category: s.category.clone(),
            shown: !is_hidden(hidden, &s.id),
            installed: s.installed,
            running: services
                .iter()
                .any(|other| group_of(&other.id) == s.id && other.status != ServiceStatus::Stopped),
            serves_sites: SERVES_SITES.contains(&s.id.as_str()),
        })
        .collect()
}

/// Puts a service back on the Services page, or takes it off.
///
/// Taking it off stops it first — PHP's pooled versions included — so a
/// removed service never keeps running out of sight. A failed stop leaves
/// it listed and is returned as the error.
pub fn set_shown(
    manager: &ServiceManager,
    state: &SettingsState,
    id: &str,
    shown: bool,
) -> Result<(), AppError> {
    let handles = manager.handles();
    // Only the ids the Manage services list offers: a pooled PHP version is
    // shown or hidden with PHP, never by itself.
    if group_of(id) != id || !handles.iter().any(|s| s.id() == id) {
        return Err(AppError::ServiceNotFound(id.to_string()));
    }

    if !shown {
        for service in handles.iter().filter(|s| group_of(s.id()) == id) {
            if service.info().status != ServiceStatus::Stopped {
                service.stop()?;
            }
        }
    }

    let mut current = state.0.lock().unwrap();
    let mut next = current.clone();
    next.hidden_services.retain(|h| h != id);
    if !shown {
        next.hidden_services.push(id.to_string());
    }
    settings::save(&next)?;
    *current = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, status: ServiceStatus) -> ServiceInfo {
        ServiceInfo {
            id: id.to_string(),
            name: id.to_uppercase(),
            category: "Test".to_string(),
            status,
            version: String::new(),
            port: 0,
            cpu_percent: None,
            cpu_history: Vec::new(),
            workers: None,
            ports: Vec::new(),
            installed: true,
            install_id: None,
            web_url: None,
            endpoint: None,
        }
    }

    fn removed(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn ids(services: &[ServiceInfo]) -> Vec<&str> {
        services.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn removed_services_leave_the_page() {
        let services = vec![
            info("nginx", ServiceStatus::Stopped),
            info("mariadb", ServiceStatus::Stopped),
            info("sqlserver", ServiceStatus::Stopped),
        ];
        let shown = visible(services, &removed(&["sqlserver"]));
        assert_eq!(ids(&shown), ["nginx", "mariadb"]);
    }

    #[test]
    fn pooled_php_versions_follow_php() {
        let services = vec![
            info("php", ServiceStatus::Stopped),
            info("php-8.2.0", ServiceStatus::Stopped),
            info("mariadb", ServiceStatus::Stopped),
        ];
        let shown = visible(services.clone(), &removed(&["php"]));
        assert_eq!(ids(&shown), ["mariadb"]);

        // The list offers PHP once, not once per pinned version.
        let rows = catalog(&services, &[]);
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["php", "mariadb"]
        );
    }

    /// Something other than the Services page can start a removed service;
    /// it must stay reachable until it stops again.
    #[test]
    fn a_removed_service_that_is_running_stays_listed() {
        let services = vec![
            info("mailpit", ServiceStatus::Running),
            info("sqlserver", ServiceStatus::Stopped),
        ];
        let shown = visible(services, &removed(&["mailpit", "sqlserver"]));
        assert_eq!(ids(&shown), ["mailpit"]);
    }

    #[test]
    fn php_reads_as_running_while_a_pooled_version_is() {
        let services = vec![
            info("php", ServiceStatus::Stopped),
            info("php-8.2.0", ServiceStatus::Running),
            info("nginx", ServiceStatus::Stopped),
        ];
        let rows = catalog(&services, &removed(&["nginx"]));
        let php = &rows[0];
        assert!(php.running && php.shown && php.serves_sites);
        let nginx = &rows[1];
        assert!(!nginx.running && !nginx.shown && nginx.serves_sites);
    }

    #[test]
    fn only_listed_services_can_be_removed() {
        let manager = ServiceManager::new(Vec::new());
        let state = SettingsState::new(Default::default());
        for id in ["nope", "php-8.2.0"] {
            assert!(matches!(
                set_shown(&manager, &state, id, false),
                Err(AppError::ServiceNotFound(_))
            ));
        }
        assert!(hidden(&state).is_empty());
    }
}
