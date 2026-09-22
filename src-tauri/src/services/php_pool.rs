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
//!
//! Port assignment is deliberately *sticky* (see [`assign_ports`]): once a
//! version has a port, it keeps it for the life of the app, even as other
//! versions get pinned or unpinned around it. An earlier version of this
//! recomputed every version's port from scratch on every sync, positional
//! by sorted order — which meant pinning a *third* project could silently
//! shift the port an *already-running* second project's `php-cgi` was
//! serving on, desyncing its vhost (which always reflects the freshly
//! computed port) from the process still bound to the old one. Nginx then
//! proxied to a port nothing was listening on: a 502 for a project nobody
//! had touched. `ServiceManager::sync_php_pool` is what actually calls
//! [`assign_ports`], since it alone knows which port an already-registered
//! pooled service is really bound to.

use std::collections::{BTreeMap, BTreeSet};

use crate::db::projects::ProjectInfo;

/// The first port a pooled (non-default) version gets. `vhosts::PHP_FASTCGI_PORT`
/// (9000) is always reserved for the default "php" service, so the pool
/// starts one above it.
pub const BASE_PORT: u16 = 9001;

/// Every PHP version currently pinned by at least one project — distinct
/// from `global_active` (already served by the default "php" service) and
/// only among versions actually `installed`; a project pinned to a version
/// that's since been removed is treated as unset rather than kept as a dead
/// pool entry. Just the *set* of versions that need a pooled instance, not
/// their ports — see [`assign_ports`] for why port assignment is a separate
/// step.
pub fn wanted_versions(
    projects: &[ProjectInfo],
    installed: &BTreeSet<String>,
    global_active: &str,
) -> BTreeSet<String> {
    projects
        .iter()
        .filter_map(|p| p.php_version.as_deref())
        .filter(|version| *version != global_active && installed.contains(*version))
        .map(|version| version.to_string())
        .collect()
}

/// Assigns a port to every version in `wanted`: reuses `existing`'s port for
/// a version that already has one (an already-running pooled service keeps
/// exactly the port it's bound to, no matter what else changes), and picks
/// the lowest port from [`BASE_PORT`] not already claimed by *anything* in
/// `existing` — including an entry that's no longer in `wanted` — for one
/// that doesn't.
///
/// That last part is deliberate, not an oversight: a version dropped from
/// `wanted` simply isn't in the *result*, but its port stays reserved for
/// as long as `existing` still lists it. In practice `existing` is a live
/// snapshot `ServiceManager::sync_php_pool` takes *before* it stops and
/// unregisters anything stale, so a version's port only actually becomes
/// available again on a later call, once that stale entry is truly gone —
/// never within the same call a version drops out, which is what keeps a
/// freshly wanted version from ever being asked to bind a port the
/// just-stopped process might not have released yet.
///
/// Pure and side-effect-free on purpose: the actual "is this version already
/// registered" question depends on live `ServiceManager` state, so that
/// lookup happens in `ServiceManager::sync_php_pool`, which calls this with
/// what it already knows — keeping the assignment rule itself trivially
/// unit-testable without a real service manager.
pub fn assign_ports(
    wanted: &BTreeSet<String>,
    existing: &BTreeMap<String, u16>,
) -> BTreeMap<String, u16> {
    let mut taken: BTreeSet<u16> = existing.values().copied().collect();
    let mut result = BTreeMap::new();

    for version in wanted {
        let port = match existing.get(version) {
            Some(&port) => port,
            None => {
                let mut candidate = BASE_PORT;
                while taken.contains(&candidate) {
                    candidate += 1;
                }
                taken.insert(candidate);
                candidate
            }
        };
        result.insert(version.clone(), port);
    }
    result
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
            node_version: None,
        }
    }

    fn installed(versions: &[&str]) -> BTreeSet<String> {
        versions.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn a_project_with_no_override_needs_no_pool_entry() {
        let projects = [project("a", None)];
        let wanted = wanted_versions(&projects, &installed(&["7.4.33", "8.3.0"]), "7.4.33");
        assert!(wanted.is_empty());
    }

    #[test]
    fn pinning_the_global_default_needs_no_extra_process() {
        // Explicitly pinning the version that's already the default is
        // redundant, not a distinct instance — the default "php" service
        // already serves it.
        let projects = [project("a", Some("7.4.33"))];
        let wanted = wanted_versions(&projects, &installed(&["7.4.33", "8.3.0"]), "7.4.33");
        assert!(wanted.is_empty());
    }

    #[test]
    fn three_projects_on_three_distinct_versions_all_get_pool_entries() {
        let projects = [
            project("a", None), // follows the global default, 7.4.33
            project("b", Some("8.0.30")),
            project("c", Some("8.5.0")),
        ];
        let wanted = wanted_versions(
            &projects,
            &installed(&["7.4.33", "8.0.30", "8.5.0"]),
            "7.4.33",
        );

        assert_eq!(
            wanted,
            BTreeSet::from(["8.0.30".to_string(), "8.5.0".to_string()])
        );
    }

    #[test]
    fn two_projects_pinning_the_same_version_share_one_pool_entry() {
        let projects = [project("a", Some("8.0.30")), project("b", Some("8.0.30"))];
        let wanted = wanted_versions(&projects, &installed(&["7.4.33", "8.0.30"]), "7.4.33");
        assert_eq!(wanted.len(), 1);
    }

    #[test]
    fn a_pin_to_an_uninstalled_version_is_treated_as_unset() {
        let projects = [project("a", Some("8.9.9"))];
        let wanted = wanted_versions(&projects, &installed(&["7.4.33"]), "7.4.33");
        assert!(wanted.is_empty());
    }

    #[test]
    fn wanted_versions_is_stable_regardless_of_project_order() {
        let forward = [project("a", Some("8.0.30")), project("b", Some("8.5.0"))];
        let backward = [project("b", Some("8.5.0")), project("a", Some("8.0.30"))];
        let versions = installed(&["7.4.33", "8.0.30", "8.5.0"]);

        assert_eq!(
            wanted_versions(&forward, &versions, "7.4.33"),
            wanted_versions(&backward, &versions, "7.4.33")
        );
    }

    #[test]
    fn a_brand_new_version_gets_the_lowest_free_port() {
        let wanted = BTreeSet::from(["8.0.30".to_string()]);
        let assignment = assign_ports(&wanted, &BTreeMap::new());
        assert_eq!(assignment["8.0.30"], BASE_PORT);
    }

    #[test]
    fn a_version_already_registered_keeps_its_existing_port_untouched() {
        // The regression this guards: a second, unrelated version being
        // added must never change a port already handed out.
        let existing = BTreeMap::from([("8.0.30".to_string(), BASE_PORT + 5)]);
        let wanted = BTreeSet::from(["8.0.30".to_string(), "8.5.0".to_string()]);

        let assignment = assign_ports(&wanted, &existing);

        assert_eq!(assignment["8.0.30"], BASE_PORT + 5, "must not have moved");
        assert_ne!(
            assignment["8.5.0"],
            BASE_PORT + 5,
            "the new version must not collide with the kept port"
        );
    }

    /// The exact scenario that produced the real bug: three versions get
    /// pinned one at a time. Each already-running version's port must stay
    /// fixed as later versions join, however the *sorted* order of the full
    /// set would otherwise reindex them.
    #[test]
    fn pinning_a_third_version_never_moves_the_first_twos_ports() {
        let after_first = assign_ports(&BTreeSet::from(["8.5.0".to_string()]), &BTreeMap::new());
        let first_port = after_first["8.5.0"];

        let after_second = assign_ports(
            &BTreeSet::from(["7.4.33".to_string(), "8.5.0".to_string()]),
            &after_first,
        );
        assert_eq!(
            after_second["8.5.0"], first_port,
            "adding a version that sorts before it must not move it"
        );

        let after_third = assign_ports(
            &BTreeSet::from([
                "7.4.33".to_string(),
                "8.0.30".to_string(),
                "8.5.0".to_string(),
            ]),
            &after_second,
        );
        assert_eq!(
            after_third["8.5.0"], first_port,
            "a third version joining must still not move it"
        );
        assert_eq!(
            after_third["7.4.33"], after_second["7.4.33"],
            "the second version must keep its own port too"
        );
    }

    /// While `existing` still lists a version that dropped out of `wanted`
    /// (the state `ServiceManager` hands in *before* it has actually
    /// stopped and removed that stale entry), a freshly wanted version must
    /// not be handed its port — the old process may not have let go of it
    /// yet.
    #[test]
    fn a_stale_versions_port_stays_reserved_while_existing_still_lists_it() {
        let existing = BTreeMap::from([("8.0.30".to_string(), BASE_PORT)]);
        let wanted = BTreeSet::from(["8.5.0".to_string()]); // 8.0.30 dropped

        let assignment = assign_ports(&wanted, &existing);

        assert_eq!(assignment.len(), 1);
        assert_ne!(
            assignment["8.5.0"], BASE_PORT,
            "must not reuse a port existing still claims for another (stale) version"
        );
    }

    /// Once the stale entry is actually gone from `existing` — the state on
    /// a *later* call, after `ServiceManager` has stopped and unregistered
    /// it — its port really is available again.
    #[test]
    fn a_port_is_reused_once_the_stale_entry_is_truly_gone() {
        let wanted = BTreeSet::from(["8.5.0".to_string()]);
        let assignment = assign_ports(&wanted, &BTreeMap::new());
        assert_eq!(assignment["8.5.0"], BASE_PORT);
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
