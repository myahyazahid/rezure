//! Which framework a project folder holds — and so which of its folders
//! nginx serves.
//!
//! # Definitions are data
//!
//! Every framework Rezure recognises is an entry in a JSON registry rather
//! than a branch in code: an id and a label, the marker files that identify
//! it, a priority, the subfolder it's served from, and a serve profile.
//! Teaching Rezure a new framework is one more entry — in
//! `resources/frameworks.json` for everyone, or in the user's own
//! `etc/frameworks.json` for one machine — never a change here.
//!
//! What a definition can't do is shape the nginx config. Every vhost is
//! included by one `nginx.conf`, so one bad directive would take every site
//! down with it (the reason `projects::is_safe_domain` exists). A definition
//! only *picks* a [`ServeProfile`]; the template behind each one lives in
//! `services::vhosts`.
//!
//! # Detection only reads
//!
//! `scan_projects` runs for every project on every visit to the Projects
//! page, so detection never spawns anything: it checks for files and
//! folders, and reads `composer.json` and `package.json` once per project.
//!
//! # The highest priority wins, not the first check
//!
//! A PHP app that builds its CSS with npm has a `package.json`, and a
//! Laravel app on Inertia depends on Vue. Checking markers in a fixed order
//! got that wrong — a CodeIgniter 4 app with `sass` in its `package.json`
//! came out as "Node", and was served from the folder above its
//! `public\index.php`. So every definition that matches is weighed and the
//! highest `priority` wins: backend frameworks above frontend ones, above
//! the generic PHP / Node / static fallbacks.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::project_frameworks::FrameworkOverride;
use crate::utils::paths;

/// The definitions every install ships with.
const BUILTIN: &str = include_str!("../../resources/frameworks.json");

/// What a project is when no definition matches. Served from its own folder.
pub const UNKNOWN: &str = "unknown";
/// What a project is when the user gave it a docroot rather than a framework.
pub const CUSTOM: &str = "custom";

const UNKNOWN_LABEL: &str = "Unknown";
const CUSTOM_LABEL: &str = "Custom";

/// Deep enough for any real `any`/`all`/`not` combination, shallow enough
/// that an odd user definition can't make every scan slow.
const MAX_RULE_DEPTH: usize = 8;
const MAX_PRIORITY: u8 = 100;
const MAX_ID_LEN: usize = 64;
const MAX_LABEL_LEN: usize = 40;
const MAX_CATEGORY_LEN: usize = 24;
const MAX_PATH_LEN: usize = 200;

/// Where the pickers list a user's own definition that names no category.
const OWN_CATEGORY: &str = "Your frameworks";

/// How nginx serves a project. Only Rust can add one — see the module doc.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServeProfile {
    /// Files as they are, and everything else through `index.php` over
    /// FastCGI — what every PHP framework's front controller expects.
    #[default]
    Php,
    /// Every request passed on to a process listening on the project's own
    /// port — a dev server (Vite, Next, Nuxt, ...) or a Node app. There's no
    /// folder to serve, so a definition with this profile has no `docroot`.
    /// See `services::dev_servers`.
    Proxy,
}

/// How a dev server is made to listen on the port Rezure gave it. A closed
/// set for the same reason [`ServeProfile`] is: a definition picks one, and
/// only Rust decides what each one adds to the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortMode {
    /// `--port <p> --strictPort`, plus `PORT`. Vite otherwise moves to the
    /// next free port without a word when its own is taken, and the proxy is
    /// left pointing at nothing.
    Vite,
    /// `--port <p>`, plus `PORT` — Next, Nuxt, Astro, `ng serve`.
    Flag,
    /// Only the `PORT` environment variable — Create React App, and any Node
    /// app that reads `process.env.PORT`.
    Env,
    /// Nothing: the app listens on a port of its own, and the user tells
    /// Rezure which one.
    Fixed,
}

/// A `proxy` framework's defaults for running its dev server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevDefaults {
    /// The `package.json` script to run. Left out: `dev`, then `start`.
    #[serde(default)]
    pub script: Option<String>,
    /// Left out: [`PortMode::Env`].
    #[serde(default)]
    pub port: Option<PortMode>,
}

/// One test against a project folder. In JSON, an object with one key:
/// `{ "file": "artisan" }`, `{ "any": [ ... ] }`, `{ "not": { ... } }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rule {
    /// A file at this path, relative to the project.
    File(String),
    /// A folder at this path, relative to the project.
    Dir(String),
    /// A package in `composer.json`'s `require` or `require-dev`.
    Composer(String),
    /// A package in `package.json`'s `dependencies` or `devDependencies`.
    Npm(String),
    Any(Vec<Rule>),
    All(Vec<Rule>),
    Not(Box<Rule>),
}

/// One entry of a `frameworks.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkDef {
    /// Stable: project overrides and telemetry refer to it.
    pub id: String,
    pub label: String,
    /// 0–100, see the module doc.
    pub priority: u8,
    pub detect: Rule,
    /// The subfolder nginx serves; the project folder itself when absent.
    #[serde(default)]
    pub docroot: Option<String>,
    #[serde(default)]
    pub serve: ServeProfile,
    /// The heading the pickers list it under ("PHP", "JavaScript", ...).
    /// Purely for display; detection never reads it.
    #[serde(default)]
    pub category: Option<String>,
    /// How its dev server is run — only for `serve: "proxy"`.
    #[serde(default)]
    pub dev: Option<DevDefaults>,
}

/// Where a definition came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Builtin,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framework {
    pub def: FrameworkDef,
    pub source: Source,
}

/// The built-in definitions with the user's merged over them, plus what
/// was wrong with any that couldn't be used.
#[derive(Debug, Default)]
pub struct Registry {
    frameworks: Vec<Framework>,
    problems: Vec<String>,
}

/// How one project is served once detection and any override are settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serving {
    /// The effective framework id — a registry id, [`CUSTOM`] or [`UNKNOWN`].
    pub framework: String,
    /// What detection alone says, whatever the override.
    pub detected: String,
    pub label: String,
    /// Whether the user's choice, rather than detection, decided this.
    pub overridden: bool,
    /// The folder nginx serves, absolute. The project folder itself for a
    /// `proxy` project, which nginx serves no files of.
    pub docroot: PathBuf,
    pub serve: ServeProfile,
}

/// A framework as the pickers list it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkSummary {
    pub id: String,
    pub label: String,
    pub docroot: Option<String>,
    pub serve: ServeProfile,
    pub source: Source,
    /// The heading it's listed under — the definition's own `category`, or
    /// "Your frameworks" for a user definition without one.
    pub category: String,
}

/// Everything the frontend needs to offer a framework choice.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkCatalog {
    pub frameworks: Vec<FrameworkSummary>,
    /// Definitions that were skipped, and saved choices that no longer
    /// apply — each a sentence for the user.
    pub problems: Vec<String>,
}

impl Registry {
    pub fn frameworks(&self) -> &[Framework] {
        &self.frameworks
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn get(&self, id: &str) -> Option<&Framework> {
        self.frameworks.iter().find(|f| f.def.id == id)
    }

    /// The best-matching framework for `dir`, if any matches at all. Ties
    /// go to whichever comes first in the registry.
    pub fn detect(&self, dir: &Path) -> Option<&Framework> {
        let facts = Facts::read(dir);
        let mut best: Option<&Framework> = None;
        for framework in &self.frameworks {
            // Not even evaluated when it couldn't win: checks touch the disk.
            if best.is_some_and(|b| b.def.priority >= framework.def.priority) {
                continue;
            }
            if framework.def.detect.matches(&facts) {
                best = Some(framework);
            }
        }
        best
    }

    /// How `dir` is served given the user's `choice`, if they made one. A
    /// choice naming a framework that no longer exists is ignored — the
    /// project falls back to detection, and [`Registry::catalog`]'s caller
    /// reports it.
    pub fn serving(&self, dir: &Path, choice: Option<&FrameworkOverride>) -> Serving {
        let detected = self.detect(dir);
        let detected_id = detected.map_or(UNKNOWN, |f| f.def.id.as_str()).to_string();

        // A custom folder is always files served as they are: someone who
        // points Rezure at `dist\` wants that build, not a dev server.
        if let Some(FrameworkOverride::Docroot(sub)) = choice {
            return Serving {
                framework: CUSTOM.to_string(),
                detected: detected_id,
                label: CUSTOM_LABEL.to_string(),
                overridden: true,
                docroot: resolve_docroot(dir, Some(sub)),
                serve: ServeProfile::Php,
            };
        }

        let chosen = match choice {
            Some(FrameworkOverride::Framework(id)) => self.get(id),
            _ => None,
        };
        let effective = chosen.or(detected);
        Serving {
            framework: effective.map_or(UNKNOWN, |f| f.def.id.as_str()).to_string(),
            detected: detected_id,
            label: effective
                .map_or(UNKNOWN_LABEL, |f| f.def.label.as_str())
                .to_string(),
            overridden: chosen.is_some(),
            docroot: resolve_docroot(dir, effective.and_then(|f| f.def.docroot.as_deref())),
            serve: effective.map_or(ServeProfile::Php, |f| f.def.serve),
        }
    }

    /// The pickers' list, with this registry's own problems.
    ///
    /// Ordered so the frontend only has to start a heading wherever the
    /// category changes: the user's own uncategorised definitions first, then
    /// each category by the highest priority in it (so specific frameworks
    /// come before the generic fallbacks, whatever the categories are
    /// called), alphabetical within one.
    pub fn catalog(&self) -> FrameworkCatalog {
        let mut frameworks: Vec<FrameworkSummary> = self
            .frameworks
            .iter()
            .map(|f| FrameworkSummary {
                id: f.def.id.clone(),
                label: f.def.label.clone(),
                docroot: f.def.docroot.clone(),
                serve: f.def.serve,
                source: f.source,
                category: f
                    .def
                    .category
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_string)
                    .unwrap_or_else(|| match f.source {
                        Source::User => OWN_CATEGORY.to_string(),
                        Source::Builtin => "Other".to_string(),
                    }),
            })
            .collect();

        // A category ranks by its most specific member; the user's own
        // uncategorised definitions go above every category.
        let mut ranks: HashMap<String, u16> = HashMap::new();
        for (framework, summary) in self.frameworks.iter().zip(&frameworks) {
            let rank = if summary.category == OWN_CATEGORY {
                u16::from(MAX_PRIORITY) + 1
            } else {
                u16::from(framework.def.priority)
            };
            let entry = ranks.entry(summary.category.clone()).or_default();
            *entry = (*entry).max(rank);
        }

        frameworks.sort_by(|a, b| {
            let rank = |f: &FrameworkSummary| ranks.get(&f.category).copied().unwrap_or(0);
            rank(b)
                .cmp(&rank(a))
                .then_with(|| a.category.to_lowercase().cmp(&b.category.to_lowercase()))
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });

        FrameworkCatalog {
            frameworks,
            problems: self.problems.clone(),
        }
    }

    /// What telemetry may say about `id`: built-in ids verbatim, anything
    /// the user defined as `user-defined` — their own ids can name their
    /// company or their project.
    pub fn reportable_id(&self, id: &str) -> String {
        match self.get(id) {
            Some(f) if f.source == Source::Builtin => id.to_string(),
            Some(_) => "user-defined".to_string(),
            None if id == CUSTOM => CUSTOM.to_string(),
            None => UNKNOWN.to_string(),
        }
    }
}

/// What a project folder offers the rules, read once per detection.
struct Facts<'a> {
    dir: &'a Path,
    composer: HashSet<String>,
    npm: HashSet<String>,
}

impl<'a> Facts<'a> {
    fn read(dir: &'a Path) -> Self {
        Self {
            dir,
            composer: package_names(&dir.join("composer.json"), &["require", "require-dev"]),
            npm: package_names(
                &dir.join("package.json"),
                &["dependencies", "devDependencies"],
            ),
        }
    }
}

/// Package names from `sections` of a JSON manifest, lowercased. A missing
/// or broken manifest just has none.
fn package_names(manifest: &Path, sections: &[&str]) -> HashSet<String> {
    let Ok(content) = fs::read_to_string(manifest) else {
        return HashSet::new();
    };
    let Ok(json) = serde_json::from_str::<Value>(content.trim_start_matches('\u{feff}')) else {
        return HashSet::new();
    };
    sections
        .iter()
        .filter_map(|section| json.get(section)?.as_object())
        .flat_map(|deps| deps.keys())
        .map(|name| name.to_ascii_lowercase())
        .collect()
}

impl Rule {
    fn matches(&self, facts: &Facts) -> bool {
        match self {
            Rule::File(path) => facts.dir.join(native(path)).is_file(),
            Rule::Dir(path) => facts.dir.join(native(path)).is_dir(),
            Rule::Composer(name) => facts.composer.contains(&name.trim().to_ascii_lowercase()),
            Rule::Npm(name) => facts.npm.contains(&name.trim().to_ascii_lowercase()),
            Rule::Any(rules) => rules.iter().any(|rule| rule.matches(facts)),
            Rule::All(rules) => rules.iter().all(|rule| rule.matches(facts)),
            Rule::Not(rule) => !rule.matches(facts),
        }
    }
}

/// A validated relative path with Windows separators and no trailing one —
/// `public/` and `public` are the same docroot.
fn native(path: &str) -> String {
    path.trim()
        .trim_end_matches(['/', '\\'])
        .replace('/', MAIN_SEPARATOR_STR)
}

/// The folder nginx serves for `project`: its `docroot` subfolder, or the
/// project folder itself when there's none — or when that subfolder isn't
/// there, which lets one definition cover two layouts (Drupal installed with
/// Composer serves `web\`, the plain tarball has no `web\`).
pub fn resolve_docroot(project: &Path, docroot: Option<&str>) -> PathBuf {
    match docroot.map(native) {
        Some(sub) if !sub.is_empty() && project.join(&sub).is_dir() => project.join(sub),
        _ => project.to_path_buf(),
    }
}

/// Whether `path` is safe as a docroot or a rule's file: relative, inside
/// the project, and nothing nginx would read as more than a path. The
/// docroot ends up between quotes in the generated config, where a `"`
/// would end the string and a `$` would be read as a variable.
pub fn validate_relative_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err("it's empty".to_string());
    }
    if trimmed.len() > MAX_PATH_LEN {
        return Err(format!("it's longer than {MAX_PATH_LEN} characters"));
    }
    if trimmed.starts_with(['/', '\\']) || trimmed.contains(':') {
        return Err("it has to be relative to the project folder".to_string());
    }
    if let Some(bad) = trimmed
        .chars()
        .find(|c| c.is_control() || matches!(c, '"' | '$' | '<' | '>' | '|' | '?' | '*'))
    {
        return Err(format!("it can't contain {bad:?}"));
    }
    for segment in trimmed.split(['/', '\\']) {
        if segment.is_empty() {
            return Err("it has an empty folder name in it".to_string());
        }
        if segment == "." || segment == ".." {
            return Err("it can't contain \".\" or \"..\"".to_string());
        }
    }
    Ok(())
}

fn validate(def: &FrameworkDef) -> Result<(), String> {
    let id = &def.id;
    if id.is_empty() || id.len() > MAX_ID_LEN {
        return Err(format!("the id must be 1 to {MAX_ID_LEN} characters"));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || id.starts_with('-')
        || id.ends_with('-')
    {
        return Err("the id may only use lowercase letters, digits and hyphens".to_string());
    }
    if id == UNKNOWN || id == CUSTOM {
        return Err(format!("\"{id}\" is reserved"));
    }
    let label = def.label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_LEN {
        return Err(format!("the label must be 1 to {MAX_LABEL_LEN} characters"));
    }
    if def.priority > MAX_PRIORITY {
        return Err(format!("priority must be 0 to {MAX_PRIORITY}"));
    }
    if let Some(category) = &def.category {
        let category = category.trim();
        if category.is_empty() || category.chars().count() > MAX_CATEGORY_LEN {
            return Err(format!(
                "the category must be 1 to {MAX_CATEGORY_LEN} characters"
            ));
        }
    }
    if let Some(docroot) = &def.docroot {
        if def.serve == ServeProfile::Proxy {
            return Err(
                "a \"proxy\" framework has no docroot — nginx passes every request to its dev server"
                    .to_string(),
            );
        }
        validate_relative_path(docroot).map_err(|reason| format!("docroot {reason}"))?;
    }
    if let Some(dev) = &def.dev {
        if def.serve != ServeProfile::Proxy {
            return Err("\"dev\" only applies to \"serve\": \"proxy\"".to_string());
        }
        if let Some(script) = &dev.script {
            validate_script_name(script).map_err(|reason| format!("dev script {reason}"))?;
        }
    }
    validate_rule(&def.detect, 1)
}

const MAX_SCRIPT_LEN: usize = 64;

/// Whether `name` is safe as the `package.json` script a dev server runs.
///
/// Stricter than npm itself on purpose: the name ends up on a `cmd.exe`
/// command line (`services::dev_servers`), where `&`, `|`, `%`, `^` and
/// quotes all mean something. Real script names (`dev`, `start`,
/// `dev:web`, `serve.local`) never need any of them.
pub fn validate_script_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > MAX_SCRIPT_LEN {
        return Err(format!("must be 1 to {MAX_SCRIPT_LEN} characters"));
    }
    if name.starts_with(['-', '.']) {
        return Err("can't start with \"-\" or \".\"".to_string());
    }
    match name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.')))
    {
        Some(bad) => Err(format!("can't contain {bad:?}")),
        None => Ok(()),
    }
}

fn validate_rule(rule: &Rule, depth: usize) -> Result<(), String> {
    if depth > MAX_RULE_DEPTH {
        return Err(format!("detect nests deeper than {MAX_RULE_DEPTH} levels"));
    }
    match rule {
        Rule::File(path) | Rule::Dir(path) => validate_relative_path(path)
            .map_err(|reason| format!("detect path \"{path}\": {reason}")),
        Rule::Composer(name) | Rule::Npm(name) if name.trim().is_empty() => {
            Err("detect names an empty package".to_string())
        }
        Rule::Composer(_) | Rule::Npm(_) => Ok(()),
        Rule::Any(rules) | Rule::All(rules) if rules.is_empty() => {
            Err("\"any\" and \"all\" need at least one rule".to_string())
        }
        Rule::Any(rules) | Rule::All(rules) => rules
            .iter()
            .try_for_each(|rule| validate_rule(rule, depth + 1)),
        Rule::Not(rule) => validate_rule(rule, depth + 1),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionFile {
    frameworks: Vec<Value>,
}

/// Every usable definition in `content`, in order. Each entry is parsed and
/// validated on its own, so one mistake skips one framework, not the file;
/// what was skipped and why lands in `problems`, prefixed with `origin`.
fn parse_definitions(content: &str, origin: &str, problems: &mut Vec<String>) -> Vec<FrameworkDef> {
    let file: DefinitionFile = match serde_json::from_str(content.trim_start_matches('\u{feff}')) {
        Ok(file) => file,
        Err(err) => {
            problems.push(format!(
                "{origin} couldn't be read, so none of it is used: {err}"
            ));
            return Vec::new();
        }
    };

    let mut seen = HashSet::new();
    let mut defs = Vec::new();
    for (index, raw) in file.frameworks.into_iter().enumerate() {
        let name = raw
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| format!("entry {}", index + 1), |id| format!("\"{id}\""));
        let def = match serde_json::from_value::<FrameworkDef>(raw) {
            Ok(def) => def,
            Err(err) => {
                problems.push(format!("{origin}: {name} was skipped — {err}"));
                continue;
            }
        };
        if let Err(reason) = validate(&def) {
            problems.push(format!("{origin}: {name} was skipped — {reason}"));
            continue;
        }
        if !seen.insert(def.id.clone()) {
            problems.push(format!(
                "{origin}: {name} was skipped — that id is already used above it"
            ));
            continue;
        }
        defs.push(def);
    }
    defs
}

/// The built-in definitions, then `user_file`'s merged over them: an id the
/// built-ins already have is replaced in place, a new one is added.
fn build(user_file: Option<&Path>) -> Registry {
    let mut problems = Vec::new();
    let mut frameworks: Vec<Framework> =
        parse_definitions(BUILTIN, "Rezure's built-in frameworks.json", &mut problems)
            .into_iter()
            .map(|def| Framework {
                def,
                source: Source::Builtin,
            })
            .collect();
    if !problems.is_empty() {
        // `builtin_definitions_are_all_valid` exists so this never happens.
        log::error!("built-in framework definitions are broken: {problems:?}");
    }

    if let Some(path) = user_file {
        match fs::read_to_string(path) {
            Ok(content) => {
                for def in parse_definitions(&content, "frameworks.json", &mut problems) {
                    let framework = Framework {
                        def,
                        source: Source::User,
                    };
                    match frameworks.iter_mut().find(|f| f.def.id == framework.def.id) {
                        Some(existing) => *existing = framework,
                        None => frameworks.push(framework),
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => problems.push(format!("frameworks.json couldn't be read: {err}")),
        }
    }

    Registry {
        frameworks,
        problems,
    }
}

/// `C:\rezure\etc\frameworks.json` — the user's own definitions.
pub fn user_file() -> Option<PathBuf> {
    paths::etc().ok().map(|etc| etc.join("frameworks.json"))
}

fn cache() -> &'static RwLock<Arc<Registry>> {
    static CACHE: OnceLock<RwLock<Arc<Registry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(Arc::new(build(user_file().as_deref()))))
}

/// The registry as last loaded: on first use, then on each [`reload`]. Read
/// once rather than per scan, so the user's file is only re-read when they
/// ask for it.
pub fn registry() -> Arc<Registry> {
    Arc::clone(&cache().read().unwrap_or_else(PoisonError::into_inner))
}

/// Re-reads the user's `frameworks.json` and replaces the cached registry.
pub fn reload() -> Arc<Registry> {
    let fresh = Arc::new(build(user_file().as_deref()));
    *cache().write().unwrap_or_else(PoisonError::into_inner) = Arc::clone(&fresh);
    fresh
}

/// Only the built-in definitions — what tests detect against, so a
/// developer's own `etc\frameworks.json` can't change their outcome.
#[cfg(test)]
pub(crate) fn builtin_registry() -> Registry {
    build(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("frameworks")
    }

    fn fixture(name: &str) -> PathBuf {
        let dir = fixtures_root().join(name);
        assert!(dir.is_dir(), "missing fixture {}", dir.display());
        dir
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rezure-test-frameworks-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// What guarantees the built-in file never fails to parse at runtime.
    #[test]
    fn builtin_definitions_are_all_valid() {
        let mut problems = Vec::new();
        let defs = parse_definitions(BUILTIN, "built-in", &mut problems);
        assert!(problems.is_empty(), "{problems:#?}");
        assert!(defs.len() >= 20, "only {} definitions", defs.len());
    }

    /// Every fixture folder is named after the framework it must be
    /// detected as (`<id>` or `<id>--<variant>`). Adding a framework is an
    /// entry in `frameworks.json` plus a folder here — no new test needed.
    #[test]
    fn every_fixture_is_detected_as_its_folder_name() {
        let registry = builtin_registry();
        let mut checked = 0;
        for entry in fs::read_dir(fixtures_root()).unwrap() {
            let dir = entry.unwrap().path();
            let name = dir.file_name().unwrap().to_string_lossy().to_string();
            let expected = name.split("--").next().unwrap().to_string();

            let serving = registry.serving(&dir, None);
            assert_eq!(serving.framework, expected, "fixture {name}");
            checked += 1;
        }
        assert!(checked > 0);
    }

    /// The other half of the contract above: a built-in framework with no
    /// fixture would be a detection rule nothing checks.
    #[test]
    fn every_builtin_framework_has_a_fixture() {
        let fixtures: HashSet<String> = fs::read_dir(fixtures_root())
            .unwrap()
            .map(|entry| {
                let name = entry.unwrap().file_name().to_string_lossy().to_string();
                name.split("--").next().unwrap().to_string()
            })
            .collect();
        for framework in builtin_registry().frameworks() {
            assert!(
                fixtures.contains(&framework.def.id),
                "no fixture for \"{}\" in {}",
                framework.def.id,
                fixtures_root().display()
            );
        }
    }

    /// The bug this registry was built for: melisa-topup has `spark`, a
    /// CodeIgniter 4 `composer.json`, and a `package.json` that only builds
    /// its CSS. It must be CodeIgniter 4, served from `public\`.
    #[test]
    fn codeigniter4_with_sass_is_served_from_public() {
        let dir = fixture("codeigniter4--with-sass");
        let serving = builtin_registry().serving(&dir, None);

        assert_eq!(serving.framework, "codeigniter4");
        assert_eq!(serving.label, "CodeIgniter 4");
        assert_eq!(serving.docroot, dir.join("public"));
        assert!(!serving.overridden);
    }

    #[test]
    fn a_backend_framework_beats_the_frontend_one_it_bundles() {
        let registry = builtin_registry();
        assert_eq!(
            registry
                .serving(&fixture("laravel--with-vue"), None)
                .framework,
            "laravel"
        );
        assert_eq!(
            registry
                .serving(&fixture("nextjs--with-react"), None)
                .framework,
            "nextjs"
        );
    }

    #[test]
    fn a_php_app_with_only_public_index_is_served_from_public() {
        let dir = fixture("php-public--no-root-index");
        assert_eq!(
            builtin_registry().serving(&dir, None).docroot,
            dir.join("public")
        );
    }

    /// One definition, two layouts: a missing docroot folder falls back to
    /// the project folder instead of pointing nginx at nothing.
    #[test]
    fn a_docroot_that_is_not_there_falls_back_to_the_project_folder() {
        let dir = fixture("drupal--tarball");
        let serving = builtin_registry().serving(&dir, None);
        assert_eq!(serving.framework, "drupal");
        assert_eq!(serving.docroot, dir);
    }

    #[test]
    fn an_empty_folder_is_unknown_and_served_from_itself() {
        let dir = temp_dir("empty");
        let serving = builtin_registry().serving(&dir, None);
        assert_eq!(serving.framework, UNKNOWN);
        assert_eq!(serving.label, "Unknown");
        assert_eq!(serving.docroot, dir);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_chosen_framework_wins_over_detection() {
        let dir = fixture("codeigniter4--with-sass");
        let choice = FrameworkOverride::Framework("php".to_string());
        let serving = builtin_registry().serving(&dir, Some(&choice));

        assert_eq!(serving.framework, "php");
        assert_eq!(serving.detected, "codeigniter4");
        assert!(serving.overridden);
        assert_eq!(serving.docroot, dir);
    }

    #[test]
    fn a_custom_docroot_is_served_as_custom() {
        let dir = fixture("codeigniter4--with-sass");
        let choice = FrameworkOverride::Docroot("public/".to_string());
        let serving = builtin_registry().serving(&dir, Some(&choice));

        assert_eq!(serving.framework, CUSTOM);
        assert_eq!(serving.label, "Custom");
        assert_eq!(serving.docroot, dir.join("public"));
        assert!(serving.overridden);
    }

    /// A user definition deleted after a project was set to it: the project
    /// keeps working on detection rather than failing.
    #[test]
    fn a_choice_naming_a_missing_framework_falls_back_to_detection() {
        let dir = fixture("codeigniter4--with-sass");
        let choice = FrameworkOverride::Framework("acme".to_string());
        let serving = builtin_registry().serving(&dir, Some(&choice));

        assert_eq!(serving.framework, "codeigniter4");
        assert!(!serving.overridden);
    }

    #[test]
    fn paths_that_leave_the_project_or_break_nginx_are_refused() {
        for bad in [
            "",
            "..",
            "../x",
            "a/../b",
            r"C:\x",
            r"\\server\x",
            "/abs",
            "a//b",
            "./public",
            "pub\"lic",
            "$document_root",
        ] {
            assert!(
                validate_relative_path(bad).is_err(),
                "{bad:?} must be refused"
            );
        }
        for good in ["public", "public/", r"src\web", "src/web", "htdocs"] {
            assert!(
                validate_relative_path(good).is_ok(),
                "{good:?} must be fine"
            );
        }
    }

    fn user_registry(name: &str, content: &str) -> Registry {
        let path = std::env::temp_dir().join(format!(
            "rezure-test-user-frameworks-{name}-{}.json",
            std::process::id()
        ));
        fs::write(&path, content).unwrap();
        let registry = build(Some(&path));
        fs::remove_file(&path).unwrap();
        registry
    }

    #[test]
    fn a_user_definition_replaces_a_builtin_and_adds_new_ones() {
        let registry = user_registry(
            "merge",
            r#"{ "frameworks": [
                { "id": "laravel", "label": "Laravel (ours)", "priority": 95,
                  "detect": { "file": "artisan" }, "docroot": "public" },
                { "id": "acme", "label": "Acme", "priority": 85,
                  "detect": { "composer": "acme/core" }, "docroot": "www" }
            ] }"#,
        );

        assert!(registry.problems().is_empty(), "{:?}", registry.problems());
        let laravel = registry.get("laravel").unwrap();
        assert_eq!(laravel.def.label, "Laravel (ours)");
        assert_eq!(laravel.source, Source::User);
        assert_eq!(registry.get("acme").unwrap().source, Source::User);
        assert_eq!(
            registry.frameworks().len(),
            builtin_registry().frameworks().len() + 1
        );
    }

    #[test]
    fn one_bad_user_entry_is_skipped_and_the_rest_kept() {
        let registry = user_registry(
            "bad-entry",
            r#"{ "frameworks": [
                { "id": "escape", "label": "Escape", "priority": 99,
                  "detect": { "file": "index.php" }, "docroot": "../.." },
                { "id": "typo", "label": "Typo", "priority": 50,
                  "detect": { "file": "x" }, "docRoot": "public" },
                { "id": "nginx", "label": "Nginx", "priority": 50,
                  "detect": { "file": "x" }, "serve": "fastcgi" },
                { "id": "fine", "label": "Fine", "priority": 50,
                  "detect": { "file": "fine.txt" } }
            ] }"#,
        );

        assert!(registry.get("fine").is_some());
        assert!(registry.get("escape").is_none());
        assert!(registry.get("typo").is_none());
        assert!(registry.get("nginx").is_none());
        assert_eq!(registry.problems().len(), 3, "{:?}", registry.problems());
        assert!(registry.problems()[0].contains("\"escape\" was skipped"));
    }

    #[test]
    fn proxy_definitions_are_checked_for_what_cannot_apply_to_them() {
        let registry = user_registry(
            "proxy",
            r#"{ "frameworks": [
                { "id": "proxy-docroot", "label": "A", "priority": 50, "detect": { "file": "x" },
                  "serve": "proxy", "docroot": "public" },
                { "id": "php-dev", "label": "B", "priority": 50, "detect": { "file": "x" },
                  "dev": { "script": "dev" } },
                { "id": "bad-script", "label": "C", "priority": 50, "detect": { "file": "x" },
                  "serve": "proxy", "dev": { "script": "dev & calc" } },
                { "id": "bad-mode", "label": "D", "priority": 50, "detect": { "file": "x" },
                  "serve": "proxy", "dev": { "port": "random" } },
                { "id": "fine-proxy", "label": "E", "priority": 50, "detect": { "file": "x" },
                  "serve": "proxy", "dev": { "script": "dev:web", "port": "vite" } }
            ] }"#,
        );

        assert_eq!(registry.problems().len(), 4, "{:?}", registry.problems());
        let fine = registry.get("fine-proxy").unwrap();
        assert_eq!(fine.def.serve, ServeProfile::Proxy);
        assert_eq!(
            fine.def.dev,
            Some(DevDefaults {
                script: Some("dev:web".to_string()),
                port: Some(PortMode::Vite),
            })
        );
    }

    #[test]
    fn script_names_that_mean_something_to_cmd_are_refused() {
        for bad in [
            "", "dev&calc", "dev|x", "%PATH%", "a^b", "\"dev\"", "-x", ".hidden", "dev web",
        ] {
            assert!(
                validate_script_name(bad).is_err(),
                "{bad:?} must be refused"
            );
        }
        for good in [
            "dev",
            "start",
            "dev:web",
            "serve.local",
            "dev_api",
            "build-watch",
        ] {
            assert!(validate_script_name(good).is_ok(), "{good:?} must be fine");
        }
    }

    /// The JavaScript frameworks run a dev server; the fallbacks a PHP or
    /// static project lands on still serve files.
    #[test]
    fn javascript_frameworks_are_proxied_and_php_ones_are_not() {
        let registry = builtin_registry();
        for id in [
            "nextjs", "nuxt", "astro", "vue", "react", "svelte", "angular", "node",
        ] {
            let def = &registry.get(id).unwrap().def;
            assert_eq!(def.serve, ServeProfile::Proxy, "{id}");
            assert!(def.dev.is_some(), "{id} needs dev defaults");
        }
        for id in ["laravel", "codeigniter4", "php", "php-public", "static"] {
            assert_eq!(
                registry.get(id).unwrap().def.serve,
                ServeProfile::Php,
                "{id}"
            );
        }
        assert_eq!(
            registry.serving(&fixture("vue"), None).serve,
            ServeProfile::Proxy
        );
    }

    /// Serving `dist\` of a Vue app through a custom folder keeps working as
    /// files, whatever the framework's own profile is.
    #[test]
    fn a_custom_docroot_is_never_proxied() {
        let dir = fixture("vue");
        let choice = FrameworkOverride::Docroot("src".to_string());
        assert_eq!(
            builtin_registry().serving(&dir, Some(&choice)).serve,
            ServeProfile::Php
        );
    }

    #[test]
    fn a_broken_user_file_leaves_the_builtins_and_says_why() {
        let registry = user_registry("broken", "{ not json");

        assert_eq!(
            registry.frameworks().len(),
            builtin_registry().frameworks().len()
        );
        assert_eq!(registry.problems().len(), 1);
        assert!(registry.problems()[0].contains("couldn't be read"));
    }

    #[test]
    fn reserved_ids_and_empty_combinators_are_refused() {
        let registry = user_registry(
            "reserved",
            r#"{ "frameworks": [
                { "id": "custom", "label": "Mine", "priority": 50, "detect": { "file": "x" } },
                { "id": "everything", "label": "All", "priority": 50, "detect": { "all": [] } }
            ] }"#,
        );
        assert_eq!(registry.problems().len(), 2, "{:?}", registry.problems());
    }

    #[test]
    fn telemetry_never_sees_a_user_defined_id() {
        let registry = user_registry(
            "telemetry",
            r#"{ "frameworks": [
                { "id": "secret-client-cms", "label": "Ours", "priority": 50, "detect": { "file": "x" } }
            ] }"#,
        );
        assert_eq!(registry.reportable_id("laravel"), "laravel");
        assert_eq!(registry.reportable_id("secret-client-cms"), "user-defined");
        assert_eq!(registry.reportable_id(CUSTOM), CUSTOM);
        assert_eq!(registry.reportable_id(UNKNOWN), UNKNOWN);
    }

    /// The headings the pickers end up with, in order.
    fn categories(catalog: &FrameworkCatalog) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for framework in &catalog.frameworks {
            if seen.last() != Some(&framework.category) {
                seen.push(framework.category.clone());
            }
        }
        seen
    }

    /// Every built-in sits under a heading, and the headings run from the
    /// most specific frameworks down to the fallbacks.
    #[test]
    fn the_catalog_groups_builtins_from_specific_to_generic() {
        let catalog = builtin_registry().catalog();
        assert_eq!(categories(&catalog), ["PHP", "JavaScript", "Generic"]);

        // Alphabetical inside a heading.
        let php: Vec<String> = catalog
            .frameworks
            .iter()
            .filter(|f| f.category == "PHP")
            .map(|f| f.label.to_lowercase())
            .collect();
        let mut sorted = php.clone();
        sorted.sort();
        assert_eq!(php, sorted);
    }

    /// A user's own definition without a category is listed first, under its
    /// own heading; one that names a category joins it.
    #[test]
    fn user_definitions_get_their_own_heading_unless_they_name_one() {
        let registry = user_registry(
            "categories",
            r#"{ "frameworks": [
                { "id": "acme", "label": "Acme", "priority": 85, "detect": { "file": "acme.php" } },
                { "id": "kirby", "label": "Kirby", "priority": 85, "category": "PHP",
                  "detect": { "composer": "getkirby/cms" } }
            ] }"#,
        );
        let catalog = registry.catalog();

        assert_eq!(
            categories(&catalog),
            ["Your frameworks", "PHP", "JavaScript", "Generic"]
        );
        let kirby = catalog.frameworks.iter().find(|f| f.id == "kirby").unwrap();
        assert_eq!(kirby.category, "PHP");
        assert_eq!(kirby.source, Source::User);
    }
}
