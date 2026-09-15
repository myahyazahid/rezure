//! Lets projects on different PHP versions run at the same time.
//!
//! There's no PHP-FPM on Windows (see `services::vhosts`), so each *version*
//! that's actually in use needs its own `php-cgi -b 127.0.0.1:PORT`
//! responder — one process can only ever be one version. The globally
//! active version (`services::php::active_id`) already gets one: the
//! original, singular "php" service on the fixed `vhosts::PHP_FASTCGI_PORT`.
//! This module is what lets a project additionally *pin* a different
//! version, distinct from that global default, giving it its own responder
//! on its own port instead.
//!
//! A project that never pins one keeps following the global default exactly
//! as before — this only changes anything for a project that explicitly
//! asks for a specific version, which is what keeps the common single-version
//! case identical to the pre-existing behavior.

use std::collections::{BTreeMap, BTreeSet};

use crate::db::projects::ProjectInfo;

/// The first port a pooled (non-default) version gets. `vhosts::PHP_FASTCGI_PORT`
/// (9000) is always reserved for the default "php" service, so the pool
/// starts one above it.
pub const BASE_PORT: u16 = 9001;

/// Every PHP version currently pinned by at least one project, distinct from
/// `global_active` (which is already served by the default "php" service) and
/// only among versions actually `installed` — a project can point at a
/// version that's since been removed, and that override is treated as
/// unset rather than spun up as a dead pool entry.
///
/// Ports are assigned by sorted version string, not by scan or insertion
/// order, so the assignment only changes when the *set* of pinned versions
/// changes — not when projects happen to get scanned in a different order.
pub fn wanted(
    projects: &[ProjectInfo],
    installed: &BTreeSet<String>,
    global_active: &str,
) -> BTreeMap<String, u16> {
    let pinned: BTreeSet<&str> = projects
        .iter()
        .filter_map(|p| p.php_version.as_deref())
        .filter(|version| *version != global_active && installed.contains(*version))
        .collect();

    pinned
        .into_iter()
        .enumerate()
        .map(|(index, version)| (version.to_string(), BASE_PORT + index as u16))
        .collect()
}

/// The FastCGI port a project's vhost should actually proxy to: its own
/// pinned version's pool port when it has a valid one, `default_port`
/// (the global default "php" service's port) otherwise — no override, or
/// one that isn't in `pool` (unpinned, or pinned to a version no longer
/// installed).
pub fn port_for_project(
    project_version: Option<&str>,
    pool: &BTreeMap<String, u16>,
    default_port: u16,
) -> u16 {
    project_version
        .and_then(|version| pool.get(version))
        .copied()
        .unwrap_or(default_port)
}

/// A pooled service's id, from the version it's pinned to — the inverse of
/// stripping the `"php-"` prefix, kept in one place so the two can never
/// drift apart.
pub fn service_id(version: &str) -> String {
    format!("php-{version}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::projects::ProjectKind;

    fn project(id: &str, php_version: Option<&str>) -> ProjectInfo {
        ProjectInfo {
            id: id.to_string(),
            name: id.to_string(),
            path: format!("C:/rezure/www/{id}"),
            domain: format!("{id}.test"),
            stack: "PHP".to_string(),
            has_hosts_entry: false,
            last_opened_at: None,
            open_count: 0,
            kind: ProjectKind::Scanned,
            missing: false,
            domain_invalid: false,
            php_version: php_version.map(|v| v.to_string()),
        }
    }

    fn installed(versions: &[&str]) -> BTreeSet<String> {
        versions.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn a_project_with_no_override_needs_no_pool_entry() {
        let projects = [project("a", None)];
        let pool = wanted(&projects, &installed(&["7.4.33", "8.3.0"]), "7.4.33");
        assert!(pool.is_empty());
    }

    #[test]
    fn pinning_the_global_default_needs_no_extra_process() {
        // Explicitly pinning the version that's already the default is
        // redundant, not a distinct instance — the default "php" service
        // already serves it.
        let projects = [project("a", Some("7.4.33"))];
        let pool = wanted(&projects, &installed(&["7.4.33", "8.3.0"]), "7.4.33");
        assert!(pool.is_empty());
    }

    #[test]
    fn three_projects_on_three_distinct_versions_all_get_pool_entries() {
        let projects = [
            project("a", None), // follows the global default, 7.4.33
            project("b", Some("8.0.30")),
            project("c", Some("8.5.0")),
        ];
        let pool = wanted(
            &projects,
            &installed(&["7.4.33", "8.0.30", "8.5.0"]),
            "7.4.33",
        );

        assert_eq!(pool.len(), 2);
        assert_eq!(pool["8.0.30"], BASE_PORT);
        assert_eq!(pool["8.5.0"], BASE_PORT + 1);
    }

    #[test]
    fn two_projects_pinning_the_same_version_share_one_pool_entry() {
        let projects = [project("a", Some("8.0.30")), project("b", Some("8.0.30"))];
        let pool = wanted(&projects, &installed(&["7.4.33", "8.0.30"]), "7.4.33");
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn a_pin_to_an_uninstalled_version_is_treated_as_unset() {
        let projects = [project("a", Some("8.9.9"))];
        let pool = wanted(&projects, &installed(&["7.4.33"]), "7.4.33");
        assert!(pool.is_empty());
    }

    #[test]
    fn port_assignment_is_stable_regardless_of_project_order() {
        let forward = [project("a", Some("8.0.30")), project("b", Some("8.5.0"))];
        let backward = [project("b", Some("8.5.0")), project("a", Some("8.0.30"))];
        let versions = installed(&["7.4.33", "8.0.30", "8.5.0"]);

        assert_eq!(
            wanted(&forward, &versions, "7.4.33"),
            wanted(&backward, &versions, "7.4.33")
        );
    }

    #[test]
    fn port_for_project_falls_back_to_the_default_when_unpinned() {
        let pool = BTreeMap::from([("8.0.30".to_string(), BASE_PORT)]);
        assert_eq!(port_for_project(None, &pool, 9000), 9000);
    }

    #[test]
    fn port_for_project_uses_the_pool_port_when_pinned_and_present() {
        let pool = BTreeMap::from([("8.0.30".to_string(), BASE_PORT)]);
        assert_eq!(port_for_project(Some("8.0.30"), &pool, 9000), BASE_PORT);
    }

    #[test]
    fn port_for_project_falls_back_when_pinned_to_something_not_in_the_pool() {
        let pool = BTreeMap::from([("8.0.30".to_string(), BASE_PORT)]);
        assert_eq!(port_for_project(Some("7.4.33"), &pool, 9000), 9000);
    }
}
