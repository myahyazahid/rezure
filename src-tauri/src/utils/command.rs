//! Keeps spawned processes from flashing a console window.
//!
//! Rezure is a GUI-subsystem binary, so it owns no console. When it spawns a
//! console-subsystem child — `php-cgi.exe`, `mysqld.exe`, `netstat`,
//! `powershell` — Windows allocates a *new* console for that child and shows
//! it. On a status poll that runs every few seconds, or on a service start,
//! the user sees black windows blinking across the screen.
//!
//! `CREATE_NO_WINDOW` suppresses that console entirely. It does not affect
//! piped stdio: the child still writes to the pipes we hand it, we just never
//! get a window we didn't ask for.
//!
//! The two places that *want* a visible console — opening a terminal in a
//! project folder, and the database CLI — pass `CREATE_NEW_CONSOLE` on
//! purpose and deliberately do not use this trait.

use std::process::Command;

/// `CREATE_NO_WINDOW` — run the child without allocating a console.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Adds [`hidden`](HiddenWindow::hidden) to `Command`.
pub trait HiddenWindow {
    /// Spawns without a console window. A no-op off Windows, so call sites
    /// stay free of `#[cfg]` blocks.
    fn hidden(&mut self) -> &mut Self;
}

impl HiddenWindow for Command {
    fn hidden(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(CREATE_NO_WINDOW);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_still_runs_the_command_and_captures_stdout() {
        // The flag must not disturb piped stdio — that's the whole point of
        // preferring it over hiding a window after the fact.
        let output = if cfg!(windows) {
            Command::new("cmd")
                .args(["/C", "echo hi"])
                .hidden()
                .output()
        } else {
            Command::new("echo").arg("hi").hidden().output()
        }
        .expect("command should run");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hi");
    }
}
