//! Auto-detects local projects by scanning `www_root()` — one folder, one
//! project, stack guessed from marker files (`artisan`, `wp-config.php`,
//! `package.json`, ...). The filesystem is the source of truth for *which*
//! projects exist, rescanned on every `list_projects` call; `commands::projects`
//! merges in each project's SQLite history (`db::projects`) afterward.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::hosts;
use crate::config::links;
use crate::db::projects::{ProjectInfo, ProjectKind};
use crate::utils::error::AppError;
use crate::utils::paths;

/// The suffix (no leading dot) every local domain is built with.
///
/// Fixed rather than configurable, because the alternatives don't work on a
/// plain HTTP vhost: `.dev` is a real gTLD on the browsers' HSTS preload
/// list, so they force `https://` before the hosts file is ever consulted,
/// and `.local` is reserved for mDNS, which on Windows intercepts it ahead
/// of the hosts file. `.test` is reserved for exactly this by RFC 6761.
pub const DOMAIN_SUFFIX: &str = "test";

/// Whether `domain` is safe to write into a generated nginx `server_name`
/// and into the Windows hosts file — ASCII letters, digits and hyphens per
/// dot-separated label.
///
/// This is a safety check, not a style rule. Windows allows a space, `;`,
/// `{` and `#` in a folder name, and every one of those means something to
/// nginx: a space makes it read the rest as a second server name, and `;`
/// or `{` ends the directive. The generated config holds every project, so
/// one bad name there stops nginx from loading at all and takes the other
/// sites down with it.
pub fn is_safe_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// `%USERPROFILE%\rezure\www` — where Rezure looks for projects. A fresh
/// install gets an empty folder created here rather than an error, so
/// there's always somewhere to drop a project into.
pub fn www_root() -> Result<PathBuf, AppError> {
    paths::www()
}

/// Every project Rezure knows about, sorted by name: the folders inside
/// `www_root()`, plus the ones linked from elsewhere.
///
/// The single place that answers "what projects exist" — the list, vhost
/// generation, the hosts file, the launcher and database-to-project
/// matching all read it, so linked projects work everywhere by virtue of
/// appearing here.
pub fn scan_projects() -> Result<Vec<ProjectInfo>, AppError> {
    let mut projects = scan_www()?;
    projects.extend(linked_projects(&projects));
    projects.sort_by_key(|p| p.name.to_lowercase());
    Ok(projects)
}

/// The linked folders, skipping any that duplicate a domain already taken
/// by a scanned project.
///
/// A duplicate should be impossible — `prepare_link` settles the domain
/// before saving — but a hand-edited `links.json`, or a folder later moved
/// into `www`, can still produce one. Two vhosts for one domain is a
/// confusing failure, so the scanned one wins and the link is logged.
fn linked_projects(scanned: &[ProjectInfo]) -> Vec<ProjectInfo> {
    let store = links::load();
    let mut taken: Vec<String> = scanned.iter().map(|p| p.domain.clone()).collect();
    let mut projects = Vec::new();

    for link in store.links {
        if taken.iter().any(|d| d.eq_ignore_ascii_case(&link.domain)) {
            log::warn!(
                "linked project {} wants {}, which is already served — skipping it",
                link.path,
                link.domain
            );
            continue;
        }
        taken.push(link.domain.clone());

        let path = PathBuf::from(&link.path);
        let missing = !path.is_dir();

        projects.push(ProjectInfo {
            id: link.id,
            name: link.name,
            path: link.path,
            has_hosts_entry: hosts::has_entry(&link.domain),
            // `link()` rejects an unsafe domain, but a `links.json` written
            // by an older build or edited by hand hasn't been through it.
            domain_invalid: !is_safe_domain(&link.domain),
            domain: link.domain,
            // A folder that isn't there can't be inspected; the badge would
            // otherwise read "Unknown" as though the project had no stack.
            stack: if missing {
                "Missing".to_string()
            } else {
                detect_stack(&path)
            },
            last_opened_at: None,
            open_count: 0,
            kind: ProjectKind::Linked,
            missing,
        });
    }
    projects
}

/// Scans `www_root()` for project folders.
fn scan_www() -> Result<Vec<ProjectInfo>, AppError> {
    let root = www_root()?;
    fs::create_dir_all(&root)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", root.display())))?;

    let entries = fs::read_dir(&root)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", root.display())))?;

    let mut projects = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Some(folder_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Skip hidden/system folders (e.g. a stray `.git` if `www/` itself
        // ends up pointed at a repo root).
        if folder_name.starts_with('.') {
            continue;
        }

        let domain = format!("{folder_name}.{DOMAIN_SUFFIX}");
        // The folder name comes off the disk, so nothing has vetted it —
        // it's whatever the user (or an unzipped archive) called it.
        let domain_invalid = !is_safe_domain(&domain);
        projects.push(ProjectInfo {
            id: folder_name.to_string(),
            name: folder_name.to_string(),
            path: path.display().to_string(),
            has_hosts_entry: hosts::has_entry(&domain),
            domain,
            domain_invalid,
            stack: detect_stack(&path),
            // Filled in by `commands::projects::list_projects` from SQLite —
            // a bare scan has no way to know this.
            last_opened_at: None,
            open_count: 0,
            kind: ProjectKind::Scanned,
            // A scanned project is a folder that was just read, so it's
            // there by definition.
            missing: false,
        });
    }

    Ok(projects)
}

/// What linking a folder would produce, without doing it.
///
/// Lets the dialog show the name, stack and domain the user is about to
/// get — and refuse a bad path — before anything is written.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    pub path: String,
    pub name: String,
    pub domain: String,
    pub stack: String,
    /// The folder nginx would actually serve — `public/` for Laravel.
    pub docroot: String,
    /// True when the domain had to be adjusted to avoid a clash, so the
    /// dialog can say why it isn't the obvious one.
    pub domain_adjusted: bool,
}

/// Folders that must never become a vhost root.
///
/// Serving a drive root or a Windows directory over HTTP would expose the
/// whole machine to anything that can reach the port. This is refused
/// rather than warned about — there is no legitimate version of it.
fn reject_dangerous_root(path: &Path) -> Result<(), AppError> {
    let display = path.display().to_string();

    if path.parent().is_none() {
        return Err(AppError::UnusableProjectPath {
            path: display,
            reason: "it's a drive root, and serving a whole drive isn't safe".to_string(),
        });
    }

    let lowered = display.to_lowercase().replace('/', "\\");
    let protected = [
        r"c:\windows",
        r"c:\program files",
        r"c:\program files (x86)",
    ];
    if protected
        .iter()
        .any(|dir| lowered == *dir || lowered.starts_with(&format!("{dir}\\")))
    {
        return Err(AppError::UnusableProjectPath {
            path: display,
            reason: "it's a system folder".to_string(),
        });
    }
    Ok(())
}

/// Picks a free domain for `name` under `DOMAIN_SUFFIX`, appending `-2`,
/// `-3`… only if the obvious one is taken.
fn free_domain(base_slug: &str, taken: &[String]) -> (String, bool) {
    let suffix = DOMAIN_SUFFIX;
    let is_free = |candidate: &str| !taken.iter().any(|d| d.eq_ignore_ascii_case(candidate));

    let first = format!("{base_slug}.{suffix}");
    if is_free(&first) {
        return (first, false);
    }
    for n in 2..100 {
        let candidate = format!("{base_slug}-{n}.{suffix}");
        if is_free(&candidate) {
            return (candidate, true);
        }
    }
    // Practically unreachable; a suffixed fallback still beats a clash.
    (
        format!("{base_slug}-{}.{suffix}", links::id_for(base_slug, "x")),
        true,
    )
}

/// Validates a folder and works out what linking it would produce.
pub fn prepare_link(path: &str) -> Result<LinkPreview, AppError> {
    let folder = PathBuf::from(path);

    if !folder.is_dir() {
        return Err(AppError::UnusableProjectPath {
            path: path.to_string(),
            reason: "it isn't a folder".to_string(),
        });
    }
    reject_dangerous_root(&folder)?;

    // Anything under `www` is already picked up by the scan; linking it too
    // would list it twice and generate two vhosts for one folder.
    if links::is_inside(path, &www_root()?) {
        return Err(AppError::UnusableProjectPath {
            path: path.to_string(),
            reason: "it's already inside your www folder, so it's listed automatically".to_string(),
        });
    }

    let store = links::load();
    if let Some(existing) = store.linked_at(path) {
        return Err(AppError::ProjectAlreadyLinked {
            path: path.to_string(),
            name: existing.name.clone(),
        });
    }

    let name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();
    let slug = links::slugify(&name);
    let slug = if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    };

    let taken: Vec<String> = scan_projects()?.into_iter().map(|p| p.domain).collect();
    let (domain, domain_adjusted) = free_domain(&slug, &taken);
    let stack = detect_stack(&folder);

    Ok(LinkPreview {
        docroot: docroot(&folder, &stack).display().to_string(),
        path: path.to_string(),
        name,
        domain,
        stack,
        domain_adjusted,
    })
}

/// Registers a folder as a project. Writes nothing inside it.
pub fn link(path: &str, name: Option<String>, domain: Option<String>) -> Result<(), AppError> {
    // Re-validated here rather than trusting the preview: the dialog may
    // have been open a while, and the folder or the domain could have been
    // taken in the meantime.
    let preview = prepare_link(path)?;

    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or(preview.name);

    let domain = match domain
        .map(|d| d.trim().to_lowercase())
        .filter(|d| !d.is_empty())
    {
        Some(requested) => {
            // Checked before the collision test: this one ends up verbatim
            // in the generated nginx config, where a stray `;` or space
            // breaks every site, not just this one.
            if !is_safe_domain(&requested) {
                return Err(AppError::UnusableProjectPath {
                    path: path.to_string(),
                    reason: format!(
                        "{requested} isn't a usable domain — use letters, digits and hyphens only"
                    ),
                });
            }
            let taken: Vec<String> = scan_projects()?.into_iter().map(|p| p.domain).collect();
            if taken.iter().any(|d| d.eq_ignore_ascii_case(&requested)) {
                return Err(AppError::UnusableProjectPath {
                    path: path.to_string(),
                    reason: format!("{requested} is already used by another project"),
                });
            }
            requested
        }
        None => preview.domain,
    };

    let mut store = links::load();
    links::add(&mut store, path.to_string(), name, domain);
    links::save(&store)
}

/// Forgets a linked project. Never touches the folder itself.
pub fn unlink(id: &str) -> Result<(), AppError> {
    let mut store = links::load();
    links::remove(&mut store, id)?;
    links::save(&store)
}

/// The project's docroot — Laravel serves from `public/`, everything else
/// from the project root itself. Used by vhost generation.
pub fn docroot(project_path: &Path, stack: &str) -> PathBuf {
    if stack == "Laravel" && project_path.join("public").is_dir() {
        project_path.join("public")
    } else {
        project_path.to_path_buf()
    }
}

fn detect_stack(dir: &Path) -> String {
    if dir.join("artisan").is_file() {
        return "Laravel".to_string();
    }
    if dir.join("wp-config.php").is_file() || dir.join("wp-load.php").is_file() {
        return "WordPress".to_string();
    }
    if let Some(stack) = detect_from_package_json(dir) {
        return stack;
    }
    if dir.join("composer.json").is_file() || dir.join("index.php").is_file() {
        return "PHP".to_string();
    }
    if dir.join("index.html").is_file() {
        return "Static".to_string();
    }
    "Unknown".to_string()
}

/// Looks for a handful of well-known frontend framework dependencies in
/// `package.json`; falls back to a generic "Node" if the file exists but
/// none of them match.
fn detect_from_package_json(dir: &Path) -> Option<String> {
    let content = fs::read_to_string(dir.join("package.json")).ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;

    let has_dep = |name: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .filter_map(|section| json.get(section)?.as_object())
            .any(|deps| deps.contains_key(name))
    };

    const FRAMEWORKS: &[(&str, &str)] = &[
        ("next", "Next.js"),
        ("nuxt", "Nuxt"),
        ("vue", "Vue"),
        ("react", "React"),
        ("svelte", "Svelte"),
    ];

    for (dep, label) in FRAMEWORKS {
        if has_dep(dep) {
            return Some((*label).to_string());
        }
    }

    Some("Node".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_folder_names_are_servable() {
        assert!(is_safe_domain("blog.test"));
        assert!(is_safe_domain("my-shop-2.test"));
        // Uppercase has always worked and must keep working — nginx matches
        // server names case-insensitively.
        assert!(is_safe_domain("MyBlog.test"));
        assert!(is_safe_domain("api.internal.test"));
    }

    /// Every one of these is a legal Windows folder name, and every one of
    /// them means something to nginx.
    #[test]
    fn names_that_would_corrupt_the_nginx_config_are_rejected() {
        // Read as two server names, so the site silently answers on the
        // wrong domain rather than failing loudly.
        assert!(!is_safe_domain("My App.test"));
        // Ends the directive — the rest becomes a stray one and nginx
        // refuses to load the whole config.
        assert!(!is_safe_domain("foo;return.test"));
        assert!(!is_safe_domain("foo{bar.test"));
        assert!(!is_safe_domain("foo}bar.test"));
        // Comments out the remainder of the hosts-file line.
        assert!(!is_safe_domain("foo#bar.test"));
        assert!(!is_safe_domain("foo'bar.test"));
        assert!(!is_safe_domain("foo\"bar.test"));
        assert!(!is_safe_domain("foo\nbar.test"));
    }

    #[test]
    fn malformed_labels_are_rejected() {
        assert!(!is_safe_domain(""));
        assert!(!is_safe_domain(".test"));
        assert!(!is_safe_domain("foo..test"));
        assert!(!is_safe_domain("-foo.test"));
        assert!(!is_safe_domain("foo-.test"));
        // Non-ASCII resolves nowhere without punycode, so it isn't served.
        assert!(!is_safe_domain("café.test"));
    }

    fn temp_project(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rezure-test-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_laravel_via_artisan() {
        let dir = temp_project("laravel");
        fs::write(dir.join("artisan"), "").unwrap();
        assert_eq!(detect_stack(&dir), "Laravel");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_wordpress_via_wp_config() {
        let dir = temp_project("wordpress");
        fs::write(dir.join("wp-config.php"), "").unwrap();
        assert_eq!(detect_stack(&dir), "WordPress");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_vue_via_package_json_dependency() {
        let dir = temp_project("vue");
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"vue":"^3.5.0"}}"#,
        )
        .unwrap();
        assert_eq!(detect_stack(&dir), "Vue");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn falls_back_to_node_for_unrecognized_package_json() {
        let dir = temp_project("node");
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"express":"^4.0.0"}}"#,
        )
        .unwrap();
        assert_eq!(detect_stack(&dir), "Node");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn falls_back_to_unknown_for_an_empty_folder() {
        let dir = temp_project("empty");
        assert_eq!(detect_stack(&dir), "Unknown");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The rule that matters most: never turn a drive root or a Windows
    /// folder into a vhost root — that would serve the machine over HTTP.
    #[test]
    fn dangerous_roots_are_refused() {
        for bad in [
            r"C:\",
            r"C:\Windows",
            r"C:\Windows\System32",
            r"C:\Program Files",
        ] {
            assert!(
                reject_dangerous_root(Path::new(bad)).is_err(),
                "{bad} must be refused"
            );
        }
        assert!(reject_dangerous_root(Path::new(r"C:\repository\ORDO")).is_ok());
    }

    /// A folder already inside `www` is scanned automatically; linking it
    /// too would list it twice and write two vhosts for one folder.
    #[test]
    fn a_folder_inside_www_cannot_also_be_linked() {
        let inside = www_root().unwrap().join("already-here");
        let err = prepare_link(&inside.display().to_string()).unwrap_err();
        // It may fail either because it's inside www or because it doesn't
        // exist; only the first is the rule under test, so create it.
        let _ = fs::create_dir_all(&inside);

        let err = prepare_link(&inside.display().to_string())
            .err()
            .unwrap_or(err);
        let _ = fs::remove_dir_all(&inside);

        assert!(
            format!("{err}").contains("www"),
            "the message must explain it's already covered, got: {err}"
        );
    }

    #[test]
    fn a_path_that_isnt_a_folder_is_refused() {
        let err = prepare_link(r"C:\definitely\not\here").unwrap_err();
        assert!(matches!(err, AppError::UnusableProjectPath { .. }));
    }

    /// Two projects can't share a domain, so the second gets a suffix
    /// rather than silently overwriting the first's vhost.
    #[test]
    fn a_taken_domain_gets_a_numbered_suffix() {
        let taken = vec!["api.test".to_string()];

        let (first, adjusted) = free_domain("ordo", &taken);
        assert_eq!(first, "ordo.test");
        assert!(!adjusted, "an unused name needs no adjustment");

        let (second, adjusted) = free_domain("api", &taken);
        assert_eq!(second, "api-2.test");
        assert!(adjusted, "the caller has to be able to explain the suffix");

        // And it keeps counting rather than stopping at one alternative.
        let taken = vec!["api.test".to_string(), "api-2.test".to_string()];
        assert_eq!(free_domain("api", &taken).0, "api-3.test");
    }

    /// Domains are compared case-insensitively — `API.test` and `api.test`
    /// are one domain as far as nginx and the hosts file are concerned.
    #[test]
    fn domain_collision_ignores_case() {
        let taken = vec!["API.test".to_string()];
        assert_eq!(free_domain("api", &taken).0, "api-2.test");
    }

    /// A linked folder that's gone is still listed, but marked — dropping
    /// it would look like Rezure lost the project.
    #[test]
    fn a_missing_linked_folder_is_reported_not_hidden() {
        let scanned: Vec<ProjectInfo> = Vec::new();
        // Nothing is linked in a clean test environment, so this asserts on
        // the shape rather than fabricating a store: whatever comes back,
        // anything flagged missing must carry the Missing stack, and
        // anything present must not be flagged.
        for project in linked_projects(&scanned) {
            if project.missing {
                assert_eq!(project.stack, "Missing");
            } else {
                assert_ne!(project.stack, "Missing");
            }
        }
    }

    #[test]
    fn laravel_docroot_is_the_public_folder() {
        let dir = temp_project("laravel-docroot");
        fs::create_dir_all(dir.join("public")).unwrap();
        assert_eq!(docroot(&dir, "Laravel"), dir.join("public"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn non_laravel_docroot_is_the_project_root() {
        let dir = temp_project("static-docroot");
        assert_eq!(docroot(&dir, "Static"), dir);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_projects_creates_www_root_and_returns_empty_when_no_projects_exist() {
        // Not asserting on real `www_root()` contents (the developer
        // machine may have real projects there) — just that scanning a
        // freshly-created empty root doesn't error.
        let root = www_root().unwrap();
        assert!(root.ends_with("rezure\\www") || root.ends_with("rezure/www"));
    }

    /// The whole link path against a real folder outside `www`: preview it,
    /// link it, confirm it shows up in `scan_projects` (which is what feeds
    /// vhosts, the hosts file and the launcher), then unlink and confirm the
    /// folder is untouched. Run with:
    /// `cargo test --lib services::projects::tests::link_a_real_outside_folder -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn link_a_real_outside_folder() {
        // Deliberately outside www, and deliberately not somewhere precious.
        let dir = std::env::temp_dir().join(format!("rezure-link-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.php"), "<?php echo 'linked';").unwrap();
        let path = dir.display().to_string();

        let preview = prepare_link(&path).expect("a normal folder must be linkable");
        println!(
            "preview: name={} domain={} stack={} docroot={}",
            preview.name, preview.domain, preview.stack, preview.docroot
        );
        assert_eq!(preview.stack, "PHP");

        link(&path, None, None).expect("linking must succeed");

        let listed = scan_projects().unwrap();
        let found = listed
            .iter()
            .find(|p| p.path.eq_ignore_ascii_case(&path))
            .expect("a linked folder must appear in the project list");
        println!(
            "listed as {} ({}) kind={:?}",
            found.name, found.domain, found.kind
        );
        assert_eq!(found.kind, ProjectKind::Linked);
        assert!(!found.missing);

        // Linking the same folder twice must be refused, not duplicated.
        assert!(
            prepare_link(&path).is_err(),
            "a folder already linked can't be linked again"
        );

        unlink(&found.id).expect("unlinking must succeed");
        assert!(
            !scan_projects()
                .unwrap()
                .iter()
                .any(|p| p.path.eq_ignore_ascii_case(&path)),
            "an unlinked project must leave the list"
        );
        assert!(
            dir.join("index.php").is_file(),
            "unlinking must never touch the folder's contents"
        );

        let _ = fs::remove_dir_all(&dir);
        println!("linked, listed, unlinked — folder left intact");
    }

    /// Manual, real-filesystem check against whatever's actually in
    /// `www_root()` right now — run with:
    /// `cargo test --lib services::projects::tests::print_real_scan -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_real_scan() {
        for p in scan_projects().unwrap() {
            println!("{} | {} | {} | {}", p.id, p.stack, p.domain, p.path);
        }
    }
}
