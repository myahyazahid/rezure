//! Opens a detected project in the three places a developer actually
//! reaches for from a project list: the browser, Explorer, and a terminal.
//!
//! Every entry point takes a project *id* and re-resolves it against a
//! fresh scan rather than trusting a path or domain passed in from the
//! frontend, so the only strings that reach the OS here are ones Rezure
//! produced itself from its own `www_root()` scan.

use std::path::Path;
use std::process::Command;

use super::projects::scan_projects;
use crate::db::projects::ProjectInfo;
use crate::utils::error::AppError;

fn resolve(id: &str) -> Result<ProjectInfo, AppError> {
    scan_projects()?
        .into_iter()
        .find(|project| project.id == id)
        .ok_or_else(|| AppError::ProjectNotFound(id.to_string()))
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
pub fn open_terminal(id: &str) -> Result<(), AppError> {
    let project = resolve(id)?;
    spawn_terminal(Path::new(&project.path))
}

/// Windows Terminal is the default on Windows 11 but isn't guaranteed to
/// be present (Windows 10, or an install with the app-execution alias
/// turned off), so this falls back to the console host that always is.
///
/// Neither call splices `dir` into a command line as text — `wt` receives
/// it as its own argv entry, and `start` inherits it as the working
/// directory — so spaces or quotes in a project path can't turn into extra
/// arguments.
fn spawn_terminal(dir: &Path) -> Result<(), AppError> {
    if Command::new("wt.exe").arg("-d").arg(dir).spawn().is_ok() {
        return Ok(());
    }

    let mut fallback = Command::new("cmd");
    fallback.args(["/C", "start", "cmd"]).current_dir(dir);
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
        spawn_terminal(&std::env::temp_dir()).unwrap();
    }
}
