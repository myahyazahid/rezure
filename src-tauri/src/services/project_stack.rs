//! What a project is built on and what it needs to run — framework versions,
//! PHP, Node.js, package manager, database — read from its own files and set
//! against what Rezure has installed and running.
//!
//! Only files are read; nothing is executed. Every source is one a developer
//! would recognise and can fix:
//!
//! - **PHP**: `require.php` in `composer.json`; the minimum Composer wrote to
//!   `vendor/composer/platform_check.php` for the *installed* packages (which
//!   catches `composer.json` saying `^8.1` while the lock has moved on to
//!   8.2); WordPress's own `$required_php_version`.
//! - **Node.js**: `engines.node`; `.nvmrc` / `.node-version` / `volta.node`;
//!   and `engines.node` of the framework and build tool packages the project
//!   uses (Next.js, Vite, Angular, …), read from `node_modules` or, before an
//!   install, from `package-lock.json`. Only those packages — an old lint
//!   plugin declaring `^14` shouldn't flag a project that runs fine.
//! - **Package manager**: the `packageManager` field, or the lockfile.
//! - **Database**: `DB_CONNECTION` / `DATABASE_URL` / CodeIgniter's
//!   `DBDriver` in `.env` (or `.env.example` before one exists), the
//!   framework's database config, Prisma's datasource, then driver
//!   dependencies (`pg`, `mysql2`, …). WordPress also states a MySQL minimum.
//!   Most frameworks don't state a database *version*, so none is guessed.
//!
//! A requirement Rezure can't parse is reported as unreadable, never as a
//! mismatch.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::doctor::env_value;
use super::frameworks::ServeProfile;
use super::version_constraint::{parse_parts, parse_version, Constraint, Dialect};
use super::{mssql_localdb, node, php, postgres, ServiceInfo, ServiceStatus};
use crate::db::projects::ProjectInfo;

/// Lockfiles past this are skipped rather than parsed on every listing.
const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;

/// Composer packages worth naming, with their label.
const COMPOSER_FRAMEWORKS: &[(&str, &str)] = &[
    ("laravel/framework", "Laravel"),
    ("laravel/lumen-framework", "Lumen"),
    ("codeigniter4/framework", "CodeIgniter 4"),
    ("symfony/framework-bundle", "Symfony"),
    ("yiisoft/yii2", "Yii 2"),
    ("cakephp/cakephp", "CakePHP"),
    ("slim/slim", "Slim"),
    ("laminas/laminas-mvc", "Laminas"),
    ("filament/filament", "Filament"),
    ("livewire/livewire", "Livewire"),
    ("inertiajs/inertia-laravel", "Inertia"),
];

/// npm packages worth naming, with their label — also the ones whose
/// `engines.node` counts as the project's own requirement.
const NODE_FRAMEWORKS: &[(&str, &str)] = &[
    ("next", "Next.js"),
    ("nuxt", "Nuxt"),
    ("@sveltejs/kit", "SvelteKit"),
    ("astro", "Astro"),
    ("@remix-run/react", "Remix"),
    ("@react-router/dev", "React Router"),
    ("gatsby", "Gatsby"),
    ("@angular/core", "Angular"),
    ("@nestjs/core", "NestJS"),
    ("express", "Express"),
    ("vite", "Vite"),
    ("react", "React"),
    ("vue", "Vue"),
    ("svelte", "Svelte"),
    ("tailwindcss", "Tailwind CSS"),
];

/// Build tools that aren't worth a chip but do refuse old Node.js.
const NODE_TOOLS: &[&str] = &[
    "laravel-vite-plugin",
    "@angular/cli",
    "@nestjs/cli",
    "react-scripts",
];

/// Everything Rezure could tell about one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStack {
    pub frameworks: Vec<FrameworkVersion>,
    pub php: Option<RuntimeRequirement>,
    pub node: Option<RuntimeRequirement>,
    pub package_manager: Option<PackageManager>,
    pub databases: Vec<DatabaseNeed>,
    /// Something here keeps the project from running as it is: a runtime
    /// that doesn't fit, or a database Rezure doesn't have.
    pub needs_attention: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkVersion {
    pub name: String,
    /// `11.9.2` when installed, the declared range (`^15.0`) otherwise.
    pub version: Option<String>,
    /// False when `version` is only what the manifest asks for.
    pub installed: bool,
}

/// What a project asks of a runtime (PHP, Node.js), set against the version
/// serving it and the ones installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRequirement {
    /// The constraint to show — the one the serving version fails, or the
    /// first one read when it fits — as written (`^8.2`, `>=18.18.0`).
    pub constraint: String,
    /// Where it was read: `composer.json`, `Next.js 15.1.0`, `.nvmrc (20.11.0)`.
    pub source: String,
    /// False when no requirement could be parsed; nothing below is known.
    pub readable: bool,
    /// Every requirement read, in reading order — `composer.json` may say
    /// `^8.3` while the installed packages already need 8.4, and both are
    /// worth seeing.
    pub stated: Vec<StatedRequirement>,
    /// The version serving the project now: its pin while installed, the
    /// global active version otherwise. `None` with nothing installed.
    pub serving: Option<String>,
    /// `None` when unknown — nothing installed, or unreadable.
    pub fits: Option<bool>,
    /// Installed versions that fit every requirement, newest first.
    pub fitting: Vec<String>,
    /// What to pin to, while the serving version doesn't fit.
    pub suggested: Option<String>,
    /// The oldest line that fits — `8.2` for PHP, `20` for Node.js — for
    /// "install …" when nothing installed does.
    pub minimum_branch: Option<String>,
}

/// One requirement as the project states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatedRequirement {
    pub constraint: String,
    pub source: String,
    /// Whether the serving version meets this one; `None` when unknown or
    /// unreadable.
    pub fits: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManager {
    /// `npm`, `pnpm`, `yarn` or `bun`.
    pub name: String,
    /// From `packageManager` (`pnpm@9.12.0`), when the project pins one.
    pub version: Option<String>,
    /// `packageManager in package.json`, `pnpm-lock.yaml`, ...
    pub source: String,
    /// `engines.<name>`, when stated.
    pub constraint: Option<String>,
    /// npm's version, bundled with the Node.js serving the project. Unknown
    /// for the others — they're installed separately.
    pub current: Option<String>,
    pub fits: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DbEngine {
    Mysql,
    Mariadb,
    Postgres,
    Sqlserver,
    Sqlite,
    Mongodb,
}

impl DbEngine {
    fn label(self) -> &'static str {
        match self {
            DbEngine::Mysql => "MySQL",
            DbEngine::Mariadb => "MariaDB",
            DbEngine::Postgres => "PostgreSQL",
            DbEngine::Sqlserver => "SQL Server",
            DbEngine::Sqlite => "SQLite",
            DbEngine::Mongodb => "MongoDB",
        }
    }

    /// The Rezure service that provides it; `None` for SQLite (a file,
    /// nothing to run) and MongoDB (not offered).
    fn service_id(self) -> Option<&'static str> {
        match self {
            DbEngine::Mysql | DbEngine::Mariadb => Some("mariadb"),
            DbEngine::Postgres => Some(postgres::SERVICE_ID),
            DbEngine::Sqlserver => Some(mssql_localdb::SERVICE_ID),
            DbEngine::Sqlite | DbEngine::Mongodb => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseNeed {
    pub engine: DbEngine,
    pub label: String,
    /// `.env (DB_CONNECTION=mysql)`, `prisma/schema.prisma`, `dependency pg`.
    pub source: String,
    /// A minimum the project states (only WordPress does).
    pub minimum: Option<String>,
    /// False for an engine Rezure doesn't offer (MongoDB).
    pub supported: bool,
    /// The Rezure service that would serve it; `None` when there's nothing
    /// to run (SQLite) or nothing Rezure offers.
    pub provider: Option<DbProvider>,
    /// `minimum` against the provider's version; `None` when either is
    /// unknown.
    pub fits: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbProvider {
    pub service_id: String,
    /// The service card's name — `MariaDB` or `MySQL` for the same service,
    /// depending on the active data directory.
    pub name: String,
    pub version: Option<String>,
    pub installed: bool,
    pub running: bool,
}

/// What Rezure has, gathered once for the whole project list.
pub struct Installed {
    pub php: Vec<String>,
    pub php_active: Option<String>,
    /// Version and its bundled npm, newest first.
    pub node: Vec<(String, Option<String>)>,
    pub node_active: Option<String>,
    pub services: Vec<ServiceInfo>,
}

impl Installed {
    pub fn gather(services: Vec<ServiceInfo>) -> Self {
        let php = php::list();
        let node = node::list();
        Installed {
            php_active: php.iter().find(|v| v.active).map(|v| v.version.clone()),
            php: php.into_iter().map(|v| v.version).collect(),
            node_active: node.iter().find(|v| v.active).map(|v| v.version.clone()),
            node: node.into_iter().map(|v| (v.version, v.npm)).collect(),
            services,
        }
    }

    fn node_versions(&self) -> Vec<String> {
        self.node.iter().map(|(v, _)| v.clone()).collect()
    }
}

/// Fills in `stack_info` for every project that isn't missing. Call after the
/// pins are known: what serves a project depends on them.
pub fn annotate(projects: &mut [ProjectInfo], installed: &Installed) {
    for project in projects.iter_mut().filter(|p| !p.missing) {
        project.stack_info = read(project, installed);
    }
}

/// One requirement as read, before evaluation.
struct Requirement {
    text: String,
    source: String,
    dialect: Dialect,
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn read_json(path: &Path) -> Option<Value> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_JSON_BYTES {
        return None;
    }
    serde_json::from_str(&read_text(path)?).ok()
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |v, key| v.get(key))?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The quoted value after `needle` on the same line: `'3.1.13'` from
/// `define('CI_VERSION', '3.1.13');` with needle `'CI_VERSION'`.
fn quoted_after(text: &str, needle: &str) -> Option<String> {
    let start = text.find(needle)? + needle.len();
    let line = text[start..].lines().next()?;
    let open = line.find(['\'', '"'])?;
    let quote = line[open..].chars().next()?;
    let rest = &line[open + 1..];
    let close = rest.find(quote)?;
    Some(rest[..close].trim().to_string()).filter(|s| !s.is_empty())
}

fn strip_v(version: &str) -> String {
    version.trim().trim_start_matches(['v', 'V']).to_string()
}

fn read(project: &ProjectInfo, installed: &Installed) -> Option<ProjectStack> {
    let dir = Path::new(&project.path);
    let composer_json = read_json(&dir.join("composer.json"));
    let package_json = read_json(&dir.join("package.json"));
    let wordpress = Wordpress::read(dir);

    let mut frameworks = composer_frameworks(dir, composer_json.as_ref());
    if let Some(ci3) = codeigniter3_version(dir) {
        frameworks.push(FrameworkVersion {
            name: "CodeIgniter 3".to_string(),
            version: Some(ci3),
            installed: true,
        });
    }
    if let Some(wp) = &wordpress {
        frameworks.push(FrameworkVersion {
            name: "WordPress".to_string(),
            version: wp.version.clone(),
            installed: true,
        });
    }
    let node_packages = package_json
        .as_ref()
        .map(|pkg| NodePackages::read(dir, pkg));
    if let Some(packages) = &node_packages {
        frameworks.extend(packages.frameworks());
    }

    let php = (project.serve == ServeProfile::Php)
        .then(|| {
            let serving = serving_version(
                project.php_version.as_deref(),
                &installed.php,
                installed.php_active.as_deref(),
            );
            evaluate(
                php_requirements(dir, composer_json.as_ref(), wordpress.as_ref()),
                serving,
                &installed.php,
                Branch::MajorMinor,
            )
        })
        .flatten();

    let node_versions = installed.node_versions();
    let node_serving = serving_version(
        project.node_version.as_deref(),
        &node_versions,
        installed.node_active.as_deref(),
    );
    let node = package_json.as_ref().and_then(|pkg| {
        evaluate(
            node_requirements(dir, pkg, node_packages.as_ref()),
            node_serving,
            &node_versions,
            Branch::Major,
        )
    });
    let npm_current = node_serving.and_then(|serving| {
        installed
            .node
            .iter()
            .find(|(v, _)| v == serving)
            .and_then(|(_, npm)| npm.clone())
    });
    let package_manager = package_json
        .as_ref()
        .map(|pkg| package_manager(dir, pkg, npm_current));

    let databases = database_needs(
        dir,
        package_json.as_ref(),
        wordpress.as_ref(),
        &installed.services,
    );

    let needs_attention = php.as_ref().is_some_and(|r| r.fits == Some(false))
        || node.as_ref().is_some_and(|r| r.fits == Some(false))
        || package_manager
            .as_ref()
            .is_some_and(|p| p.fits == Some(false))
        || databases.iter().any(|db| {
            !db.supported
                || db.fits == Some(false)
                || db.provider.as_ref().is_some_and(|p| !p.installed)
        });

    let empty = frameworks.is_empty()
        && php.is_none()
        && node.is_none()
        && package_manager.is_none()
        && databases.is_empty();
    (!empty).then_some(ProjectStack {
        frameworks,
        php,
        node,
        package_manager,
        databases,
        needs_attention,
    })
}

/// The pin while it's installed, the active version otherwise — the same
/// fallback `php_pool` and the terminal launcher use.
fn serving_version<'a>(
    pinned: Option<&'a str>,
    installed: &[String],
    active: Option<&'a str>,
) -> Option<&'a str> {
    pinned
        .filter(|p| installed.iter().any(|v| v == p))
        .or(active)
}

// ---------------------------------------------------------------- PHP side

fn composer_frameworks(dir: &Path, composer_json: Option<&Value>) -> Vec<FrameworkVersion> {
    let lock = read_json(&dir.join("composer.lock"));
    let locked = |name: &str| -> Option<String> {
        lock.as_ref()?
            .get("packages")?
            .as_array()?
            .iter()
            .find(|p| p.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|p| p.get("version")?.as_str())
            .map(strip_v)
    };
    COMPOSER_FRAMEWORKS
        .iter()
        .filter_map(|(package, label)| {
            if let Some(version) = locked(package) {
                return Some(FrameworkVersion {
                    name: label.to_string(),
                    version: Some(version),
                    installed: true,
                });
            }
            let declared = composer_json.and_then(|c| str_at(c, &["require", package]))?;
            Some(FrameworkVersion {
                name: label.to_string(),
                version: Some(declared.to_string()),
                installed: false,
            })
        })
        .collect()
}

fn codeigniter3_version(dir: &Path) -> Option<String> {
    let core = read_text(&dir.join("system").join("core").join("CodeIgniter.php"))?;
    quoted_after(&core, "'CI_VERSION'")
}

/// What `wp-includes/version.php` states.
struct Wordpress {
    version: Option<String>,
    required_php: Option<String>,
    required_mysql: Option<String>,
}

impl Wordpress {
    fn read(dir: &Path) -> Option<Wordpress> {
        let text = read_text(&dir.join("wp-includes").join("version.php"))?;
        Some(Wordpress {
            version: quoted_after(&text, "$wp_version"),
            required_php: quoted_after(&text, "$required_php_version"),
            required_mysql: quoted_after(&text, "$required_mysql_version"),
        })
    }
}

fn php_requirements(
    dir: &Path,
    composer_json: Option<&Value>,
    wordpress: Option<&Wordpress>,
) -> Vec<Requirement> {
    let mut found = Vec::new();
    if let Some(text) = composer_json.and_then(|c| str_at(c, &["require", "php"])) {
        found.push(Requirement {
            text: text.to_string(),
            source: "composer.json".to_string(),
            dialect: Dialect::Composer,
        });
    }
    let platform_check = dir
        .join("vendor")
        .join("composer")
        .join("platform_check.php");
    if let Some(minimum) = read_text(&platform_check).and_then(|t| platform_check_minimum(&t)) {
        found.push(Requirement {
            text: minimum,
            source: "installed Composer packages (vendor/composer/platform_check.php)".to_string(),
            dialect: Dialect::Composer,
        });
    }
    if let Some(wp) = wordpress {
        if let Some(minimum) = &wp.required_php {
            found.push(Requirement {
                text: format!(">= {minimum}"),
                source: format!(
                    "WordPress {} (wp-includes/version.php)",
                    wp.version.as_deref().unwrap_or("")
                )
                .replace("  ", " "),
                dialect: Dialect::Composer,
            });
        }
    }
    found
}

/// The lowest version `platform_check.php` lets through, as `>= 8.2.0`.
/// Composer writes it as `PHP_VERSION_ID >= 80200`.
fn platform_check_minimum(text: &str) -> Option<String> {
    const NEEDLE: &str = "PHP_VERSION_ID >= ";
    let at = text.find(NEEDLE)? + NEEDLE.len();
    let id: u64 = text[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    Some(format!(
        ">= {}.{}.{}",
        id / 10000,
        (id / 100) % 100,
        id % 100
    ))
}

// --------------------------------------------------------------- Node side

/// The framework and tool packages a project depends on, with what's known
/// of each from `node_modules` or `package-lock.json`.
struct NodePackages {
    /// (package, label or `None` for a tool, installed version, declared
    /// range, `engines.node`)
    found: Vec<NodePackage>,
}

struct NodePackage {
    name: &'static str,
    label: Option<&'static str>,
    installed: Option<String>,
    declared: Option<String>,
    engines_node: Option<String>,
}

impl NodePackages {
    fn read(dir: &Path, package_json: &Value) -> NodePackages {
        let declared = |name: &str| -> Option<String> {
            ["dependencies", "devDependencies"]
                .iter()
                .find_map(|section| str_at(package_json, &[section, name]))
                .map(str::to_string)
        };
        // Parsed at most once, and only when something isn't installed.
        let mut lock: Option<Option<Value>> = None;
        let mut found = Vec::new();
        let candidates = NODE_FRAMEWORKS
            .iter()
            .map(|(name, label)| (*name, Some(*label)))
            .chain(NODE_TOOLS.iter().map(|name| (*name, None)));
        for (name, label) in candidates {
            let Some(range) = declared(name) else {
                continue;
            };
            let mut manifest = read_json(&dir.join("node_modules").join(name).join("package.json"));
            if manifest.is_none() {
                let lock = lock.get_or_insert_with(|| read_json(&dir.join("package-lock.json")));
                manifest = lock
                    .as_ref()
                    .and_then(|l| l.get("packages")?.get(format!("node_modules/{name}")))
                    .cloned();
            }
            found.push(NodePackage {
                name,
                label,
                installed: manifest
                    .as_ref()
                    .and_then(|m| str_at(m, &["version"]))
                    .map(strip_v),
                declared: Some(range),
                engines_node: manifest
                    .as_ref()
                    .and_then(|m| str_at(m, &["engines", "node"]))
                    .map(str::to_string),
            });
        }
        NodePackages { found }
    }

    fn frameworks(&self) -> Vec<FrameworkVersion> {
        self.found
            .iter()
            .filter_map(|p| {
                Some(FrameworkVersion {
                    name: p.label?.to_string(),
                    installed: p.installed.is_some(),
                    version: p.installed.clone().or_else(|| p.declared.clone()),
                })
            })
            .collect()
    }
}

/// `.nvmrc`'s `v20.11.0` or `20` as "any 20.x": these files name a version
/// to use, and Rezure installs whole release lines, not exact patches.
/// `lts/*` and `node` say nothing checkable and are skipped.
fn preferred_line(raw: &str) -> Option<String> {
    let parts = parse_parts(raw)?;
    Some(format!("{}.x", parts[0]))
}

fn node_requirements(
    dir: &Path,
    package_json: &Value,
    packages: Option<&NodePackages>,
) -> Vec<Requirement> {
    let mut found = Vec::new();
    if let Some(text) = str_at(package_json, &["engines", "node"]) {
        found.push(Requirement {
            text: text.to_string(),
            source: "package.json (engines.node)".to_string(),
            dialect: Dialect::Npm,
        });
    }
    for file in [".nvmrc", ".node-version"] {
        let Some(raw) = read_text(&dir.join(file)) else {
            continue;
        };
        let raw = raw.lines().next().unwrap_or_default().trim().to_string();
        if let Some(line) = preferred_line(&raw) {
            found.push(Requirement {
                text: line,
                source: format!("{file} ({raw})"),
                dialect: Dialect::Npm,
            });
        }
    }
    if let Some(raw) = str_at(package_json, &["volta", "node"]) {
        if let Some(line) = preferred_line(raw) {
            found.push(Requirement {
                text: line,
                source: format!("package.json (volta.node {raw})"),
                dialect: Dialect::Npm,
            });
        }
    }
    for package in packages.map(|p| p.found.as_slice()).unwrap_or_default() {
        if let Some(engines) = &package.engines_node {
            let name = package.label.unwrap_or(package.name);
            found.push(Requirement {
                text: engines.clone(),
                source: match &package.installed {
                    Some(version) => format!("{name} {version}"),
                    None => name.to_string(),
                },
                dialect: Dialect::Npm,
            });
        }
    }
    found
}

fn package_manager(
    dir: &Path,
    package_json: &Value,
    npm_current: Option<String>,
) -> PackageManager {
    let pinned = str_at(package_json, &["packageManager"]).map(|field| {
        let (name, version) = field.split_once('@').unwrap_or((field, ""));
        let version = version.split('+').next().unwrap_or_default();
        (
            name.to_string(),
            Some(version.to_string()).filter(|v| !v.is_empty()),
        )
    });
    let (name, version, source) = match pinned {
        Some((name, version)) => (name, version, "packageManager in package.json".to_string()),
        None => {
            let by_lock = [
                ("pnpm-lock.yaml", "pnpm"),
                ("yarn.lock", "yarn"),
                ("bun.lock", "bun"),
                ("bun.lockb", "bun"),
                ("package-lock.json", "npm"),
            ]
            .into_iter()
            .find(|(file, _)| dir.join(file).is_file());
            match by_lock {
                Some((file, name)) => (name.to_string(), None, file.to_string()),
                None => ("npm".to_string(), None, "no lockfile yet".to_string()),
            }
        }
    };
    let constraint = str_at(package_json, &["engines", name.as_str()]).map(str::to_string);
    let current = (name == "npm").then_some(npm_current).flatten();
    let fits = match (&constraint, &current) {
        (Some(c), Some(v)) => Constraint::parse(c, Dialect::Npm)
            .zip(parse_version(v))
            .map(|(c, v)| c.allows(v)),
        _ => None,
    };
    PackageManager {
        name,
        version,
        source,
        constraint,
        current,
        fits,
    }
}

// ------------------------------------------------------------ requirements

/// How "the oldest line that fits" is counted.
#[derive(Clone, Copy)]
enum Branch {
    /// PHP: `8.2`.
    MajorMinor,
    /// Node.js: `20`.
    Major,
}

fn minimum_branch(constraints: &[Constraint], branch: Branch) -> Option<String> {
    let fits_all = |v| constraints.iter().all(|c| c.allows(v));
    match branch {
        // The line's latest patch stands for the line, so `>= 8.2.5` still
        // counts 8.2.
        Branch::MajorMinor => (5..=9)
            .flat_map(|major| (0..=9).map(move |minor| (major, minor)))
            .find(|&(major, minor)| fits_all((major, minor, 999)))
            .map(|(major, minor)| format!("{major}.{minor}")),
        Branch::Major => (4..=40)
            .find(|&major| fits_all((major, 999, 999)))
            .map(|major| major.to_string()),
    }
}

fn evaluate(
    requirements: Vec<Requirement>,
    serving: Option<&str>,
    installed: &[String],
    branch: Branch,
) -> Option<RuntimeRequirement> {
    let first = requirements.first()?;
    let serving_version = serving.and_then(parse_version);
    let stated: Vec<StatedRequirement> = requirements
        .iter()
        .map(|r| StatedRequirement {
            constraint: r.text.clone(),
            source: r.source.clone(),
            fits: Constraint::parse(&r.text, r.dialect)
                .zip(serving_version)
                .map(|(c, v)| c.allows(v)),
        })
        .collect();
    let parsed: Vec<(&Requirement, Constraint)> = requirements
        .iter()
        .filter_map(|r| Some((r, Constraint::parse(&r.text, r.dialect)?)))
        .collect();

    if parsed.is_empty() {
        return Some(RuntimeRequirement {
            constraint: first.text.clone(),
            source: first.source.clone(),
            readable: false,
            stated,
            serving: serving.map(str::to_string),
            fits: None,
            fitting: Vec::new(),
            suggested: None,
            minimum_branch: None,
        });
    }

    let fits_all = |version: &str| -> Option<bool> {
        let v = parse_version(version)?;
        Some(parsed.iter().all(|(_, c)| c.allows(v)))
    };
    let fitting: Vec<String> = installed
        .iter()
        .filter(|v| fits_all(v) == Some(true))
        .cloned()
        .collect();
    let fits = serving.and_then(fits_all);

    // Report the requirement the serving version fails, in reading order —
    // the project's own manifest before what its packages need.
    let blocking =
        serving_version.and_then(|v| parsed.iter().find(|(_, c)| !c.allows(v)).map(|(r, _)| *r));
    let shown = blocking.unwrap_or(parsed[0].0);
    let constraints: Vec<Constraint> = parsed.iter().map(|(_, c)| c.clone()).collect();

    Some(RuntimeRequirement {
        constraint: shown.text.clone(),
        source: shown.source.clone(),
        readable: true,
        stated,
        serving: serving.map(str::to_string),
        suggested: (fits == Some(false))
            .then(|| fitting.first().cloned())
            .flatten(),
        fits,
        fitting,
        minimum_branch: minimum_branch(&constraints, branch),
    })
}

// ---------------------------------------------------------------- database

fn engine_from_laravel(value: &str) -> Option<DbEngine> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mysql" => Some(DbEngine::Mysql),
        "mariadb" => Some(DbEngine::Mariadb),
        "pgsql" | "postgres" | "postgresql" => Some(DbEngine::Postgres),
        "sqlsrv" => Some(DbEngine::Sqlserver),
        "sqlite" => Some(DbEngine::Sqlite),
        "mongodb" => Some(DbEngine::Mongodb),
        _ => None,
    }
}

fn engine_from_codeigniter(value: &str) -> Option<DbEngine> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mysqli" | "mysql" | "pdo_mysql" => Some(DbEngine::Mysql),
        "postgre" | "pgsql" | "pdo_pgsql" => Some(DbEngine::Postgres),
        "sqlsrv" | "pdo_sqlsrv" => Some(DbEngine::Sqlserver),
        "sqlite3" | "sqlite" => Some(DbEngine::Sqlite),
        _ => None,
    }
}

/// The scheme of a `DATABASE_URL`: `postgresql://…` → PostgreSQL.
fn engine_from_url(url: &str) -> Option<DbEngine> {
    let scheme = url.split(':').next()?.to_ascii_lowercase();
    match scheme.as_str() {
        "mysql" | "mysql2" => Some(DbEngine::Mysql),
        "mariadb" => Some(DbEngine::Mariadb),
        "postgres" | "postgresql" | "pgsql" => Some(DbEngine::Postgres),
        "sqlserver" | "mssql" => Some(DbEngine::Sqlserver),
        "sqlite" | "file" => Some(DbEngine::Sqlite),
        "mongodb" | "mongodb+srv" => Some(DbEngine::Mongodb),
        _ => None,
    }
}

/// Prisma's `datasource db { provider = "postgresql" }`.
fn engine_from_prisma(schema: &str) -> Option<DbEngine> {
    let datasource = &schema[schema.find("datasource")?..];
    let provider = quoted_after(datasource, "provider")?;
    match provider.as_str() {
        "cockroachdb" => Some(DbEngine::Postgres),
        other => engine_from_url(&format!("{other}:")),
    }
}

fn database_needs(
    dir: &Path,
    package_json: Option<&Value>,
    wordpress: Option<&Wordpress>,
    services: &[ServiceInfo],
) -> Vec<DatabaseNeed> {
    let mut found: Vec<(DbEngine, String, Option<String>)> = Vec::new();

    if let Some(wp) = wordpress {
        found.push((
            DbEngine::Mysql,
            "WordPress".to_string(),
            wp.required_mysql.clone(),
        ));
    }

    // `.env` first; `.env.example` says what's intended before one exists.
    let env = [".env", ".env.example"]
        .into_iter()
        .find_map(|file| Some((file, read_text(&dir.join(file))?)));
    if let Some((file, env)) = &env {
        if let Some(value) = env_value(env, "DB_CONNECTION") {
            if let Some(engine) = engine_from_laravel(&value) {
                found.push((engine, format!("{file} (DB_CONNECTION={value})"), None));
            }
        }
        if let Some(value) = env_value(env, "database.default.DBDriver") {
            if let Some(engine) = engine_from_codeigniter(&value) {
                found.push((engine, format!("{file} (database.default.DBDriver)"), None));
            }
        }
        if let Some(value) = env_value(env, "DATABASE_URL") {
            if let Some(engine) = engine_from_url(&value) {
                found.push((engine, format!("{file} (DATABASE_URL)"), None));
            }
        }
    }

    if found.is_empty() {
        let configs = [
            (["application", "config", "database.php"], "['dbdriver']"),
            (["app", "Config", "Database.php"], "'DBDriver'"),
        ];
        for (path, needle) in configs {
            let file = path.iter().fold(dir.to_path_buf(), |p, part| p.join(part));
            if let Some(engine) = read_text(&file)
                .and_then(|t| quoted_after(&t, needle))
                .and_then(|v| engine_from_codeigniter(&v))
            {
                found.push((engine, path.join("/"), None));
            }
        }
    }

    if let Some(engine) = read_text(&dir.join("prisma").join("schema.prisma"))
        .and_then(|schema| engine_from_prisma(&schema))
    {
        found.push((engine, "prisma/schema.prisma".to_string(), None));
    }

    if found.is_empty() {
        if let Some(pkg) = package_json {
            let drivers = [
                ("mysql2", DbEngine::Mysql),
                ("mysql", DbEngine::Mysql),
                ("pg", DbEngine::Postgres),
                ("postgres", DbEngine::Postgres),
                ("mssql", DbEngine::Sqlserver),
                ("tedious", DbEngine::Sqlserver),
                ("mongoose", DbEngine::Mongodb),
                ("mongodb", DbEngine::Mongodb),
                ("better-sqlite3", DbEngine::Sqlite),
                ("sqlite3", DbEngine::Sqlite),
            ];
            for (name, engine) in drivers {
                let declared = ["dependencies", "devDependencies"]
                    .iter()
                    .any(|section| str_at(pkg, &[section, name]).is_some());
                if declared {
                    found.push((engine, format!("dependency {name}"), None));
                }
            }
        }
    }

    let mut needs: Vec<DatabaseNeed> = Vec::new();
    for (engine, source, minimum) in found {
        if needs.iter().any(|n| n.engine == engine) {
            continue;
        }
        let provider = engine
            .service_id()
            .map(|id| match services.iter().find(|s| s.id == id) {
                Some(service) => DbProvider {
                    service_id: id.to_string(),
                    name: service.name.clone(),
                    version: Some(service.version.clone()).filter(|v| !v.is_empty()),
                    installed: true,
                    running: matches!(
                        service.status,
                        ServiceStatus::Running | ServiceStatus::Starting
                    ),
                },
                None => DbProvider {
                    service_id: id.to_string(),
                    name: engine.label().to_string(),
                    version: None,
                    installed: false,
                    running: false,
                },
            });
        let fits = minimum
            .as_deref()
            .and_then(parse_version)
            .zip(
                provider
                    .as_ref()
                    .and_then(|p| p.version.as_deref())
                    .and_then(parse_version),
            )
            .map(|(min, have)| have >= min);
        needs.push(DatabaseNeed {
            engine,
            label: engine.label().to_string(),
            source,
            minimum,
            supported: engine != DbEngine::Mongodb,
            provider,
            fits,
        });
    }
    needs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str, source: &str, dialect: Dialect) -> Requirement {
        Requirement {
            text: text.to_string(),
            source: source.to_string(),
            dialect,
        }
    }

    fn php_installed() -> Vec<String> {
        ["8.4.10", "8.3.33", "8.1.30"].map(str::to_string).to_vec()
    }

    #[test]
    fn a_php_mismatch_suggests_the_newest_installed_fit() {
        let r = evaluate(
            vec![req("^8.2", "composer.json", Dialect::Composer)],
            Some("8.1.30"),
            &php_installed(),
            Branch::MajorMinor,
        )
        .unwrap();
        assert_eq!(r.fits, Some(false));
        assert_eq!(r.fitting, vec!["8.4.10", "8.3.33"]);
        assert_eq!(r.suggested.as_deref(), Some("8.4.10"));
        assert_eq!(r.minimum_branch.as_deref(), Some("8.2"));
    }

    #[test]
    fn a_fit_suggests_nothing() {
        let r = evaluate(
            vec![req("^8.2", "composer.json", Dialect::Composer)],
            Some("8.3.33"),
            &php_installed(),
            Branch::MajorMinor,
        )
        .unwrap();
        assert_eq!(r.fits, Some(true));
        assert_eq!(r.suggested, None);
    }

    /// `composer.json` still says `^8.1`, but the installed packages need
    /// 8.2 — the platform check is what's reported.
    #[test]
    fn the_failing_requirement_is_the_one_reported() {
        let r = evaluate(
            vec![
                req("^8.1", "composer.json", Dialect::Composer),
                req(">= 8.2.0", "installed Composer packages", Dialect::Composer),
            ],
            Some("8.1.30"),
            &php_installed(),
            Branch::MajorMinor,
        )
        .unwrap();
        assert_eq!(r.constraint, ">= 8.2.0");
        assert_eq!(r.source, "installed Composer packages");
        let fits: Vec<_> = r.stated.iter().map(|s| s.fits).collect();
        assert_eq!(fits, vec![Some(true), Some(false)]);
    }

    #[test]
    fn nothing_installed_fits_points_at_a_line_to_install() {
        let r = evaluate(
            vec![req("7.4.*", "composer.json", Dialect::Composer)],
            Some("8.1.30"),
            &php_installed(),
            Branch::MajorMinor,
        )
        .unwrap();
        assert!(r.fitting.is_empty());
        assert_eq!(r.suggested, None);
        assert_eq!(r.minimum_branch.as_deref(), Some("7.4"));
    }

    #[test]
    fn node_lines_are_counted_by_major() {
        let r = evaluate(
            vec![req("^20.19.0 || >=22.12.0", "Vite 7.0.0", Dialect::Npm)],
            Some("18.20.4"),
            &["22.11.0".to_string(), "18.20.4".to_string()],
            Branch::Major,
        )
        .unwrap();
        assert_eq!(r.fits, Some(false));
        assert!(r.fitting.is_empty());
        assert_eq!(r.minimum_branch.as_deref(), Some("20"));
        assert_eq!(r.source, "Vite 7.0.0");
    }

    #[test]
    fn unreadable_requirements_are_dropped_not_flagged() {
        let only_nonsense = evaluate(
            vec![req("latest", "composer.json", Dialect::Composer)],
            Some("8.1.30"),
            &php_installed(),
            Branch::MajorMinor,
        )
        .unwrap();
        assert!(!only_nonsense.readable);
        assert_eq!(only_nonsense.fits, None);

        let mixed = evaluate(
            vec![
                req("latest", "composer.json", Dialect::Composer),
                req(">= 8.2.0", "platform", Dialect::Composer),
            ],
            Some("8.3.33"),
            &php_installed(),
            Branch::MajorMinor,
        )
        .unwrap();
        assert!(mixed.readable);
        assert_eq!(mixed.fits, Some(true));
    }

    #[test]
    fn no_runtime_installed_is_unknown() {
        let r = evaluate(
            vec![req("^8.2", "composer.json", Dialect::Composer)],
            None,
            &[],
            Branch::MajorMinor,
        )
        .unwrap();
        assert_eq!(r.fits, None);
    }

    #[test]
    fn files_are_read_as_their_tools_write_them() {
        let check = "<?php\nif (!(PHP_VERSION_ID >= 80200)) {\n    $issues[] = 'Your Composer dependencies require a PHP version \">= 8.2.0\".';\n}\n";
        assert_eq!(platform_check_minimum(check).as_deref(), Some(">= 8.2.0"));
        assert_eq!(
            quoted_after("define('CI_VERSION', '3.1.13');", "'CI_VERSION'").as_deref(),
            Some("3.1.13")
        );
        let wp = "$wp_version = '6.5.2';\n$required_php_version = '7.2.24';\n$required_mysql_version = '5.5.5';";
        assert_eq!(
            quoted_after(wp, "$required_php_version").as_deref(),
            Some("7.2.24")
        );
        assert_eq!(
            quoted_after("$db['default']['dbdriver'] = 'mysqli';", "['dbdriver']").as_deref(),
            Some("mysqli")
        );
        assert_eq!(preferred_line("v20.11.0").as_deref(), Some("20.x"));
        assert_eq!(preferred_line("lts/iron"), None);
    }

    #[test]
    fn database_engines_are_read_from_each_framework_s_spelling() {
        assert_eq!(engine_from_laravel("pgsql"), Some(DbEngine::Postgres));
        assert_eq!(engine_from_codeigniter("MySQLi"), Some(DbEngine::Mysql));
        assert_eq!(engine_from_codeigniter("Postgre"), Some(DbEngine::Postgres));
        assert_eq!(
            engine_from_url("postgresql://u:p@localhost:5432/app"),
            Some(DbEngine::Postgres)
        );
        assert_eq!(
            engine_from_url("mongodb+srv://cluster/app"),
            Some(DbEngine::Mongodb)
        );
        let prisma = "generator client {\n  provider = \"prisma-client-js\"\n}\n\ndatasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n";
        assert_eq!(engine_from_prisma(prisma), Some(DbEngine::Postgres));
    }

    #[test]
    fn the_package_manager_comes_from_the_field_then_the_lockfile() {
        let dir = std::env::temp_dir().join(format!("rezure-stack-pm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();

        let field: Value =
            serde_json::from_str(r#"{"packageManager":"yarn@4.5.1+sha512.abc"}"#).unwrap();
        let pm = package_manager(&dir, &field, None);
        assert_eq!(
            (pm.name.as_str(), pm.version.as_deref()),
            ("yarn", Some("4.5.1"))
        );

        let none: Value = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(package_manager(&dir, &none, None).name, "pnpm");

        let npm: Value = serde_json::from_str(r#"{"engines":{"npm":">=10"}}"#).unwrap();
        std::fs::remove_file(dir.join("pnpm-lock.yaml")).unwrap();
        let pm = package_manager(&dir, &npm, Some("9.8.1".to_string()));
        assert_eq!(pm.fits, Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What every project on this machine reads as, without services or
    /// pins. Run with
    /// `cargo test --lib services::project_stack::tests::print_stacks -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_stacks() {
        let mut projects = crate::services::projects::scan_projects().unwrap();
        annotate(&mut projects, &Installed::gather(Vec::new()));
        for project in projects {
            println!(
                "== {} ({})
{:#?}",
                project.name, project.path, project.stack_info
            );
        }
    }
}
