//! Lets projects on different PHP versions run at the same time, and lets
//! each version serve more than one request at a time.
//!
//! There's no PHP-FPM on Windows (see `services::vhosts`), so each *version*
//! that's actually in use needs its own `php-cgi -b 127.0.0.1:PORT`
//! responders — one process can only ever be one version. The globally
//! active version (`services::php::active_id`) already gets them: the
//! original "php" service on the fixed [`DEFAULT_BASE_PORT`] block. This
//! module is what lets a project additionally *pin* a different version,
//! distinct from that global default, giving it its own responders on its
//! own port block instead.
//!
//! # Worker pool per version
//!
//! A Windows `php-cgi` handles exactly one request at a time —
//! `PHP_FCGI_CHILDREN` needs `fork()`, which Windows doesn't have. With one
//! process per version, a single slow request (a heavy query, a big upload,
//! anything running up to `php_ini::MAX_EXECUTION_TIME_SECS`) queued every
//! other request to that version behind it until nginx gave up on them.
//! So every version runs [`WORKERS_PER_VERSION`] processes, one per port,
//! and nginx spreads requests across them through an `upstream` block (see
//! [`upstream_name`] and `vhosts::sync_vhosts`).
//!
//! # Port layout
//!
//! Each version owns a block of [`PORT_BLOCK`] consecutive ports, of which
//! its workers use the first [`WORKERS_PER_VERSION`]: the default service
//! gets `9100–9103`, the first pinned version `9110–9113`, the next
//! `9120–9123`, and so on. The spare room in each block is what lets the
//! worker count grow later without renumbering anything.
//!
//! The range deliberately starts at 9100, not the 9000 the single responder
//! used to sit on: a block starting at 9000 would put a worker on 9003,
//! which is the port Xdebug 3 connects *out* to — the IDE listens there, and
//! a `php-cgi` already holding it would silently break step debugging. 9000
//! itself is also what Laragon's and XAMPP's own PHP responders tend to
//! occupy.
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

/// How many `php-cgi` processes serve each PHP version in use. Four keeps a
/// couple of slow requests from stalling a page's other requests (assets
/// proxied through PHP, Livewire/Inertia calls, polling) without holding
/// much memory — an idle `php-cgi` is a few tens of MB.
pub const WORKERS_PER_VERSION: u16 = 4;

/// Ports reserved per version. Larger than [`WORKERS_PER_VERSION`] on
/// purpose; see the module docs.
pub const PORT_BLOCK: u16 = 10;

// A block too small for its workers would hand one version's last worker
// the next version's first port — refused at compile time, not at runtime.
const _: () = assert!(WORKERS_PER_VERSION <= PORT_BLOCK);

/// First port of the default "php" service's block — every project that
/// hasn't pinned a version of its own is served from here.
pub const DEFAULT_BASE_PORT: u16 = 9100;

/// First port of the first pooled (non-default) version's block. The
/// default service's block is always reserved, so the pool starts one
/// block above it.
pub const BASE_PORT: u16 = DEFAULT_BASE_PORT + PORT_BLOCK;

/// The ports a version's workers bind, given its block's first port.
pub fn worker_ports(base: u16) -> impl Iterator<Item = u16> {
    (0..WORKERS_PER_VERSION).map(move |worker| base + worker)
}

/// The nginx `upstream` name for the workers of the block starting at
/// `base` — what a vhost's `fastcgi_pass` names instead of one host:port.
/// Keyed by port, not version, so it stays a valid nginx identifier whatever
/// a version string looks like, and so the default service's upstream never
/// changes name when the global version is switched.
pub fn upstream_name(base: u16) -> String {
    format!("rezure_php_{base}")
}

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

/// Assigns a port block to every version in `wanted`, returned as each
/// block's first port: reuses `existing`'s block for a version that already
/// has one (an already-running pooled service keeps exactly the ports it's
/// bound to, no matter what else changes), and picks the lowest block from
/// [`BASE_PORT`] not already claimed by *anything* in `existing` — including
/// an entry that's no longer in `wanted` — for one that doesn't.
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
                    candidate += PORT_BLOCK;
                }
                taken.insert(candidate);
                candidate
            }
        };
        result.insert(version.clone(), port);
    }
    result
}

/// The first port of the block a project's vhost should actually proxy to:
/// its own pinned version's pool block when it has a valid one,
/// `default_port` (the global default "php" service's block) otherwise — no
/// override, or one that isn't in `pool` (unpinned, or pinned to a version
/// no longer installed).
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
    fn every_version_gets_its_own_non_overlapping_block_of_workers() {
        let wanted = BTreeSet::from([
            "7.4.33".to_string(),
            "8.0.30".to_string(),
            "8.5.0".to_string(),
        ]);
        let assignment = assign_ports(&wanted, &BTreeMap::new());

        let mut all: Vec<u16> = worker_ports(DEFAULT_BASE_PORT).collect();
        for base in assignment.values() {
            all.extend(worker_ports(*base));
        }
        let unique: BTreeSet<u16> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "overlapping ports: {all:?}");
        assert_eq!(all.len(), 4 * WORKERS_PER_VERSION as usize);
    }

    #[test]
    fn a_blocks_workers_never_spill_into_the_next_block() {
        assert_eq!(
            worker_ports(DEFAULT_BASE_PORT).last(),
            Some(DEFAULT_BASE_PORT + WORKERS_PER_VERSION - 1)
        );
    }

    /// 9003 is where the IDE listens for Xdebug 3; 9000 is where Laragon's
    /// and XAMPP's own PHP responders usually sit. Neither may ever be
    /// handed to a worker.
    #[test]
    fn no_worker_lands_on_xdebugs_port_or_the_old_fastcgi_port() {
        let wanted: BTreeSet<String> = (0..20).map(|i| format!("8.{i}.0")).collect();
        let assignment = assign_ports(&wanted, &BTreeMap::new());

        let bases = std::iter::once(DEFAULT_BASE_PORT).chain(assignment.values().copied());
        for base in bases {
            for port in worker_ports(base) {
                assert_ne!(port, 9003, "a worker took Xdebug's port");
                assert_ne!(port, 9000, "a worker took the conventional FastCGI port");
            }
        }
    }

    #[test]
    fn upstream_names_are_plain_nginx_identifiers() {
        let name = upstream_name(DEFAULT_BASE_PORT);
        assert_eq!(name, "rezure_php_9100");
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
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
        let existing = BTreeMap::from([("8.0.30".to_string(), BASE_PORT + 5 * PORT_BLOCK)]);
        let wanted = BTreeSet::from(["8.0.30".to_string(), "8.5.0".to_string()]);

        let assignment = assign_ports(&wanted, &existing);

        assert_eq!(
            assignment["8.0.30"],
            BASE_PORT + 5 * PORT_BLOCK,
            "must not have moved"
        );
        assert_ne!(
            assignment["8.5.0"],
            BASE_PORT + 5 * PORT_BLOCK,
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
        assert_eq!(
            port_for_project(None, &pool, DEFAULT_BASE_PORT),
            DEFAULT_BASE_PORT
        );
    }

    #[test]
    fn port_for_project_uses_the_pool_port_when_pinned_and_present() {
        let pool = BTreeMap::from([("8.0.30".to_string(), BASE_PORT)]);
        assert_eq!(
            port_for_project(Some("8.0.30"), &pool, DEFAULT_BASE_PORT),
            BASE_PORT
        );
    }

    #[test]
    fn port_for_project_falls_back_when_pinned_to_something_not_in_the_pool() {
        let pool = BTreeMap::from([("8.0.30".to_string(), BASE_PORT)]);
        assert_eq!(
            port_for_project(Some("7.4.33"), &pool, DEFAULT_BASE_PORT),
            DEFAULT_BASE_PORT
        );
    }
}
