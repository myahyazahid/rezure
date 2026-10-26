//! Auto-detects local projects by scanning `www_root()` — one folder, one
//! project, framework recognised by `services::frameworks` (with the user's
//! own choice from `config::project_frameworks` winning over detection). The
//! filesystem is the source of truth for *which* projects exist, rescanned on
//! every `list_projects` call; `commands::projects` merges in each project's
//! SQLite history (`db::projects`) afterward.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::frameworks::{self, FrameworkCatalog, Registry};
use super::hosts;
use crate::config::links::{self, LinkStore};
use crate::config::project_frameworks::{self, FrameworkOverride, OverrideStore};
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
/// appearing here. The same goes for framework choices: they're applied
/// here, so every one of those consumers serves the corrected docroot.
pub fn scan_projects() -> Result<Vec<ProjectInfo>, AppError> {
    let registry = frameworks::registry();
    let overrides = project_frameworks::load();
    let mut projects = scan_www(&registry, &overrides)?;
    projects.extend(linked_projects(&projects, &registry, &overrides));
    projects.sort_by_key(|p| p.name.to_lowercase());
    Ok(projects)
}

/// Re-resolves `id` against a fresh scan.
///
/// Every caller that only ever received a project *id* from the frontend
/// (`services::launcher`, `services::share`) goes through this rather than
/// trusting a domain or path passed in alongside it — the scan is the one
/// source of truth for what a project's real domain is right now.
pub fn find(id: &str) -> Result<ProjectInfo, AppError> {
    scan_projects()?
        .into_iter()
        .find(|project| project.id == id)
        .ok_or_else(|| AppError::ProjectNotFound(id.to_string()))
}

/// The linked folders, skipping any that duplicate a domain already taken
/// by a scanned project.
///
/// A duplicate should be impossible — `prepare_link` settles the domain
/// before saving — but a hand-edited `links.json`, or a folder later moved
/// into `www`, can still produce one. Two vhosts for one domain is a
/// confusing failure, so the scanned one wins and the link is logged.
fn linked_projects(
    scanned: &[ProjectInfo],
    registry: &Registry,
    overrides: &OverrideStore,
) -> Vec<ProjectInfo> {
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
        let choice = overrides.get(&link.id);

        // A folder that isn't there can't be inspected; the badge would
        // otherwise read "Unknown" as though the project had no framework.
        let (stack, framework, detected_framework, framework_overridden, docroot) = if missing {
            (
                "Missing".to_string(),
                frameworks::UNKNOWN.to_string(),
                frameworks::UNKNOWN.to_string(),
                choice.is_some(),
                link.path.clone(),
            )
        } else {
            let serving = registry.serving(&path, choice);
            (
                serving.label,
                serving.framework,
                serving.detected,
                serving.overridden,
                serving.docroot.display().to_string(),
            )
        };

        projects.push(ProjectInfo {
            id: link.id,
            name: link.name,
            path: link.path,
            has_hosts_entry: hosts::has_entry(&link.domain),
            // `link()` rejects an unsafe domain, but a `links.json` written
            // by an older build or edited by hand hasn't been through it.
            domain_invalid: !is_safe_domain(&link.domain),
            domain: link.domain,
            stack,
            framework,
            detected_framework,
            framework_overridden,
            docroot,
            last_opened_at: None,
            open_count: 0,
            kind: ProjectKind::Linked,
            missing,
            // Filled in by `commands::projects::list_projects` from SQLite,
            // same as `last_opened_at`/`open_count` — a scan has no opinion
            // on PHP version.
            php_version: None,
            node_version: None,
        });
    }
    projects
}

/// Scans `www_root()` for project folders.
fn scan_www(registry: &Registry, overrides: &OverrideStore) -> Result<Vec<ProjectInfo>, AppError> {
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
        let serving = registry.serving(&path, overrides.get(folder_name));
        projects.push(ProjectInfo {
            id: folder_name.to_string(),
            name: folder_name.to_string(),
            path: path.display().to_string(),
            has_hosts_entry: hosts::has_entry(&domain),
            domain,
            domain_invalid,
            stack: serving.label,
            framework: serving.framework,
            detected_framework: serving.detected,
            framework_overridden: serving.overridden,
            docroot: serving.docroot.display().to_string(),
            // Filled in by `commands::projects::list_projects` from SQLite —
            // a bare scan has no way to know this.
            last_opened_at: None,
            open_count: 0,
            kind: ProjectKind::Scanned,
            // A scanned project is a folder that was just read, so it's
            // there by definition.
            missing: false,
            // Filled in by `commands::projects::list_projects` from SQLite —
            // a bare scan has no way to know this either.
            php_version: None,
            node_version: None,
        });
    }

    Ok(projects)
}

/// What linking a folder would produce, without doing it.
///
/// Lets the dialog show the name, framework and domain the user is about to
/// get — and refuse a bad path — before anything is written.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    pub path: String,
    pub name: String,
    pub domain: String,
    /// The label of the framework it would be served as.
    pub stack: String,
    /// That framework's id — the user's choice when they made one.
    pub framework: String,
    /// What detection alone says.
    pub detected_framework: String,
    /// The folder nginx would actually serve — `public/` for Laravel.
    pub docroot: String,
    /// True when the domain had to be adjusted to avoid a clash, so the
    /// dialog can say why it isn't the obvious one.
    pub domain_adjusted: bool,
    /// Set when the folder picked looks like the docroot *of* a project —
    /// see [`parent_suggestion`].
    pub suggestion: Option<LinkSuggestion>,
}

/// "This is the `public\` folder of a CodeIgniter 4 project — add the
/// project instead?"
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSuggestion {
    /// The project folder to link instead.
    pub path: String,
    /// The label of the framework detected there.
    pub framework: String,
    /// The docroot the picked folder turned out to be, as the definition
    /// names it (`public`).
    pub folder: String,
}

/// What [`link`] did, for telemetry.
#[derive(Debug, Clone)]
pub struct LinkOutcome {
    pub detected_framework: String,
    /// `None` when the user kept what detection found.
    pub choice: Option<FrameworkOverride>,
}

/// What [`set_framework`] changed, for telemetry.
#[derive(Debug, Clone)]
pub struct FrameworkChange {
    pub detected_framework: String,
    /// `None` when the project went back to detection.
    pub choice: Option<FrameworkOverride>,
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

/// The user's framework choice from the two optional fields a command
/// receives — a framework id, or a folder for "Custom". Blank counts as
/// absent; neither means "go with detection".
pub fn choice(
    framework: Option<String>,
    docroot: Option<String>,
) -> Result<Option<FrameworkOverride>, AppError> {
    let framework = framework
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty());
    let docroot = docroot
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    match (framework, docroot) {
        (Some(_), Some(docroot)) => Err(AppError::InvalidDocroot {
            docroot,
            reason: "pick a framework or a folder, not both".to_string(),
        }),
        (Some(framework), None) => Ok(Some(FrameworkOverride::Framework(framework))),
        (None, Some(docroot)) => Ok(Some(FrameworkOverride::Docroot(docroot))),
        (None, None) => Ok(None),
    }
}

/// Refuses a choice that can't be served: a framework the registry doesn't
/// have, or a folder that's outside the project, unsafe in an nginx config,
/// or simply not there.
fn validate_choice(
    registry: &Registry,
    project: &Path,
    choice: &FrameworkOverride,
) -> Result<(), AppError> {
    match choice {
        FrameworkOverride::Framework(id) => match registry.get(id) {
            Some(_) => Ok(()),
            None => Err(AppError::UnknownFramework(id.clone())),
        },
        FrameworkOverride::Docroot(docroot) => {
            frameworks::validate_relative_path(docroot).map_err(|reason| {
                AppError::InvalidDocroot {
                    docroot: docroot.clone(),
                    reason,
                }
            })?;
            if frameworks::resolve_docroot(project, Some(docroot)) == project {
                return Err(AppError::InvalidDocroot {
                    docroot: docroot.clone(),
                    reason: "there's no folder by that name in this project".to_string(),
                });
            }
            Ok(())
        }
    }
}

/// Validates a folder and works out what linking it would produce — with
/// the user's framework `choice`, when they made one.
pub fn prepare_link(
    path: &str,
    choice: Option<&FrameworkOverride>,
) -> Result<LinkPreview, AppError> {
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
    let www = www_root()?;
    if links::is_inside(path, &www) {
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

    let registry = frameworks::registry();
    if let Some(choice) = choice {
        validate_choice(&registry, &folder, choice)?;
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
    let serving = registry.serving(&folder, choice);

    Ok(LinkPreview {
        docroot: serving.docroot.display().to_string(),
        path: path.to_string(),
        name,
        domain,
        stack: serving.label,
        framework: serving.framework,
        detected_framework: serving.detected,
        domain_adjusted,
        suggestion: parent_suggestion(&folder, &registry, &store, &www),
    })
}

/// When `folder` is the docroot of a framework project above it — the
/// `public\` of a CodeIgniter 4 app, say — that project folder.
///
/// Linking `public\` directly is the obvious workaround when detection gets
/// a project wrong, and it half works: the site loads, but the project's
/// folder and terminal buttons open `public\`, and its name is taken by the
/// wrong folder. Offering the project itself undoes that.
fn parent_suggestion(
    folder: &Path,
    registry: &Registry,
    store: &LinkStore,
    www: &Path,
) -> Option<LinkSuggestion> {
    let docroots: BTreeSet<&str> = registry
        .frameworks()
        .iter()
        .filter_map(|f| f.def.docroot.as_deref())
        .collect();

    for docroot in docroots {
        let depth = docroot.split(['/', '\\']).filter(|s| !s.is_empty()).count();
        let Some(project) = folder.ancestors().nth(depth) else {
            continue;
        };
        let served = frameworks::resolve_docroot(project, Some(docroot));
        if served == project
            || !links::same_path(&served.display().to_string(), &folder.display().to_string())
        {
            continue;
        }

        let project_path = project.display().to_string();
        let linkable = reject_dangerous_root(project).is_ok()
            && store.linked_at(&project_path).is_none()
            && !links::is_inside(&project_path, www);
        if !linkable {
            continue;
        }

        let Some(detected) = registry.detect(project) else {
            continue;
        };
        if detected.def.docroot.as_deref() == Some(docroot) {
            return Some(LinkSuggestion {
                path: project_path,
                framework: detected.def.label.clone(),
                folder: docroot.to_string(),
            });
        }
    }
    None
}

/// Registers a folder as a project, with the user's framework `choice` if
/// they made one. Writes nothing inside the folder.
pub fn link(
    path: &str,
    name: Option<String>,
    domain: Option<String>,
    choice: Option<FrameworkOverride>,
) -> Result<LinkOutcome, AppError> {
    // Re-validated here rather than trusting the preview: the dialog may
    // have been open a while, and the folder or the domain could have been
    // taken in the meantime.
    let preview = prepare_link(path, choice.as_ref())?;

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

    // Picking exactly what detection found is no choice at all. Leaving it
    // unset means a better detector in a later release still applies.
    let choice = choice.filter(|choice| {
        !matches!(choice, FrameworkOverride::Framework(id) if *id == preview.detected_framework)
    });

    // The dialog's choice is the whole truth for this id — including "Auto",
    // which clears anything a previous link of the same folder left behind.
    // Written before the link itself, so a failure here leaves nothing half
    // added.
    let id = links::id_for(path, &name);
    let mut overrides = project_frameworks::load();
    if overrides.set(&id, choice.clone()) {
        project_frameworks::save(&overrides)?;
    }

    let mut store = links::load();
    links::add(&mut store, path.to_string(), name, domain);
    links::save(&store)?;

    Ok(LinkOutcome {
        detected_framework: preview.detected_framework,
        choice,
    })
}

/// Forgets a linked project, and the framework chosen for it. Never touches
/// the folder itself.
pub fn unlink(id: &str) -> Result<(), AppError> {
    let mut store = links::load();
    links::remove(&mut store, id)?;
    links::save(&store)?;

    // Best-effort: a leftover override only matters if the same folder is
    // linked again under the same name, and `link` resets it then anyway.
    let mut overrides = project_frameworks::load();
    if overrides.set(id, None) {
        if let Err(err) = project_frameworks::save(&overrides) {
            log::warn!("unlinked {id} but couldn't forget its framework choice: {err}");
        }
    }
    Ok(())
}

/// Sets, or with `None` clears back to detection, the framework a project is
/// served as — scanned and linked projects alike.
pub fn set_framework(
    id: &str,
    choice: Option<FrameworkOverride>,
) -> Result<FrameworkChange, AppError> {
    let project = find(id)?;
    if let Some(choice) = &choice {
        validate_choice(&frameworks::registry(), Path::new(&project.path), choice)?;
    }

    let mut overrides = project_frameworks::load();
    if overrides.set(id, choice.clone()) {
        project_frameworks::save(&overrides)?;
    }
    Ok(FrameworkChange {
        detected_framework: project.detected_framework,
        choice,
    })
}

/// The frameworks the pickers offer, and everything the user should know is
/// wrong: definitions that were skipped, and projects set to a framework
/// that has since disappeared (they're served as detected meanwhile).
pub fn framework_catalog() -> Result<FrameworkCatalog, AppError> {
    let registry = frameworks::registry();
    let overrides = project_frameworks::load();
    let mut catalog = registry.catalog();

    for project in scan_projects()? {
        if let Some(FrameworkOverride::Framework(id)) = overrides.get(&project.id) {
            if registry.get(id).is_none() {
                catalog.problems.push(format!(
                    "{} is set to the framework \"{id}\", which isn't defined any more — it's served as detected until you pick another.",
                    project.name
                ));
            }
        }
    }
    Ok(catalog)
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

    /// The label a project folder gets from the built-in definitions alone.
    fn stack_of(dir: &Path) -> String {
        frameworks::builtin_registry().serving(dir, None).label
    }

    #[test]
    fn detects_laravel_via_artisan() {
        let dir = temp_project("laravel");
        fs::write(dir.join("artisan"), "").unwrap();
        assert_eq!(stack_of(&dir), "Laravel");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_wordpress_via_wp_config() {
        let dir = temp_project("wordpress");
        fs::write(dir.join("wp-config.php"), "").unwrap();
        assert_eq!(stack_of(&dir), "WordPress");
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
        assert_eq!(stack_of(&dir), "Vue");
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
        assert_eq!(stack_of(&dir), "Node");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The regression this release is about: npm for assets doesn't make a
    /// PHP project a Node one.
    #[test]
    fn a_php_project_that_also_uses_npm_stays_php() {
        let dir = temp_project("php-with-npm");
        fs::write(dir.join("index.php"), "<?php").unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"sass":"latest"}}"#,
        )
        .unwrap();
        assert_eq!(stack_of(&dir), "PHP");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn falls_back_to_unknown_for_an_empty_folder() {
        let dir = temp_project("empty");
        assert_eq!(stack_of(&dir), "Unknown");
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
        let err = prepare_link(&inside.display().to_string(), None).unwrap_err();
        // It may fail either because it's inside www or because it doesn't
        // exist; only the first is the rule under test, so create it.
        let _ = fs::create_dir_all(&inside);

        let err = prepare_link(&inside.display().to_string(), None)
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
        let err = prepare_link(r"C:\definitely\not\here", None).unwrap_err();
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
        let registry = frameworks::builtin_registry();
        for project in linked_projects(&scanned, &registry, &OverrideStore::default()) {
            if project.missing {
                assert_eq!(project.stack, "Missing");
                assert_eq!(project.docroot, project.path);
            } else {
                assert_ne!(project.stack, "Missing");
            }
        }
    }

    #[test]
    fn a_choice_needs_one_field_not_both() {
        assert_eq!(choice(None, None).unwrap(), None);
        assert_eq!(
            choice(Some("  ".into()), Some(String::new())).unwrap(),
            None
        );
        assert_eq!(
            choice(Some("laravel".into()), None).unwrap(),
            Some(FrameworkOverride::Framework("laravel".into()))
        );
        assert_eq!(
            choice(None, Some(" public ".into())).unwrap(),
            Some(FrameworkOverride::Docroot("public".into()))
        );
        assert!(choice(Some("laravel".into()), Some("public".into())).is_err());
    }

    #[test]
    fn a_choice_that_cannot_be_served_is_refused() {
        let dir = temp_project("choice");
        fs::create_dir_all(dir.join("htdocs")).unwrap();
        let registry = frameworks::builtin_registry();

        let unknown = FrameworkOverride::Framework("acme".into());
        assert!(matches!(
            validate_choice(&registry, &dir, &unknown),
            Err(AppError::UnknownFramework(_))
        ));
        for bad in ["..", r"C:\Windows", "nope"] {
            let docroot = FrameworkOverride::Docroot(bad.into());
            assert!(
                matches!(
                    validate_choice(&registry, &dir, &docroot),
                    Err(AppError::InvalidDocroot { .. })
                ),
                "{bad} must be refused"
            );
        }
        assert!(validate_choice(
            &registry,
            &dir,
            &FrameworkOverride::Docroot("htdocs".into())
        )
        .is_ok());
        assert!(validate_choice(
            &registry,
            &dir,
            &FrameworkOverride::Framework("codeigniter4".into())
        )
        .is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Picking `public\` of a CodeIgniter 4 app — the workaround for the
    /// old detection bug — offers the app itself instead.
    #[test]
    fn the_public_folder_of_a_project_suggests_the_project() {
        let project = temp_project("suggest-ci4");
        fs::write(project.join("spark"), "").unwrap();
        fs::create_dir_all(project.join("public")).unwrap();
        fs::write(project.join("public").join("index.php"), "<?php").unwrap();
        let registry = frameworks::builtin_registry();
        let www = Path::new(r"C:\nowhere\www");

        let suggestion = parent_suggestion(
            &project.join("public"),
            &registry,
            &LinkStore::default(),
            www,
        )
        .expect("public\\ of a CodeIgniter 4 app must suggest the app");
        assert!(links::same_path(
            &suggestion.path,
            &project.display().to_string()
        ));
        assert_eq!(suggestion.framework, "CodeIgniter 4");
        assert_eq!(suggestion.folder, "public");

        // The project itself, or an ordinary folder, suggests nothing.
        assert!(parent_suggestion(&project, &registry, &LinkStore::default(), www).is_none());
        let plain = temp_project("suggest-plain");
        fs::create_dir_all(plain.join("public")).unwrap();
        assert!(
            parent_suggestion(&plain.join("public"), &registry, &LinkStore::default(), www)
                .is_none()
        );

        fs::remove_dir_all(&project).unwrap();
        fs::remove_dir_all(&plain).unwrap();
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

        let preview = prepare_link(&path, None).expect("a normal folder must be linkable");
        println!(
            "preview: name={} domain={} stack={} docroot={}",
            preview.name, preview.domain, preview.stack, preview.docroot
        );
        assert_eq!(preview.stack, "PHP");

        link(&path, None, None, None).expect("linking must succeed");

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
            prepare_link(&path, None).is_err(),
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
            println!(
                "{} | {} ({}{}) | {} | {} | docroot {}",
                p.id,
                p.stack,
                p.framework,
                if p.framework_overridden {
                    ", manual"
                } else {
                    ""
                },
                p.domain,
                p.path,
                p.docroot
            );
        }
    }

    /// Detection against a folder of real projects, without linking any of
    /// them — set `REZURE_DETECT_DIR` to the folder that holds them:
    /// `$env:REZURE_DETECT_DIR='C:\repository\ORDO'; cargo test --lib services::projects::tests::print_detection -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_detection() {
        let Some(root) = std::env::var_os("REZURE_DETECT_DIR") else {
            println!("set REZURE_DETECT_DIR to a folder of projects");
            return;
        };
        let registry = frameworks::builtin_registry();
        for entry in fs::read_dir(root).unwrap().flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let serving = registry.serving(&dir, None);
            println!(
                "{:<40} {:<16} docroot {}",
                dir.file_name().unwrap().to_string_lossy(),
                serving.label,
                serving.docroot.display()
            );
        }
    }
}
