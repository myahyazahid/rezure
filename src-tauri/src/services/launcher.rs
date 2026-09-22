//! Opens a detected project in the three places a developer actually
//! reaches for from a project list: the browser, Explorer, and a terminal.
//!
//! Every entry point takes a project *id* and re-resolves it against a
//! fresh scan rather than trusting a path or domain passed in from the
//! frontend, so the only strings that reach the OS here are ones Rezure
//! produced itself from its own `www_root()` scan.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use super::projects;
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

/// Opens a terminal already sitting in the project directory.
///
/// `node_bin_dir`, when given, is put first on the new terminal's `PATH` —
/// see `commands::projects::resolve_node_bin_dir` for how it's picked.
/// `None` means "leave `PATH` exactly as Rezure's own process has it",
/// same as before this parameter existed.
pub fn open_terminal(id: &str, node_bin_dir: Option<&Path>) -> Result<(), AppError> {
    let project = resolve(id)?;
    spawn_terminal(Path::new(&project.path), node_bin_dir)
}

/// Prepends `dir` to the current process's own `PATH`, for handing to a
/// spawned child — never the other way around, and never written back to
/// this process's own environment. `None` only when the current `PATH`
/// can't be read *and* rejoined, which practically never happens on
/// Windows; the caller falls back to leaving `PATH` untouched either way.
fn path_with_first(dir: &Path) -> Option<OsString> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut entries = vec![dir.to_path_buf()];
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
/// # Why a PATH override skips `wt.exe` entirely
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
/// exists, so a PATH override always takes the `cmd` fallback below
/// instead — a plain console window instead of a Terminal tab, but one this
/// function spawns start to finish itself, so the environment it sets
/// can't be swallowed by another process's. No override (the ordinary
/// terminal, no project Node version in play) keeps using `wt.exe` as
/// before.
fn spawn_terminal(dir: &Path, node_bin_dir: Option<&Path>) -> Result<(), AppError> {
    let path_override = node_bin_dir.and_then(path_with_first);

    if path_override.is_none() && Command::new("wt.exe").arg("-d").arg(dir).spawn().is_ok() {
        return Ok(());
    }

    let mut fallback = Command::new("cmd");
    fallback.args(["/C", "start", "cmd"]).current_dir(dir);
    if let Some(path) = &path_override {
        fallback.env("PATH", path);
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
        spawn_terminal(&std::env::temp_dir(), None).unwrap();
    }

    #[test]
    fn path_with_first_puts_the_given_dir_ahead_of_the_existing_path() {
        let dir = Path::new(r"C:\rezure\bin\node\22.11.0\node-v22.11.0-win-x64");
        let joined = path_with_first(dir).unwrap();
        let joined = joined.to_string_lossy();
        assert!(
            joined.starts_with(&*dir.to_string_lossy()),
            "the given dir must be first, got: {joined}"
        );
    }
}
