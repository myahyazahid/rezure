use std::sync::Arc;

use super::{MockService, Service, ServiceManager, ServiceStatus};

/// Seed data mirroring `docs/UI-design` — swapped for real bundled
/// services once Phase 2's binary bundling work lands.
pub fn seed_services() -> ServiceManager {
    let services: Vec<Arc<dyn Service>> = vec![
        Arc::new(MockService::new(
            "nginx",
            "Nginx",
            "Web server",
            "v1.25.3",
            80,
            ServiceStatus::Running,
        )),
        Arc::new(MockService::new(
            "apache",
            "Apache",
            "Web server",
            "v2.4.58",
            8080,
            ServiceStatus::Stopped,
        )),
        Arc::new(MockService::new(
            "mysql",
            "MySQL",
            "Database",
            "v8.0.35",
            3306,
            ServiceStatus::Running,
        )),
        Arc::new(MockService::new(
            "redis",
            "Redis",
            "Cache",
            "v7.2.3",
            6379,
            ServiceStatus::Running,
        )),
        Arc::new(MockService::new(
            "phpmyadmin",
            "phpMyAdmin",
            "Admin tool",
            "v5.2.1",
            8081,
            ServiceStatus::Stopped,
        )),
    ];

    ServiceManager::new(services)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_services_has_expected_initial_statuses() {
        let list = seed_services().list();
        let status_of = |id: &str| list.iter().find(|s| s.id == id).unwrap().status;

        assert_eq!(status_of("nginx"), ServiceStatus::Running);
        assert_eq!(status_of("apache"), ServiceStatus::Stopped);
        assert_eq!(status_of("mysql"), ServiceStatus::Running);
        assert_eq!(status_of("redis"), ServiceStatus::Running);
        assert_eq!(status_of("phpmyadmin"), ServiceStatus::Stopped);
    }
}
