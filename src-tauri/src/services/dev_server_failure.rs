//! Why a dev server didn't come up, in the parts the UI lays out apart: what
//! happened, the cause when it's a familiar one, the step that fixes it, and
//! the lines of its output that explain it.
//!
//! The raw output alone was specific but hard to act on — "Cannot find
//! module '@astrojs/node' imported from '…'" followed by a location inside
//! Vite, with a generic "dependencies look incomplete" after it. Naming the
//! package is what makes the fix obvious, and checking `package.json` is what
//! makes it *right*: Install only helps when the package is already listed
//! there. One that isn't has to be added, and saying "run Install" for it
//! sends the user round in circles.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use super::dev_servers::PackageManager;

/// How many explanatory lines of output a failure keeps.
const OUTPUT_LINES: usize = 12;

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

/// Why a dev server's last start failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFailure {
    /// What was seen happen: "it exited (exit code: 1)", "nothing answered
    /// on :4401 after 300s".
    pub what: String,
    /// The cause, when it's one Rezure recognizes — "@astrojs/node isn't
    /// installed". A sentence, capitalized, no trailing period.
    pub cause: Option<String>,
    /// The step that fixes a recognized cause.
    pub fix: Option<FailureFix>,
    /// What it printed that explains the failure, oldest first — stack
    /// frames, npm's banners and code locations taken out.
    pub output: Vec<String>,
}

/// The recommended next step for a recognized failure. Each one maps to an
/// action the UI can offer as a button, not just a sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FailureFix {
    /// `package.json` lists what's missing, so installing brings it.
    /// `package` is `None` when the missing thing is a command
    /// (`'vite' is not recognized`), not a package name.
    Install { package: Option<String> },
    /// The code imports a package `package.json` doesn't list: Install
    /// wouldn't bring it. `command` adds it — shown to copy, never run by
    /// Rezure, since the name comes from the dev server's own output.
    #[serde(rename_all = "camelCase")]
    AddDependency { package: String, command: String },
    /// Another copy of this dev server is already running.
    StopOther,
    /// It came up on a port of its own choosing instead of the one set.
    #[serde(rename_all = "camelCase")]
    UsePort { found: Vec<u16>, expected: u16 },
}

impl FailureFix {
    /// The step, as a sentence ending without a period.
    pub fn describe(&self) -> String {
        match self {
            Self::Install { .. } => "Run Install in Dev servers, then start it again".to_string(),
            Self::AddDependency { command, .. } => {
                format!("Add it with `{command}` in the project folder, then start it again")
            }
            Self::StopOther => {
                "Stop the other copy (Dev servers offers to), then start it again".to_string()
            }
            Self::UsePort { found, expected } => format!(
                "Set {} as its port in Dev servers, or make the app use :{expected}",
                ports_text(found)
            ),
        }
    }
}

impl StartFailure {
    /// A failure Rezure saw directly, with no output to read — the port
    /// check rather than the dev server's own words.
    pub fn wrong_port(found: Vec<u16>, expected: u16) -> Self {
        Self {
            what: format!(
                "it's listening on {} instead of :{expected}",
                ports_text(&found)
            ),
            cause: None,
            fix: Some(FailureFix::UsePort { found, expected }),
            output: Vec::new(),
        }
    }

    /// A failure with nothing more to say than what happened.
    pub fn plain(what: String) -> Self {
        Self {
            what,
            cause: None,
            fix: None,
            output: Vec::new(),
        }
    }

    /// The text `AppError::DevServerFailed` carries: the cause and its fix
    /// when recognized, otherwise what happened and the first line that
    /// explains it. Short on purpose — the rest is in [`Self::output`].
    pub fn reason(&self) -> String {
        let mut text = match (&self.cause, self.output.first()) {
            (Some(cause), _) => cause.clone(),
            (None, Some(line)) => format!("{}: {line}", self.what),
            (None, None) => self.what.clone(),
        };
        if let Some(fix) = &self.fix {
            text.push_str(". ");
            text.push_str(&fix.describe());
        }
        text
    }
}

fn ports_text(ports: &[u16]) -> String {
    ports
        .iter()
        .map(|port| format!(":{port}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads a dev server's last output (`lines`) after `what` happened, for
/// the project in `dir` run with `pm`.
pub fn diagnose(what: String, lines: &[String], dir: &Path, pm: PackageManager) -> StartFailure {
    let output = explaining_lines(lines);
    let all = lines.join("\n").to_lowercase();

    let (cause, fix) = if all.contains("already running")
        || all.contains("eaddrinuse")
        || all.contains("address already in use")
        || all.contains("acquire lock")
        || all.contains("another instance")
    {
        (
            Some("Another copy of this dev server is already running".to_string()),
            Some(FailureFix::StopOther),
        )
    } else if let Some(missing) = lines.iter().find_map(|line| missing_module(line)) {
        explain_missing_module(&missing, &declared_dependencies(dir), pm)
    } else if let Some(command) = lines.iter().find_map(|line| unknown_command(line)) {
        (
            Some(format!(
                "`{command}` isn't installed in the project's node_modules"
            )),
            Some(FailureFix::Install { package: None }),
        )
    } else {
        (None, None)
    };

    StartFailure {
        what,
        cause,
        fix,
        output,
    }
}

/// A module that couldn't be found, and the file that imported it if the
/// message says.
#[derive(Debug, PartialEq, Eq)]
struct Missing {
    specifier: String,
    imported_by: Option<String>,
}

/// The cause and fix for a module that couldn't be found.
fn explain_missing_module(
    missing: &Missing,
    declared: &HashSet<String>,
    pm: PackageManager,
) -> (Option<String>, Option<FailureFix>) {
    let by = missing
        .imported_by
        .as_deref()
        .map(|file| format!(" (imported by {file})"))
        .unwrap_or_default();

    let Some(package) = package_name(&missing.specifier) else {
        // A relative or absolute path: the project's own file is missing,
        // which no install fixes.
        return (
            Some(format!(
                "A file it imports doesn't exist: {}{by}",
                missing.specifier
            )),
            None,
        );
    };

    if declared.contains(&package) {
        (
            Some(format!("{package} isn't installed{by}")),
            Some(FailureFix::Install {
                package: Some(package),
            }),
        )
    } else {
        let command = format!("{} {} {package}", pm.command(), pm.add_verb());
        (
            Some(format!(
                "{package} is imported{by} but isn't in package.json"
            )),
            Some(FailureFix::AddDependency { package, command }),
        )
    }
}

/// The module named in a "cannot find module/package" message — Vite's
/// (`Cannot find module 'x' imported from 'file'`), Node's ESM loader
/// (`Cannot find package 'x' imported from file`) and CommonJS's
/// (`Cannot find module 'x'`).
fn missing_module(line: &str) -> Option<Missing> {
    // ASCII-only lowercasing keeps every byte where it was, so indices found
    // in `lower` are valid in `line`.
    let lower = line.to_ascii_lowercase();
    let start = ["cannot find module ", "cannot find package "]
        .iter()
        .find_map(|marker| lower.find(marker).map(|at| at + marker.len()))?;
    let rest = &line[start..];
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = &rest[1..];
    let end = inner.find(quote)?;
    let specifier = inner[..end].to_string();
    if specifier.is_empty() {
        return None;
    }

    let after = &inner[end + 1..];
    let imported_by = after
        .to_ascii_lowercase()
        .find("imported from ")
        .map(|at| after[at + "imported from ".len()..].trim())
        .map(|file| file.trim_matches(|c| matches!(c, '\'' | '"')))
        .and_then(|file| file.rsplit(['/', '\\']).next())
        .filter(|name| !name.is_empty())
        .map(str::to_string);

    Some(Missing {
        specifier,
        imported_by,
    })
}

/// The npm package a module specifier belongs to — `@astrojs/node` for
/// `@astrojs/node/server`, `lodash` for `lodash/merge` — or `None` for a
/// path, a Node built-in or anything that isn't a valid package name.
///
/// The result is shown inside a command for the user to copy, so it must
/// be a real package name and nothing more.
fn package_name(specifier: &str) -> Option<String> {
    if specifier.starts_with(['.', '/', '\\'])
        || specifier.starts_with("node:")
        || specifier.starts_with("file:")
        || specifier.get(1..3) == Some(":\\")
        || specifier.get(1..3) == Some(":/")
    {
        return None;
    }
    let mut parts = specifier.split('/');
    let first = parts.next()?;
    let name = if first.starts_with('@') {
        format!("{first}/{}", parts.next()?)
    } else {
        first.to_string()
    };

    let valid = name.len() <= 214
        && name.trim_start_matches('@').split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part.chars().all(|c| {
                    c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || matches!(c, '-' | '.' | '_' | '~')
                })
        });
    valid.then_some(name)
}

/// The command `cmd.exe` couldn't find — `vite` in `'vite' is not
/// recognized as an internal or external command`.
fn unknown_command(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    if let Some(at) = lower.find("' is not recognized as an internal or external command") {
        let name = line[..at].rsplit('\'').next()?.trim();
        return (!name.is_empty()).then(|| name.to_string());
    }
    // sh's form, from a script that shells out: "sh: vite: command not found".
    let at = lower.find(": command not found")?;
    let name = line[..at].rsplit(':').next()?.trim();
    (!name.is_empty() && !name.contains(' ')).then(|| name.to_string())
}

/// Every package `dir\package.json` lists, in any dependency section.
fn declared_dependencies(dir: &Path) -> HashSet<String> {
    let Ok(content) = std::fs::read_to_string(dir.join("package.json")) else {
        return HashSet::new();
    };
    let Ok(json) =
        serde_json::from_str::<serde_json::Value>(content.trim_start_matches('\u{feff}'))
    else {
        return HashSet::new();
    };
    [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ]
    .iter()
    .filter_map(|section| json.get(section).and_then(serde_json::Value::as_object))
    .flat_map(|deps| deps.keys().cloned())
    .collect()
}

/// The lines of `lines` that explain a failure: from the first one that
/// reads like an error, with stack frames, npm's banners and bare code
/// locations taken out. The line worth reading is usually above a dozen
/// `at …` lines, and the last few lines alone are nothing but frames.
fn explaining_lines(lines: &[String]) -> Vec<String> {
    let meaningful: Vec<&str> = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !is_noise(line))
        .collect();
    let first_error = meaningful.iter().position(|line| {
        let lower = line.to_lowercase();
        ERROR_WORDS.iter().any(|word| lower.contains(word))
    });
    let start = first_error.unwrap_or(meaningful.len().saturating_sub(3));
    meaningful
        .iter()
        .skip(start)
        .take(OUTPUT_LINES)
        .map(|line| line.to_string())
        .collect()
}

/// A line that says where in some code something happened, or that a tool
/// prints around every failure — never what the failure was.
fn is_noise(line: &str) -> bool {
    let lower = line.to_lowercase();
    line.is_empty()
        || line.starts_with("at ")
        || line.starts_with("> ")
        || lower.starts_with("npm error")
        || lower.starts_with("npm err!")
        || lower.trim_end_matches(':') == "location"
        || lower.trim_end_matches(':') == "stack trace"
        || is_code_location(line)
}

/// `C:\x\node_modules\vite\dist\node\chunks\node.js:36285:32`, or the same
/// as a `file:///` URL — a position in a file, alone on its line.
fn is_code_location(line: &str) -> bool {
    let looks_like_path = line.get(1..3).is_some_and(|s| s == ":\\" || s == ":/")
        || line.starts_with("file:")
        || line.starts_with('/');
    if !looks_like_path {
        return false;
    }
    let mut rest = line;
    let mut numbers = 0;
    while let Some((head, tail)) = rest.rsplit_once(':') {
        if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        rest = head;
        numbers += 1;
    }
    numbers > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    fn project(name: &str, package_json: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rezure-failure-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), package_json).unwrap();
        dir
    }

    /// The case that started this: what `astro dev` printed with
    /// `@astrojs/node` listed in package.json but not installed yet.
    const ASTRO_MISSING_ADAPTER: &str = "> company-profile@0.0.1 dev\n\
        > astro dev --port 4401\n\
        [astro] Unable to load your Astro config\n\
        \n\
        Cannot find module '@astrojs/node' imported from 'C:/repository/whitescale/company-profile/astro.config.mjs'\n\
        Location:\n\
        \x20 C:\\repository\\whitescale\\company-profile\\node_modules\\vite\\dist\\node\\chunks\\node.js:36285:32\n\
        Stack trace:\n\
        \x20 at nodeImport (file:///C:/x/node_modules/vite/dist/node/chunks/node.js:36285:32)";

    #[test]
    fn a_declared_but_missing_package_is_named_and_points_at_install() {
        let dir = project(
            "declared",
            r#"{"dependencies":{"@astrojs/node":"^11.1.9","astro":"^7.4.2"}}"#,
        );
        let failure = diagnose(
            "it exited (exit code: 1)".to_string(),
            &lines(ASTRO_MISSING_ADAPTER),
            &dir,
            PackageManager::Npm,
        );
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(
            failure.cause.as_deref(),
            Some("@astrojs/node isn't installed (imported by astro.config.mjs)")
        );
        assert_eq!(
            failure.fix,
            Some(FailureFix::Install {
                package: Some("@astrojs/node".to_string())
            })
        );
        assert_eq!(
            failure.output,
            [
                "[astro] Unable to load your Astro config",
                "Cannot find module '@astrojs/node' imported from 'C:/repository/whitescale/company-profile/astro.config.mjs'",
            ]
        );
        assert_eq!(
            failure.reason(),
            "@astrojs/node isn't installed (imported by astro.config.mjs). Run Install in Dev servers, then start it again"
        );
    }

    /// Install can't bring what package.json doesn't list — the fix is to
    /// add it, with the project's own package manager.
    #[test]
    fn an_undeclared_package_is_to_be_added_not_installed() {
        let dir = project("undeclared", r#"{"dependencies":{"astro":"^7.4.2"}}"#);
        let failure = diagnose(
            "it exited (exit code: 1)".to_string(),
            &lines(ASTRO_MISSING_ADAPTER),
            &dir,
            PackageManager::Pnpm,
        );
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(
            failure.fix,
            Some(FailureFix::AddDependency {
                package: "@astrojs/node".to_string(),
                command: "pnpm add @astrojs/node".to_string(),
            })
        );
        assert!(
            failure.reason().contains("isn't in package.json"),
            "{}",
            failure.reason()
        );
    }

    #[test]
    fn a_missing_local_file_gets_no_install_advice() {
        let failure = diagnose(
            "it exited (exit code: 1)".to_string(),
            &lines("Error: Cannot find module './routes/home' imported from '/app/src/main.js'"),
            Path::new(r"C:\nowhere"),
            PackageManager::Npm,
        );
        assert_eq!(
            failure.cause.as_deref(),
            Some("A file it imports doesn't exist: ./routes/home (imported by main.js)")
        );
        assert_eq!(failure.fix, None);
    }

    #[test]
    fn package_names_are_taken_from_specifiers() {
        assert_eq!(
            package_name("@astrojs/node/server").as_deref(),
            Some("@astrojs/node")
        );
        assert_eq!(package_name("lodash/merge").as_deref(), Some("lodash"));
        assert_eq!(package_name("./x"), None);
        assert_eq!(package_name("node:fs"), None);
        assert_eq!(package_name(r"C:\x\y.js"), None);
        // Never anything a shell would read as more than a name.
        assert_eq!(package_name("evil & del"), None);
        assert_eq!(package_name("@scope"), None);
    }

    /// What `astro dev` printed when one was already running from a terminal
    /// — the last three lines alone were nothing but stack frames.
    #[test]
    fn a_failure_is_explained_by_its_message_not_its_stack_trace() {
        let failure = diagnose(
            "it exited (exit code: 1)".to_string(),
            &lines(
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
            ),
            Path::new(r"C:\nowhere"),
            PackageManager::Npm,
        );
        assert_eq!(
            failure.output[0],
            "Another astro dev server is already running."
        );
        assert!(failure.output.iter().any(|line| line == "PID:  20584"));
        assert!(failure.output.iter().all(|line| !line.starts_with("at ")));
        assert_eq!(failure.fix, Some(FailureFix::StopOther));
    }

    #[test]
    fn a_missing_command_points_at_install() {
        let failure = diagnose(
            "it exited (exit code: 1)".to_string(),
            &lines(
                "> vite --port 4400\n'vite' is not recognized as an internal or external command,\noperable program or batch file.",
            ),
            Path::new(r"C:\nowhere"),
            PackageManager::Npm,
        );
        assert_eq!(
            failure.cause.as_deref(),
            Some("`vite` isn't installed in the project's node_modules")
        );
        assert_eq!(failure.fix, Some(FailureFix::Install { package: None }));
    }

    #[test]
    fn an_unfamiliar_failure_shows_its_words_without_a_made_up_fix() {
        let failure = diagnose(
            "it exited (exit code: 1)".to_string(),
            &lines("starting\nsomething odd happened"),
            Path::new(r"C:\nowhere"),
            PackageManager::Npm,
        );
        assert_eq!(failure.cause, None);
        assert_eq!(failure.fix, None);
        assert_eq!(failure.output, ["starting", "something odd happened"]);
        assert_eq!(failure.reason(), "it exited (exit code: 1): starting");

        let silent = diagnose(
            "it exited".to_string(),
            &[],
            Path::new(r"C:\nowhere"),
            PackageManager::Npm,
        );
        assert_eq!(silent.reason(), "it exited");
    }

    #[test]
    fn a_wrong_port_says_which_and_how_to_fix_it() {
        let failure = StartFailure::wrong_port(vec![4321], 4401);
        assert_eq!(
            failure.reason(),
            "it's listening on :4321 instead of :4401. Set :4321 as its port in Dev servers, or make the app use :4401"
        );
    }

    #[test]
    fn code_locations_are_noise_but_messages_with_colons_are_not() {
        assert!(is_code_location(
            r"C:\x\node_modules\vite\dist\node.js:36285:32"
        ));
        assert!(is_code_location("file:///C:/x/index.js:182:10"));
        assert!(!is_code_location("Error: something: failed"));
        assert!(!is_code_location("URL:  http://localhost:4321"));
    }
}
