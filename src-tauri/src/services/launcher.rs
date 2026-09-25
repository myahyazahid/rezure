//! Opens a detected project in the three places a developer actually
//! reaches for from a project list: the browser, Explorer, and a terminal.
//!
//! Every entry point takes a project *id* and re-resolves it against a
//! fresh scan rather than trusting a path or domain passed in from the
//! frontend, so the only strings that reach the OS here are ones Rezure
//! produced itself from its own `www_root()` scan.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{node, php, php_ini, projects};
use crate::db::projects::ProjectInfo;
use crate::utils::error::AppError;

fn resolve(id: &str) -> Result<ProjectInfo, AppError> {
    projects::find(id)
}

fn open_failed(target: &str, reason: impl std::fmt::Display) -> AppError {
    AppError::OpenFailed {
        target: target.to_string(),
        reason: reason.to_string(),
    }
}

/// Opens the project's domain in the default browser.
///
/// Deliberately `http://`, not `https://`: Rezure's nginx only listens on
/// port 80 (TLS is a later roadmap phase), so an https link would just
/// fail to connect.
pub fn open_site(id: &str) -> Result<(), AppError> {
    let project = resolve(id)?;
    tauri_plugin_opener::open_url(format!("http://{}", project.domain), None::<&str>)
        .map_err(|e| open_failed("the browser", e))
}

/// Reveals the project folder in Explorer.
pub fn open_folder(id: &str) -> Result<(), AppError> {
    let project = resolve(id)?;
    tauri_plugin_opener::open_path(&project.path, None::<&str>)
        .map_err(|e| open_failed("the project folder", e))
}

/// Opens a terminal already sitting in the project directory, where `php`
/// and `node`/`npm`/`npx` resolve to the versions that project uses.
///
/// `php_pin`/`node_pin` are the project's own overrides from SQLite, `None`
/// meaning "follow the global active version" — see [`terminal_env`].
pub fn open_terminal(
    id: &str,
    php_pin: Option<&str>,
    node_pin: Option<&str>,
) -> Result<(), AppError> {
    let project = resolve(id)?;
    spawn_terminal(Path::new(&project.path), &terminal_env(php_pin, node_pin))
}

/// What a spawned terminal gets on top of Rezure's own environment. Only
/// ever set on that child, never written back to this process or to the
/// machine. Empty means "exactly Rezure's own".
#[derive(Debug, Default)]
struct TerminalEnv {
    /// Put first on `PATH`, in this order.
    path_first: Vec<PathBuf>,
    vars: Vec<(&'static str, OsString)>,
}

impl TerminalEnv {
    fn is_empty(&self) -> bool {
        self.path_first.is_empty() && self.vars.is_empty()
    }
}

/// The PHP and Node.js folders a project's terminal resolves first — each
/// runtime's own rules for pin-vs-active live in [`php::terminal_bin_dir`]
/// and [`node::terminal_bin_dir`]. PHP also gets `conf.d` on its scan dir
/// ([`php_ini::terminal_scan_dir`]), so `php -m` there matches the site.
///
/// Deliberately not `OPENSSL_CONF`, even though Rezure's own PHP processes
/// get it: this is a shell the user also runs Git from, which reads the same
/// variable (see [`php_ini::OPENSSL_CONF_ENV`]).
fn terminal_env(php_pin: Option<&str>, node_pin: Option<&str>) -> TerminalEnv {
    let mut env = TerminalEnv::default();

    if let Some(dir) = php::terminal_bin_dir(php_pin) {
        env.path_first.push(dir);
        match php_ini::terminal_scan_dir() {
            Ok(value) => env.vars.push((php_ini::SCAN_DIR_ENV, value)),
            Err(err) => log::warn!("could not point a terminal's PHP at conf.d: {err}"),
        }
    }
    if let Some(dir) = node::terminal_bin_dir(node_pin) {
        env.path_first.push(dir);
    }

    env
}

/// Prepends `dirs` to the current process's own `PATH`, for handing to a
/// spawned child — never the other way around, and never written back to
/// this process's own environment. `None` only when the current `PATH`
/// can't be read *and* rejoined, which practically never happens on
/// Windows; the caller falls back to leaving `PATH` untouched either way.
fn path_with_first(dirs: &[PathBuf]) -> Option<OsString> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut entries = dirs.to_vec();
    entries.extend(std::env::split_paths(&current));
    std::env::join_paths(entries).ok()
}

/// Windows Terminal is the default on Windows 11 but isn't guaranteed to
/// be present (Windows 10, or an install with the app-execution alias
/// turned off), so this falls back to the console host that always is.
///
/// Neither call splices `dir` into a command line as text — `wt` receives
/// it as its own argv entry, and `start` inherits it as the working
/// directory — so spaces or quotes in a project path can't turn into extra
/// arguments.
///
/// # Why an environment override skips `wt.exe` entirely
///
/// Windows Terminal keeps one long-lived "monarch" process that owns every
/// window; once one is already running (the ordinary case — a user rarely
/// has zero Terminal windows open), every later `wt.exe` invocation is a
/// throwaway "peasant" that just asks the monarch to open a new tab and
/// exits. The monarch creates that tab's shell from **its own** process
/// environment — the one it had when it first started — not from whatever
/// `Command::env` set on the peasant we just spawned and which is already
/// gone. Confirmed against a real machine: a pinned/active Node version
/// silently had no effect whenever a Terminal window was already open,
/// `node -v` resolving to whatever else was on the system's own PATH
/// instead. `-w -1` and similar flags only pick which *window* the new tab
/// lands in; they don't change whose environment it's spawned from, so
/// they don't fix this.
///
/// There's no reliable way to tell from here whether a monarch already
/// exists, so any override always takes the `cmd` fallback below instead —
/// a plain console window instead of a Terminal tab, but one this function
/// spawns start to finish itself, so the environment it sets can't be
/// swallowed by another process's. No override (no PHP or Node.js installed
/// at all) keeps using `wt.exe` as before.
fn spawn_terminal(dir: &Path, env: &TerminalEnv) -> Result<(), AppError> {
    if env.is_empty() && Command::new("wt.exe").arg("-d").arg(dir).spawn().is_ok() {
        return Ok(());
    }

    let mut fallback = Command::new("cmd");
    fallback.args(["/C", "start", "cmd"]).current_dir(dir);
    if let Some(path) = path_with_first(&env.path_first) {
        fallback.env("PATH", path);
    }
    for (name, value) in &env.vars {
        fallback.env(name, value);
    }
    // The `cmd /C` that runs `start` is pure plumbing — without this it
    // flashes its own console window for the moment it lives. `start`
    // itself asks for a new console explicitly, so the terminal the user
    // actually wanted is unaffected.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        fallback.creation_flags(CREATE_NO_WINDOW);
    }

    fallback
        .spawn()
        .map(|_| ())
        .map_err(|e| open_failed("a terminal", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_project_id_is_an_error_not_a_launch() {
        let err = resolve("definitely-not-a-real-project-9f3a").unwrap_err();
        assert!(
            matches!(err, AppError::ProjectNotFound(id) if id == "definitely-not-a-real-project-9f3a"),
            "an id that isn't in the scan must never reach the OS"
        );
    }

    /// Actually opens a terminal window — run by hand with:
    /// `cargo test --lib services::launcher::tests::opens_a_real_terminal -- --ignored`
    #[test]
    #[ignore]
    fn opens_a_real_terminal() {
        spawn_terminal(&std::env::temp_dir(), &TerminalEnv::default()).unwrap();
    }

    #[test]
    fn path_with_first_puts_the_given_dirs_ahead_of_the_existing_path_in_order() {
        let php = PathBuf::from(r"C:\rezure\bin\php\7.4.33");
        let node = PathBuf::from(r"C:\rezure\bin\node\22.11.0\node-v22.11.0-win-x64");
        let joined = path_with_first(&[php.clone(), node.clone()]).unwrap();

        let mut entries = std::env::split_paths(&joined);
        assert_eq!(entries.next(), Some(php));
        assert_eq!(entries.next(), Some(node));
    }

    /// What a terminal opened for a project would get on this machine,
    /// without opening one. Run with:
    /// `cargo test --lib services::launcher::tests::print_terminal_env -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_terminal_env() {
        for php_pin in [None]
            .into_iter()
            .chain(php::installed().into_iter().map(|r| Some(r.version)))
        {
            let env = terminal_env(php_pin.as_deref(), None);
            println!("php pin {php_pin:?}: {env:#?}");
        }
    }
}
