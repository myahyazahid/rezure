//! Small helpers for the places Rezure has to reach Windows through
//! PowerShell — things Win32 exposes but `std` doesn't (directory
//! junctions, the user's `Environment` registry key, `WM_SETTINGCHANGE`).

use std::process::Command;

use crate::utils::error::AppError;

/// Wraps `s` as a single-quoted PowerShell string literal, doubling any
/// embedded single quotes — PowerShell's own escape for that context.
///
/// Every path Rezure hands to PowerShell goes through this. Windows paths
/// routinely contain spaces (`C:\Users\Jane Doe\…`), and an unquoted one
/// splits into several arguments — a failure mode this codebase has already
/// been bitten by once, in the hosts-file writer.
pub fn quote_ps(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Runs `script` through PowerShell and returns its stdout.
///
/// The whole script is passed as a *single* argv entry, so `std` quotes it
/// correctly and nothing re-splits it; any path inside must still be
/// wrapped with [`quote_ps`].
pub fn run(script: &str, what: &str) -> Result<String, AppError> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| AppError::Io(format!("could not run powershell: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no error output")
            .trim();
        return Err(AppError::Io(format!("{what} failed: {detail}")));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ps_doubles_embedded_single_quotes() {
        assert_eq!(quote_ps("C:/plain/path"), "'C:/plain/path'");
        assert_eq!(quote_ps("it's here"), "'it''s here'");
    }

    #[test]
    fn quote_ps_keeps_a_spaced_path_as_one_literal() {
        assert_eq!(
            quote_ps(r"C:\Users\Jane Doe\rezure"),
            r"'C:\Users\Jane Doe\rezure'"
        );
    }

    #[test]
    fn run_returns_stdout_and_reports_failures() {
        assert_eq!(run("Write-Output 'hi'", "test").unwrap(), "hi");
        assert!(run("exit 1", "test").is_err());
    }
}
