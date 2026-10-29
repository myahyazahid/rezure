//! The processes behind `proxy` projects — a Vite, Astro, Next or Nuxt dev
//! server, or a Node app like 9router.
//!
//! # Two layers
//!
//! A `proxy` project's vhost passes every request to `127.0.0.1:<port>`
//! (`services::vhosts`), whoever started the process listening there. That
//! alone is enough for an app the user runs themselves. On top of it, Rezure
//! can run the process too: each `proxy` project is registered with
//! `ServiceManager` as a [`Service`](super::Service) — a `ProcessService`
//! built from a [`DevSpec`] — so it starts, stops, logs and is cleaned up
//! like nginx or PHP, and is stopped when Rezure closes.
//!
//! Unlike every other service, a dev server is never started by Start all
//! and never listed on the Services page: each one holds hundreds of MB, so
//! starting it is always a choice made for that one project (`Open`, or the
//! Dev servers dialog).
//!
//! # The port is Rezure's
//!
//! Vite moves to the next free port without a word when its own is taken,
//! which leaves the proxy pointing at nothing. So Rezure picks the port —
//! once, from [`PORT_RANGE`], kept in `config::dev_servers` — and makes the
//! dev server use it, the way its [`PortMode`] says that tool is told.
//!
//! # Only scripts, never commands
//!
//! What runs is always `<package manager> run <script>`, with a script that
//! exists in the project's `package.json` and a name [`validate_script_name`]
//! accepts. Nothing a user types reaches `cmd.exe` as text: the port is a
//! number, the package manager one of four, and the arguments per mode are
//! fixed here.
//!
//! [`validate_script_name`]: frameworks::validate_script_name

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::frameworks::{self, PortMode, Registry, ServeProfile};
use super::process::{LogSink, LogStream};
use super::{database, db_profiles, launcher, localdb_bridge, node, php_pool, postgres, process};
use super::{ports, ServiceManager, ServiceStatus};
use crate::config::dev_servers::{self as config, DevServerConfig, DevServerStore};
use crate::config::links;
use crate::db::projects::ProjectInfo;
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

/// Where a project's port is picked from the first time it needs one. Clear
/// of the defaults dev tools pick for themselves (3000, 4200, 4321, 5173,
/// 8080), so a dev server started by hand elsewhere doesn't collide with
/// one Rezure runs, and below Windows' dynamic range (49152 and up), which
/// the OS hands out to outgoing connections.
pub const PORT_RANGE: RangeInclusive<u16> = 4400..=4499;

/// The lowest port a dev server may be given by hand. Below it, binding
/// needs administrator rights on most systems anyway.
const MIN_PORT: u16 = 1024;

/// What a dev server's service id starts with. Not `dev:` — the id also
/// names its folder under `data\`, and `:` can't be in a Windows path.
const SERVICE_PREFIX: &str = "dev-";

/// The id a project's dev server is registered under with `ServiceManager`.
pub fn service_id(project_id: &str) -> String {
    format!("{SERVICE_PREFIX}{project_id}")
}

/// Whether `id` names a dev server rather than one of Rezure's own services.
pub fn is_dev_service(id: &str) -> bool {
    id.starts_with(SERVICE_PREFIX)
}

/// The project a dev server's service id belongs to.
pub fn project_id_of(service_id: &str) -> Option<&str> {
    service_id.strip_prefix(SERVICE_PREFIX)
}

/// Which of Rezure's own services listens on `port`, if any — a dev server
/// given that port would fail to start, or worse, start and be shadowed.
fn reserved_by_rezure(port: u16) -> Option<&'static str> {
    // The pool grows one block per pinned PHP version; thirty is far more
    // versions than anyone pins at once.
    let php = php_pool::DEFAULT_BASE_PORT..php_pool::DEFAULT_BASE_PORT + php_pool::PORT_BLOCK * 30;
    match port {
        80 | 443 => Some("Nginx"),
        database::DEFAULT_PORT => Some("the database"),
        postgres::PORT => Some("PostgreSQL"),
        process::MAILPIT_SMTP_PORT | process::MAILPIT_WEB_PORT => Some("Mailpit"),
        localdb_bridge::PORT => Some("SQL Server LocalDB"),
        _ if php.contains(&port) => Some("PHP"),
        _ if db_profiles::active().is_some_and(|profile| profile.port == port) => {
            Some("the database")
        }
        _ => None,
    }
}

/// Whether `project_id`'s dev server may listen on `port`: not a system
/// port, not one of Rezure's own, and not another project's.
pub fn validate_port(
    port: u16,
    project_id: &str,
    store: &DevServerStore,
    projects: &[ProjectInfo],
) -> Result<(), AppError> {
    if port < MIN_PORT {
        return Err(AppError::InvalidDevPort {
            port,
            reason: format!("ports below {MIN_PORT} need administrator rights"),
        });
    }
    if let Some(service) = reserved_by_rezure(port) {
        return Err(AppError::InvalidDevPort {
            port,
            reason: format!("{service} already uses it"),
        });
    }
    if let Some(owner) = store.owner_of(port).filter(|owner| *owner != project_id) {
        let by = projects
            .iter()
            .find(|project| project.id == owner)
            .map_or_else(|| owner.to_string(), |project| project.name.clone());
        return Err(AppError::DevPortTaken { port, by });
    }
    Ok(())
}

/// The dev server port of every `proxy` project in `projects`, picking and
/// saving one from [`PORT_RANGE`] for any that has none yet.
///
/// Called from the vhost sync, which is the one place that has to know the
/// port to write it — the scan itself only ever reads.
pub fn assign_ports(projects: &[ProjectInfo]) -> HashMap<String, u16> {
    let mut store = config::load();
    let wanted: Vec<&str> = projects
        .iter()
        .filter(|project| project.serve == ServeProfile::Proxy)
        .map(|project| project.id.as_str())
        .collect();

    if allocate(&mut store, &wanted, |port| {
        TcpListener::bind(("127.0.0.1", port)).is_ok()
    }) {
        if let Err(err) = config::save(&store) {
            log::warn!("couldn't save the dev server ports just picked: {err}");
        }
    }

    wanted
        .into_iter()
        .filter_map(|id| store.get(id).map(|entry| (id.to_string(), entry.port)))
        .collect()
}

/// Gives every id in `wanted` without an entry the first port in
/// [`PORT_RANGE`] that no project has, Rezure doesn't use, and `is_free`
/// accepts. Whether anything was added.
fn allocate(store: &mut DevServerStore, wanted: &[&str], is_free: impl Fn(u16) -> bool) -> bool {
    let mut changed = false;
    for id in wanted {
        if store.get(id).is_some() {
            continue;
        }
        let port = PORT_RANGE.clone().find(|port| {
            store.owner_of(*port).is_none() && reserved_by_rezure(*port).is_none() && is_free(*port)
        });
        match port {
            Some(port) => {
                store.set(id, Some(DevServerConfig::with_port(port)));
                changed = true;
            }
            None => log::warn!(
                "no free dev server port left in {}–{} for {id}",
                PORT_RANGE.start(),
                PORT_RANGE.end()
            ),
        }
    }
    changed
}

/// Which package manager a project uses, going by its lockfile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    /// pnpm, Yarn and Bun each leave a lockfile npm never writes; a project
    /// with none of them is npm's.
    pub fn detect(dir: &Path) -> Self {
        if dir.join("pnpm-lock.yaml").is_file() {
            Self::Pnpm
        } else if dir.join("yarn.lock").is_file() {
            Self::Yarn
        } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() {
            Self::Bun
        } else {
            Self::Npm
        }
    }

    /// The command, as typed.
    pub fn command(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }

    /// What `cmd.exe` finds on `PATH` for [`Self::command`].
    fn executables(self) -> &'static [&'static str] {
        match self {
            Self::Npm => &["npm.cmd"],
            Self::Pnpm => &["pnpm.cmd", "pnpm.exe"],
            Self::Yarn => &["yarn.cmd", "yarn.exe"],
            Self::Bun => &["bun.exe"],
        }
    }

    /// npm alone needs `--` before arguments meant for the script. pnpm
    /// passes a `--` on to the script as a literal argument, which makes
    /// Vite read everything after it as file names.
    fn needs_separator(self) -> bool {
        self == Self::Npm
    }
}

/// The script names in `dir\package.json` that a dev server could run.
/// Names [`frameworks::validate_script_name`] refuses are left out — they
/// can't be chosen anyway.
pub fn package_scripts(dir: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(json) =
        serde_json::from_str::<serde_json::Value>(content.trim_start_matches('\u{feff}'))
    else {
        return Vec::new();
    };
    json.get("scripts")
        .and_then(serde_json::Value::as_object)
        .map(|scripts| {
            scripts
                .keys()
                .filter(|name| frameworks::validate_script_name(name).is_ok())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// The script to run: the user's choice; otherwise the framework's default,
/// then `dev`, then `start` — whichever `package.json` actually has. A
/// choice the project doesn't have is still returned, so the dialog can say
/// it's missing rather than quietly running something else.
fn resolve_script(
    chosen: Option<&str>,
    default: Option<&str>,
    scripts: &[String],
) -> Option<String> {
    if let Some(chosen) = chosen {
        return Some(chosen.to_string());
    }
    [default, Some("dev"), Some("start")]
        .into_iter()
        .flatten()
        .find(|candidate| scripts.iter().any(|script| script == candidate))
        .map(str::to_string)
}

/// Everything about one project's dev server, resolved: its settings over
/// its framework's defaults over Rezure's. What a dev server `Service` is
/// built from — a changed spec means a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevSpec {
    pub project_id: String,
    pub name: String,
    pub domain: String,
    /// The framework's label.
    pub framework: String,
    pub dir: PathBuf,
    pub port: u16,
    pub port_mode: PortMode,
    /// The user's choice, `None` meaning the framework's.
    pub port_mode_setting: Option<PortMode>,
    pub default_port_mode: PortMode,
    pub script: Option<String>,
    /// The user's choice, `None` meaning the framework's.
    pub script_setting: Option<String>,
    pub scripts: Vec<String>,
    pub package_manager: PackageManager,
    /// The project's own Node.js version, `None` meaning the active one.
    pub node_pin: Option<String>,
    pub auto_start: bool,
}

/// The dev server of every `proxy` project that has a port, in `projects`'
/// order. Projects whose folder is gone or whose domain can't be served get
/// none, like they get no vhost.
pub fn specs(
    projects: &[ProjectInfo],
    registry: &Registry,
    store: &DevServerStore,
    node_pins: &HashMap<String, Option<String>>,
) -> Vec<DevSpec> {
    projects
        .iter()
        .filter(|project| {
            project.serve == ServeProfile::Proxy && !project.missing && !project.domain_invalid
        })
        .filter_map(|project| {
            let config = store.get(&project.id)?;
            let defaults = registry
                .get(&project.framework)
                .and_then(|framework| framework.def.dev.clone());
            let default_port_mode = defaults
                .as_ref()
                .and_then(|dev| dev.port)
                .unwrap_or(PortMode::Env);
            let dir = PathBuf::from(&project.path);
            let scripts = package_scripts(&dir);
            Some(DevSpec {
                project_id: project.id.clone(),
                name: project.name.clone(),
                domain: project.domain.clone(),
                framework: project.stack.clone(),
                port: config.port,
                port_mode: config.port_mode.unwrap_or(default_port_mode),
                port_mode_setting: config.port_mode,
                default_port_mode,
                script: resolve_script(
                    config.script.as_deref(),
                    defaults.as_ref().and_then(|dev| dev.script.as_deref()),
                    &scripts,
                ),
                script_setting: config.script.clone(),
                scripts,
                package_manager: PackageManager::detect(&dir),
                node_pin: node_pins.get(&project.id).cloned().flatten(),
                auto_start: config.auto_start,
                dir,
            })
        })
        .collect()
}

/// Why a dev server can't be started as things stand, in the order the user
/// has to fix them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DevServerIssue {
    /// No `package.json`: there's nothing to run.
    NoPackageJson,
    /// `node.exe` isn't installed, or the version the project pins isn't.
    #[serde(rename_all = "camelCase")]
    NodeMissing { version: Option<String> },
    /// The lockfile says pnpm, Yarn or Bun, and it isn't on `PATH`.
    #[serde(rename_all = "camelCase")]
    PackageManagerMissing { tool: String },
    /// `package.json` has no script to run, or not the one chosen.
    #[serde(rename_all = "camelCase")]
    ScriptMissing { script: Option<String> },
    /// `node_modules` isn't there yet.
    NeedsInstall,
    /// A dev server for this project is already running, started outside
    /// Rezure (from a terminal, an IDE). A second one would collide with it —
    /// Astro and Next refuse to start beside their own lock file.
    #[serde(rename_all = "camelCase")]
    AlreadyRunning { servers: Vec<OutsideServer> },
}

impl DevServerIssue {
    pub fn describe(&self) -> String {
        match self {
            Self::NoPackageJson => "the project has no package.json".to_string(),
            Self::NodeMissing { version: Some(version) } => {
                format!("it's set to Node.js {version}, which isn't installed - install it on the Switch page")
            }
            Self::NodeMissing { version: None } => {
                "Node.js isn't installed - install it on the Switch page".to_string()
            }
            Self::PackageManagerMissing { tool } => format!(
                "the project uses {tool}, which isn't installed - run `corepack enable` (or install {tool}) in a terminal"
            ),
            Self::ScriptMissing { script: Some(script) } => {
                format!("package.json has no \"{script}\" script - pick another in Dev servers")
            }
            Self::ScriptMissing { script: None } => {
                "package.json has no dev or start script - pick one in Dev servers".to_string()
            }
            Self::NeedsInstall => {
                "its dependencies aren't installed - click Install in Dev servers".to_string()
            }
            Self::AlreadyRunning { servers } => format!(
                "a dev server for this project is already running outside Rezure ({}) - stop it from Dev servers (\"Stop it and start here\"), or close the terminal it runs in",
                servers
                    .iter()
                    .map(OutsideServer::describe)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }
}

/// A process running a project's code that Rezure didn't start — `npm run
/// dev` from a terminal, an IDE's run button — and is listening on a port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutsideServer {
    /// The topmost process of it that's still a JavaScript runtime (`npm`'s
    /// `node.exe`, not the terminal it was typed in) — what stopping it kills.
    pub pid: u32,
    pub name: String,
    pub ports: Vec<u16>,
}

impl OutsideServer {
    fn describe(&self) -> String {
        let ports: Vec<String> = self.ports.iter().map(|port| format!(":{port}")).collect();
        format!("{}, pid {}, on {}", self.name, self.pid, ports.join(" "))
    }
}

/// The executables a project's dev server runs as.
const JS_RUNTIMES: &[&str] = &["node.exe", "bun.exe", "deno.exe"];

/// Whether `cwd` is `dir` or a folder inside it.
fn in_dir(cwd: &Path, dir: &Path) -> bool {
    let (cwd, dir) = (cwd.display().to_string(), dir.display().to_string());
    links::same_path(&cwd, &dir) || links::is_inside(&cwd, Path::new(&dir))
}

/// The dev servers already running for each of `specs`' projects that Rezure
/// didn't start, by project id. Only projects with one are in the map.
///
/// "Running for a project" is a JavaScript runtime whose working folder is
/// the project's, with something under it listening on a port — a build
/// watcher or a test runner in the same folder listens on nothing, and is
/// left alone. Anything descended from this Rezure is its own dev server; a
/// leftover from a Rezure that crashed isn't, and is reported like any other.
pub fn outside_servers(specs: &[DevSpec]) -> HashMap<String, Vec<OutsideServer>> {
    if specs.is_empty() {
        return HashMap::new();
    }
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::Always),
    );
    let mut listening: HashMap<u32, Vec<u16>> = HashMap::new();
    for (port, pid) in ports::listening_sockets() {
        listening.entry(pid).or_default().push(port);
    }
    find_outside(&sys, &listening, specs, std::process::id())
}

fn find_outside(
    sys: &System,
    listening: &HashMap<u32, Vec<u16>>,
    specs: &[DevSpec],
    rezure: u32,
) -> HashMap<String, Vec<OutsideServer>> {
    let name_of = |pid: Pid| {
        sys.process(pid)
            .map(|p| p.name().to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let is_runtime = |pid: Pid| JS_RUNTIMES.contains(&name_of(pid).as_str());
    let descends_from_rezure = |pid: Pid| {
        let mut current = Some(pid);
        // Bounded: a recycled pid could in principle close a loop.
        for _ in 0..32 {
            let Some(pid) = current else { return false };
            if pid.as_u32() == rezure {
                return true;
            }
            current = sys.process(pid).and_then(|p| p.parent());
        }
        false
    };
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, process) in sys.processes() {
        if let Some(parent) = process.parent() {
            children.entry(parent).or_default().push(*pid);
        }
    }

    let mut found: HashMap<String, Vec<OutsideServer>> = HashMap::new();
    for spec in specs {
        let mut roots: HashSet<Pid> = HashSet::new();
        for (pid, process) in sys.processes() {
            let in_project = process.cwd().is_some_and(|cwd| in_dir(cwd, &spec.dir));
            if !in_project || !is_runtime(*pid) || descends_from_rezure(*pid) {
                continue;
            }
            // Up to the topmost runtime still in the project — `npm`'s own
            // node, above the one actually serving — but never into the
            // shell it was typed in: that's the user's terminal.
            let mut root = *pid;
            for _ in 0..16 {
                let Some(parent) = sys.process(root).and_then(|p| p.parent()) else {
                    break;
                };
                let parent_in_project = sys
                    .process(parent)
                    .and_then(|p| p.cwd())
                    .is_some_and(|cwd| in_dir(cwd, &spec.dir));
                if !is_runtime(parent) || !parent_in_project {
                    break;
                }
                root = parent;
            }
            roots.insert(root);
        }

        let mut servers: Vec<OutsideServer> = roots
            .into_iter()
            .filter_map(|root| {
                let mut ports = Vec::new();
                let mut stack = vec![root];
                let mut seen = HashSet::new();
                while let Some(pid) = stack.pop() {
                    if !seen.insert(pid) {
                        continue;
                    }
                    ports.extend(listening.get(&pid.as_u32()).into_iter().flatten());
                    stack.extend(children.get(&pid).into_iter().flatten());
                }
                ports.sort_unstable();
                ports.dedup();
                (!ports.is_empty()).then(|| OutsideServer {
                    pid: root.as_u32(),
                    name: name_of(root),
                    ports,
                })
            })
            .collect();
        if !servers.is_empty() {
            servers.sort_by_key(|server| server.pid);
            found.insert(spec.project_id.clone(), servers);
        }
    }
    found
}

/// Stops the dev servers running for `spec`'s project outside Rezure, and
/// waits for their ports to be let go. Re-reads them rather than trusting a
/// list the dialog showed: one may have exited since, and its pid been reused
/// by something unrelated.
pub fn stop_outside(spec: &DevSpec) -> Result<(), AppError> {
    let servers = outside_servers(std::slice::from_ref(spec))
        .remove(&spec.project_id)
        .unwrap_or_default();
    for server in &servers {
        process::kill_process_tree(server.pid);
    }
    let held: Vec<u16> = servers.iter().flat_map(|s| s.ports.clone()).collect();
    for _ in 0..50 {
        if held.iter().all(|port| ports::holder(*port).is_none()) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(AppError::DevServerFailed {
        name: spec.name.clone(),
        reason: "the dev server running outside Rezure didn't let go of its port - close the terminal it runs in".to_string(),
    })
}

/// Words that say what a dev server's failure was, as opposed to where in
/// its code it happened.
const ERROR_WORDS: &[&str] = &[
    "error",
    "already",
    "eaddrinuse",
    "cannot",
    "can't",
    "not found",
    "not recognized",
    "failed",
    "unable",
    "lock",
    "missing",
];

/// A dev server's last output, cut down to what explains a failed start,
/// with the step that fixes it when the failure is a familiar one.
///
/// Stack frames and npm's own `> script` banner are dropped: the line worth
/// reading is usually above a dozen `at …` lines, and the last few lines
/// alone are nothing but frames.
pub fn summarize_failure(lines: &[String]) -> String {
    let meaningful: Vec<&str> = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("at ")
                && !line.starts_with("> ")
                && *line != "Stack trace:"
                && !line.starts_with("npm error")
                && !line.starts_with("npm ERR!")
        })
        .collect();
    if meaningful.is_empty() {
        return String::new();
    }
    let first_error = meaningful.iter().position(|line| {
        let lower = line.to_lowercase();
        ERROR_WORDS.iter().any(|word| lower.contains(word))
    });
    let start = first_error.unwrap_or(meaningful.len().saturating_sub(3));
    let shown: Vec<&str> = meaningful.iter().skip(start).take(4).copied().collect();
    let mut text = format!(". It said: {}", shown.join(" / "));

    let all = meaningful.join("\n").to_lowercase();
    let hint = if all.contains("already running")
        || all.contains("eaddrinuse")
        || all.contains("address already in use")
        || all.contains("acquire lock")
        || all.contains("another instance")
    {
        Some(
            "Another copy of this dev server is still running - stop it (Dev servers offers to) and start again",
        )
    } else if all.contains("cannot find module")
        || all.contains("err_module_not_found")
        || all.contains("is not recognized as an internal or external command")
        || all.contains("command not found")
    {
        Some("Its dependencies look incomplete - run Install in Dev servers, then start again")
    } else {
        None
    };
    if let Some(hint) = hint {
        text.push_str(". ");
        text.push_str(hint);
    }
    text
}

/// `PATH` for a process run in `spec`'s project: its Node.js version first,
/// like a terminal opened for the project gets (`launcher::open_terminal`).
fn path_for(node_pin: Option<&str>) -> Result<OsString, DevServerIssue> {
    if let Some(version) = node_pin {
        if node::bin_dir_for(version).is_err() {
            return Err(DevServerIssue::NodeMissing {
                version: Some(version.to_string()),
            });
        }
    }
    let first: Vec<PathBuf> = node::terminal_bin_dir(node_pin).into_iter().collect();
    Ok(launcher::path_with_first(&first)
        .unwrap_or_else(|| std::env::var_os("PATH").unwrap_or_default()))
}

/// The first of `names` found in a folder on `path`.
fn find_on_path(path: &OsString, names: &[&str]) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|dir| {
        names
            .iter()
            .map(|name| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// Node.js and the package manager, or what's missing of them.
fn toolchain(spec: &DevSpec) -> Result<OsString, DevServerIssue> {
    if !spec.dir.join("package.json").is_file() {
        return Err(DevServerIssue::NoPackageJson);
    }
    let path = path_for(spec.node_pin.as_deref())?;
    // Bun runs scripts without Node.js.
    if spec.package_manager != PackageManager::Bun && find_on_path(&path, &["node.exe"]).is_none() {
        return Err(DevServerIssue::NodeMissing {
            version: spec.node_pin.clone(),
        });
    }
    if find_on_path(&path, spec.package_manager.executables()).is_none() {
        return Err(DevServerIssue::PackageManagerMissing {
            tool: spec.package_manager.command().to_string(),
        });
    }
    Ok(path)
}

/// What stands between `spec` and a started dev server, if anything.
pub fn issue(spec: &DevSpec) -> Option<DevServerIssue> {
    if let Err(issue) = toolchain(spec) {
        return Some(issue);
    }
    match &spec.script {
        None => return Some(DevServerIssue::ScriptMissing { script: None }),
        Some(script) if !spec.scripts.contains(script) => {
            return Some(DevServerIssue::ScriptMissing {
                script: Some(script.clone()),
            })
        }
        Some(_) => {}
    }
    if !spec.dir.join("node_modules").is_dir() {
        return Some(DevServerIssue::NeedsInstall);
    }
    None
}

/// The command a dev server is started with, as `cmd.exe` gets it.
///
/// Only ever assembled from a package manager, a script name that passed
/// [`frameworks::validate_script_name`] and a port number — which is what
/// makes it safe to hand to `cmd.exe` as one string.
pub fn command_line(pm: PackageManager, script: &str, mode: PortMode, port: u16) -> String {
    let args: Vec<String> = match mode {
        PortMode::Vite => vec!["--port".into(), port.to_string(), "--strictPort".into()],
        PortMode::Flag => vec!["--port".into(), port.to_string()],
        PortMode::Env | PortMode::Fixed => Vec::new(),
    };
    let mut line = format!("{} run {script}", pm.command());
    if !args.is_empty() {
        if pm.needs_separator() {
            line.push_str(" --");
        }
        line.push(' ');
        line.push_str(&args.join(" "));
    }
    line
}

/// `cmd.exe`. A `.cmd` shim (`npm.cmd`) can only be run through it, and
/// running it explicitly is what lets the command line above be passed
/// exactly as written instead of re-quoted for a program that doesn't
/// follow the usual rules.
pub fn shell() -> PathBuf {
    std::env::var_os("ComSpec")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"))
}

/// A `cmd.exe /d /s /c "<line>"` in `dir`, with `path` and output kept
/// plain for the log panel. `/d` skips AutoRun commands from the registry,
/// which could otherwise print into the log or change directory.
fn shell_command(line: &str, dir: &Path, path: &OsString) -> Command {
    let mut cmd = Command::new(shell());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("/d /s /c \"{line}\""));
    }
    #[cfg(not(windows))]
    cmd.args(["-c", line]);
    cmd.current_dir(dir)
        .env("PATH", path)
        // The log panel shows plain text; escape codes would show as junk.
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("npm_config_update_notifier", "false");
    cmd
}

/// The command that starts `spec`'s dev server, or why it can't yet.
pub fn command(spec: &DevSpec) -> Result<Command, AppError> {
    let not_ready = |issue: DevServerIssue| AppError::DevServerNotReady {
        name: spec.name.clone(),
        reason: issue.describe(),
    };
    if let Some(issue) = issue(spec) {
        return Err(not_ready(issue));
    }
    let path = toolchain(spec).map_err(not_ready)?;
    let script = spec
        .script
        .as_deref()
        .ok_or_else(|| not_ready(DevServerIssue::ScriptMissing { script: None }))?;

    let mut cmd = shell_command(
        &command_line(spec.package_manager, script, spec.port_mode, spec.port),
        &spec.dir,
        &path,
    );
    cmd.env("PORT", spec.port.to_string())
        // Create React App opens a browser tab of its own otherwise.
        .env("BROWSER", "none")
        // Vite refuses requests for any host but localhost unless told
        // otherwise, and nginx passes `astro.test` on as the Host.
        .env("__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS", &spec.domain);
    Ok(cmd)
}

/// Runs `<package manager> install` in `spec`'s project, sending its output
/// to `sink` under the dev server's own id, so it shows in the same log.
pub fn install(spec: &DevSpec, sink: &LogSink) -> Result<(), AppError> {
    let failed = |reason: String| AppError::DevServerFailed {
        name: spec.name.clone(),
        reason,
    };
    let path = toolchain(spec).map_err(|issue| AppError::DevServerNotReady {
        name: spec.name.clone(),
        reason: issue.describe(),
    })?;
    let line = format!("{} install", spec.package_manager.command());
    let id = service_id(&spec.project_id);
    sink(&id, LogStream::Stdout, &format!("> {line}"));

    let mut child = shell_command(&line, &spec.dir, &path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .hidden()
        .spawn()
        .map_err(|e| failed(format!("couldn't run {line}: {e}")))?;

    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|out| forward(sink.clone(), id.clone(), LogStream::Stdout, out)),
        child
            .stderr
            .take()
            .map(|err| forward(sink.clone(), id.clone(), LogStream::Stderr, err)),
    ]
    .into_iter()
    .flatten()
    .collect();

    let status = child
        .wait()
        .map_err(|e| failed(format!("couldn't wait for {line}: {e}")))?;
    for reader in readers {
        let _ = reader.join();
    }
    if status.success() {
        Ok(())
    } else {
        Err(failed(format!("{line} failed ({status}) - see its log")))
    }
}

fn forward(
    sink: LogSink,
    id: String,
    stream: LogStream,
    reader: impl Read + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            sink(&id, stream, &line);
        }
    })
}

/// Whether Open has to start `project_id`'s dev server before it opens the
/// browser: a `proxy` project whose Open-starts-it setting is on. One that's
/// already running is "started" too — that's a no-op that returns at once.
pub fn starts_on_open(serve: ServeProfile, project_id: &str, specs: &[DevSpec]) -> bool {
    serve == ServeProfile::Proxy
        && specs
            .iter()
            .any(|spec| spec.project_id == project_id && spec.auto_start)
}

/// One row of the Dev servers dialog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevServerRow {
    pub project_id: String,
    pub service_id: String,
    pub name: String,
    pub domain: String,
    pub framework: String,
    pub port: u16,
    pub port_mode: PortMode,
    pub port_mode_setting: Option<PortMode>,
    pub default_port_mode: PortMode,
    pub script: Option<String>,
    pub script_setting: Option<String>,
    pub scripts: Vec<String>,
    pub package_manager: PackageManager,
    /// What Start runs, for display — `None` while there's no script.
    pub command: Option<String>,
    pub auto_start: bool,
    pub status: ServiceStatus,
    pub issue: Option<DevServerIssue>,
}

/// The dialog's rows, with each dev server's live status from `manager`. A
/// stopped one already running outside Rezure says so first, since nothing
/// else about it matters until that's dealt with.
pub fn rows(specs: &[DevSpec], manager: &ServiceManager) -> Vec<DevServerRow> {
    let status_of = |spec: &DevSpec| {
        manager
            .find(&service_id(&spec.project_id))
            .map_or(ServiceStatus::Stopped, |service| service.info().status)
    };
    let stopped: Vec<DevSpec> = specs
        .iter()
        .filter(|spec| status_of(spec) == ServiceStatus::Stopped)
        .cloned()
        .collect();
    let mut outside = outside_servers(&stopped);

    specs
        .iter()
        .map(|spec| {
            let service_id = service_id(&spec.project_id);
            let status = status_of(spec);
            let problem = match outside.remove(&spec.project_id) {
                Some(servers) => Some(DevServerIssue::AlreadyRunning { servers }),
                None => issue(spec),
            };
            DevServerRow {
                command: spec.script.as_deref().map(|script| {
                    command_line(spec.package_manager, script, spec.port_mode, spec.port)
                }),
                issue: problem,
                project_id: spec.project_id.clone(),
                service_id,
                name: spec.name.clone(),
                domain: spec.domain.clone(),
                framework: spec.framework.clone(),
                port: spec.port,
                port_mode: spec.port_mode,
                port_mode_setting: spec.port_mode_setting,
                default_port_mode: spec.default_port_mode,
                script: spec.script.clone(),
                script_setting: spec.script_setting.clone(),
                scripts: spec.scripts.clone(),
                package_manager: spec.package_manager,
                auto_start: spec.auto_start,
                status,
            }
        })
        .collect()
}

/// A change to one project's dev server settings, from the dialog.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevServerChange {
    pub port: u16,
    /// `None`: the framework's.
    pub port_mode: Option<PortMode>,
    /// `None` or blank: the framework's.
    pub script: Option<String>,
    pub auto_start: bool,
}

/// Validates and saves `change` for `project`. A running dev server keeps
/// running on its old port until it's stopped, so changing anything it was
/// started with is refused while it runs; `auto_start` can change any time.
pub fn set_config(
    project: &ProjectInfo,
    projects: &[ProjectInfo],
    change: DevServerChange,
    running: bool,
) -> Result<(), AppError> {
    if project.serve != ServeProfile::Proxy {
        return Err(AppError::NotADevServerProject(project.name.clone()));
    }
    let script = change
        .script
        .map(|script| script.trim().to_string())
        .filter(|script| !script.is_empty());
    if let Some(script) = &script {
        frameworks::validate_script_name(script).map_err(|reason| AppError::InvalidDevScript {
            script: script.clone(),
            reason,
        })?;
    }

    let mut store = config::load();
    validate_port(change.port, &project.id, &store, projects)?;

    let next = DevServerConfig {
        port: change.port,
        port_mode: change.port_mode,
        script,
        auto_start: change.auto_start,
    };
    if running {
        let unchanged = store.get(&project.id).is_some_and(|current| {
            current.port == next.port
                && current.port_mode == next.port_mode
                && current.script == next.script
        });
        if !unchanged {
            return Err(AppError::DevServerRunning {
                name: project.name.clone(),
            });
        }
    }
    if store.set(&project.id, Some(next)) {
        config::save(&store)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::projects::ProjectKind;

    fn project(id: &str, serve: ServeProfile) -> ProjectInfo {
        ProjectInfo {
            id: id.to_string(),
            name: format!("{id} app"),
            path: format!(r"C:\nowhere\{id}"),
            domain: format!("{id}.test"),
            stack: "Astro".to_string(),
            framework: "astro".to_string(),
            detected_framework: "astro".to_string(),
            framework_overridden: false,
            docroot: format!(r"C:\nowhere\{id}"),
            serve,
            dev_port: None,
            has_hosts_entry: false,
            last_opened_at: None,
            open_count: 0,
            kind: ProjectKind::Scanned,
            missing: false,
            domain_invalid: false,
            php_version: None,
            node_version: None,
        }
    }

    fn temp_project(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rezure-test-dev-servers-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn each_port_mode_tells_the_dev_server_its_port_its_own_way() {
        use PackageManager::*;
        assert_eq!(
            command_line(Npm, "dev", PortMode::Vite, 4400),
            "npm run dev -- --port 4400 --strictPort"
        );
        assert_eq!(
            command_line(Npm, "dev", PortMode::Flag, 4400),
            "npm run dev -- --port 4400"
        );
        assert_eq!(
            command_line(Npm, "start", PortMode::Env, 4400),
            "npm run start"
        );
        assert_eq!(
            command_line(Npm, "start", PortMode::Fixed, 20128),
            "npm run start"
        );
        // Everyone but npm passes arguments on without a `--`.
        assert_eq!(
            command_line(Pnpm, "dev", PortMode::Vite, 4401),
            "pnpm run dev --port 4401 --strictPort"
        );
        assert_eq!(
            command_line(Yarn, "dev:web", PortMode::Flag, 4402),
            "yarn run dev:web --port 4402"
        );
        assert_eq!(command_line(Bun, "dev", PortMode::Env, 4403), "bun run dev");
    }

    #[test]
    fn the_lockfile_decides_the_package_manager() {
        let dir = temp_project("lockfiles");
        assert_eq!(PackageManager::detect(&dir), PackageManager::Npm);
        std::fs::write(dir.join("package-lock.json"), "{}").unwrap();
        assert_eq!(PackageManager::detect(&dir), PackageManager::Npm);
        std::fs::write(dir.join("bun.lockb"), "").unwrap();
        assert_eq!(PackageManager::detect(&dir), PackageManager::Bun);
        std::fs::write(dir.join("yarn.lock"), "").unwrap();
        assert_eq!(PackageManager::detect(&dir), PackageManager::Yarn);
        // A project migrated to pnpm often still has its old lockfile.
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(PackageManager::detect(&dir), PackageManager::Pnpm);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_script_is_the_choice_then_the_framework_then_dev_then_start() {
        let scripts = vec!["start".to_string(), "dev".to_string(), "serve".to_string()];
        assert_eq!(
            resolve_script(Some("serve"), Some("dev"), &scripts).as_deref(),
            Some("serve")
        );
        assert_eq!(
            resolve_script(None, Some("start"), &scripts).as_deref(),
            Some("start")
        );
        assert_eq!(resolve_script(None, None, &scripts).as_deref(), Some("dev"));
        // The framework's default isn't there (Create React App has `start`).
        assert_eq!(
            resolve_script(None, Some("dev"), &["start".to_string()]).as_deref(),
            Some("start")
        );
        assert_eq!(resolve_script(None, Some("dev"), &[]), None);
        // A chosen script that's gone stays chosen, so it can be reported.
        assert_eq!(
            resolve_script(Some("gone"), None, &scripts).as_deref(),
            Some("gone")
        );
    }

    #[test]
    fn package_scripts_skips_names_cmd_would_misread() {
        let dir = temp_project("scripts");
        std::fs::write(
            dir.join("package.json"),
            r#"{ "scripts": { "dev": "vite", "dev:web": "vite", "x&y": "calc", "build": "vite build" } }"#,
        )
        .unwrap();
        let mut scripts = package_scripts(&dir);
        scripts.sort();
        assert_eq!(scripts, ["build", "dev", "dev:web"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ports_are_picked_from_the_range_and_kept() {
        let mut store = DevServerStore::default();
        store.set("taken", Some(DevServerConfig::with_port(4400)));

        // 4401 is busy with something outside Rezure.
        assert!(allocate(&mut store, &["taken", "a", "b"], |port| port != 4401));
        assert_eq!(store.get("taken").unwrap().port, 4400);
        assert_eq!(store.get("a").unwrap().port, 4402);
        assert_eq!(store.get("b").unwrap().port, 4403);

        // A second pass changes nothing: ports stay where they were put.
        assert!(!allocate(&mut store, &["taken", "a", "b"], |_| true));
    }

    #[test]
    fn ports_rezure_or_another_project_uses_are_refused() {
        let mut store = DevServerStore::default();
        store.set("other", Some(DevServerConfig::with_port(4410)));
        let projects = vec![project("other", ServeProfile::Proxy)];

        for port in [80, 1023, 3306, 5432, 9100, 9135, 1025, 8025, 14330] {
            assert!(
                matches!(
                    validate_port(port, "mine", &store, &projects),
                    Err(AppError::InvalidDevPort { .. })
                ),
                "{port} must be refused"
            );
        }
        match validate_port(4410, "mine", &store, &projects) {
            Err(AppError::DevPortTaken { by, .. }) => assert_eq!(by, "other app"),
            other => panic!("expected DevPortTaken, got {other:?}"),
        }
        // Its own port, and a free one, are fine.
        assert!(validate_port(4410, "other", &store, &projects).is_ok());
        assert!(validate_port(20128, "mine", &store, &projects).is_ok());
    }

    #[test]
    fn only_proxy_projects_with_a_port_get_a_dev_server() {
        let mut store = DevServerStore::default();
        store.set("web", Some(DevServerConfig::with_port(4400)));
        store.set("php-site", Some(DevServerConfig::with_port(4401)));
        let mut gone = project("gone", ServeProfile::Proxy);
        gone.missing = true;
        store.set("gone", Some(DevServerConfig::with_port(4402)));
        let projects = vec![
            project("web", ServeProfile::Proxy),
            project("php-site", ServeProfile::Php),
            project("no-port-yet", ServeProfile::Proxy),
            gone,
        ];

        let registry = frameworks::builtin_registry();
        let specs = specs(&projects, &registry, &store, &HashMap::new());
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.project_id, "web");
        // Astro's defaults, since nothing was chosen.
        assert_eq!(spec.port_mode, PortMode::Flag);
        assert_eq!(spec.port_mode_setting, None);
        assert!(spec.auto_start);
    }

    #[test]
    fn a_project_without_package_json_says_so_first() {
        let dir = temp_project("no-package");
        let mut info = project("bare", ServeProfile::Proxy);
        info.path = dir.display().to_string();
        let mut store = DevServerStore::default();
        store.set("bare", Some(DevServerConfig::with_port(4400)));
        let spec = specs(
            &[info],
            &frameworks::builtin_registry(),
            &store,
            &HashMap::new(),
        )
        .remove(0);

        assert_eq!(issue(&spec), Some(DevServerIssue::NoPackageJson));
        assert!(matches!(
            command(&spec),
            Err(AppError::DevServerNotReady { .. })
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pinned_node_version_that_is_gone_is_reported() {
        let dir = temp_project("pinned");
        std::fs::write(
            dir.join("package.json"),
            r#"{ "scripts": { "dev": "vite" } }"#,
        )
        .unwrap();
        let mut info = project("pinned", ServeProfile::Proxy);
        info.path = dir.display().to_string();
        let mut store = DevServerStore::default();
        store.set("pinned", Some(DevServerConfig::with_port(4400)));
        let pins = HashMap::from([("pinned".to_string(), Some("0.0.1-gone".to_string()))]);
        let spec = specs(&[info], &frameworks::builtin_registry(), &store, &pins).remove(0);

        assert_eq!(
            issue(&spec),
            Some(DevServerIssue::NodeMissing {
                version: Some("0.0.1-gone".to_string())
            })
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    /// What `astro dev` printed when one was already running from a terminal
    /// — the last three lines alone were nothing but stack frames.
    #[test]
    fn a_failure_is_explained_by_its_message_not_its_stack_trace() {
        let said = summarize_failure(&lines(
            "> companyprofile@0.0.1 dev\n\
             > astro dev --port 4401\n\
             Another astro dev server is already running.\n\
             \n\
             \x20 URL:  http://localhost:4321\n\
             \x20 PID:  20584\n\
             \n\
             Run `astro dev stop` to stop it, or use `astro dev --force` to replace it.\n\
             Stack trace:\n\
             \x20 at dev (file:///C:/x/node_modules/astro/dist/cli/dev/index.js:182:10)\n\
             \x20 at async runCommand (file:///C:/x/node_modules/astro/dist/cli/index.js:170:19)",
        ));
        assert!(
            said.contains("Another astro dev server is already running."),
            "{said}"
        );
        assert!(said.contains("PID:  20584"), "{said}");
        assert!(!said.contains("at dev"), "{said}");
        assert!(
            said.contains("stop it"),
            "a fix, not just the error: {said}"
        );
    }

    #[test]
    fn a_missing_dependency_points_at_install() {
        let said = summarize_failure(&lines(
            "> vite --port 4400\n'vite' is not recognized as an internal or external command,\noperable program or batch file.",
        ));
        assert!(said.contains("'vite' is not recognized"), "{said}");
        assert!(said.contains("run Install"), "{said}");
    }

    #[test]
    fn an_unfamiliar_failure_still_shows_its_last_words_without_a_made_up_fix() {
        let said = summarize_failure(&lines("starting\nsomething odd happened"));
        assert_eq!(said, ". It said: starting / something odd happened");
        assert_eq!(summarize_failure(&[]), "");
    }

    /// A Node server started in a project folder by something other than
    /// this Rezure is found, with the port it holds — and stopping it frees
    /// the port. Needs Node.js on `PATH`. Run with
    /// `cargo test --lib services::dev_servers::tests::a_server_started_outside_rezure_is_found_and_stopped -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn a_server_started_outside_rezure_is_found_and_stopped() {
        let dir = temp_project("outside");
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut child = Command::new("node")
            .args([
                "-e",
                &format!("require('http').createServer((q,r)=>r.end()).listen({port})"),
            ])
            .current_dir(&dir)
            .spawn()
            .expect("node must be on PATH");
        std::thread::sleep(Duration::from_millis(1500));

        let mut info = project("outside", ServeProfile::Proxy);
        info.path = dir.display().to_string();
        let mut store = DevServerStore::default();
        store.set("outside", Some(DevServerConfig::with_port(4400)));
        let spec = specs(
            &[info],
            &frameworks::builtin_registry(),
            &store,
            &HashMap::new(),
        )
        .remove(0);

        // Started by this test process, so "this Rezure" has to be someone
        // else for it to count as outside.
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cwd(UpdateKind::Always),
        );
        let mut listening: HashMap<u32, Vec<u16>> = HashMap::new();
        for (bound, pid) in ports::listening_sockets() {
            listening.entry(pid).or_default().push(bound);
        }
        let found = find_outside(&sys, &listening, std::slice::from_ref(&spec), u32::MAX);
        println!("{found:?}");
        let servers = &found["outside"];
        assert_eq!(servers[0].pid, child.id());
        assert_eq!(servers[0].ports, [port]);

        // As this Rezure's own, it isn't outside.
        let own = find_outside(
            &sys,
            &listening,
            std::slice::from_ref(&spec),
            std::process::id(),
        );
        assert!(own.is_empty(), "{own:?}");

        process::kill_process_tree(child.id());
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_starts_only_a_proxy_project_that_asks_for_it() {
        let registry = frameworks::builtin_registry();
        let mut store = DevServerStore::default();
        store.set("on", Some(DevServerConfig::with_port(4400)));
        store.set(
            "off",
            Some(DevServerConfig {
                auto_start: false,
                ..DevServerConfig::with_port(4401)
            }),
        );
        let projects = [
            project("on", ServeProfile::Proxy),
            project("off", ServeProfile::Proxy),
        ];
        let specs = specs(&projects, &registry, &store, &HashMap::new());

        assert!(starts_on_open(ServeProfile::Proxy, "on", &specs));
        assert!(!starts_on_open(ServeProfile::Proxy, "off", &specs));
        // A PHP project never has a dev server to start, and a proxy one with
        // no port yet has no spec.
        assert!(!starts_on_open(ServeProfile::Php, "on", &specs));
        assert!(!starts_on_open(ServeProfile::Proxy, "no-port", &specs));
    }

    #[test]
    fn service_ids_round_trip_and_never_look_like_a_rezure_service() {
        let id = service_id("astro-web");
        assert_eq!(id, "dev-astro-web");
        assert!(is_dev_service(&id));
        assert_eq!(project_id_of(&id), Some("astro-web"));
        for own in [
            "nginx",
            "php",
            "php-8.3.0",
            "mariadb",
            "mailpit",
            "postgres",
        ] {
            assert!(!is_dev_service(own), "{own}");
        }
    }
}
