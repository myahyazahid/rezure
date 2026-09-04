//! Answers one question: does the PHP that will serve this project have the
//! extensions the project says it needs?
//!
//! Composer already knows the answer and refuses to install without it, but
//! only at install time and only in a terminal. Everything after that is
//! silent: a Laravel app missing `ext-intl` serves a blank 500 and writes the
//! real reason into `storage/logs/laravel.log`, which is the last place a user
//! looks. Reading `composer.json`'s `ext-*` requirements back against `php -m`
//! turns that into one line, before the browser is even opened.
//!
//! The PHP it asks is deliberately the *serving* one: the active version,
//! started with the same generated ini and the same [`php_ini::SCAN_DIR_ENV`]
//! the FastCGI service gets. Asking a differently configured PHP would produce
//! an answer that is true of nothing.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use super::{php, php_ini};
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

/// One `ext-*` requirement, and whether the active PHP has it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCheck {
    /// The name as `composer.json` spells it, without the `ext-` prefix.
    pub name: String,
    pub loaded: bool,
    /// True when only `require-dev` asks for it — missing is a smaller
    /// problem there, since the served app doesn't need it to run.
    pub dev_only: bool,
}

/// What a check found, for one project.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiagnosis {
    /// The PHP the answer is about — the active version, which is also the
    /// one serving the project.
    pub php_version: String,
    /// False when the project has no `composer.json`; there is then nothing
    /// to check, which is a result rather than an error.
    pub has_composer_json: bool,
    /// Every `ext-*` requirement found, in the order they read best: missing
    /// first, then dev-only, then satisfied.
    pub extensions: Vec<ExtensionCheck>,
    /// The names a user has to act on. Derived here rather than in the UI so
    /// that what counts as "a problem" — required, not dev-only, not loaded
    /// — is defined once, next to the data it is about.
    pub missing: Vec<String>,
}

/// The subset of `extensions` the served app actually breaks without.
fn missing_from(extensions: &[ExtensionCheck]) -> Vec<String> {
    extensions
        .iter()
        .filter(|check| !check.loaded && !check.dev_only)
        .map(|check| check.name.clone())
        .collect()
}

/// Folds the spellings that mean the same extension onto one key.
///
/// `composer.json` says `ext-zend-opcache` where `php -m` prints
/// `Zend OPcache`, and `ext-pdo_mysql` matches `pdo_mysql` only once case
/// stops mattering. Dropping case, spaces, hyphens and underscores makes
/// every one of those pairs compare equal.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The `ext-*` keys in a `composer.json`, paired with whether the only place
/// asking for them is `require-dev`.
///
/// Malformed JSON returns nothing rather than an error: this runs on a file
/// the user is free to be mid-edit in, and "couldn't tell" is a better answer
/// there than a failed check.
fn required_extensions(composer_json: &str) -> Vec<(String, bool)> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(composer_json) else {
        return Vec::new();
    };

    let names = |section: &str| -> BTreeSet<String> {
        root.get(section)
            .and_then(|value| value.as_object())
            .map(|map| {
                map.keys()
                    .filter_map(|key| key.strip_prefix("ext-"))
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };

    let required = names("require");
    let dev = names("require-dev");

    let mut all: Vec<(String, bool)> = required
        .iter()
        .map(|name| (name.clone(), false))
        .chain(
            dev.into_iter()
                .filter(|name| !required.contains(name))
                .map(|name| (name, true)),
        )
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

/// The module names `php -m` prints, minus its section headers.
fn parse_modules(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('['))
        .map(normalize)
        .collect()
}

/// Runs `php -m` under exactly the configuration a served request gets.
fn loaded_modules(php_exe: &Path) -> Result<BTreeSet<String>, AppError> {
    let ini_path = php_ini::ensure_php_ini(php_exe)?;
    let output = Command::new(php_exe)
        .env(php_ini::SCAN_DIR_ENV, php_ini::ensure_conf_d()?)
        .arg("-c")
        .arg(&ini_path)
        .arg("-m")
        .hidden()
        .output()
        .map_err(|e| AppError::Io(format!("could not run {}: {e}", php_exe.display())))?;

    if !output.status.success() {
        return Err(AppError::Io(format!(
            "{} -m failed: {}",
            php_exe.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(parse_modules(&String::from_utf8_lossy(&output.stdout)))
}

/// Checks the project with this id — the same id the Projects page lists.
///
/// Resolved through the same scan every other project action goes through,
/// so a stale id from the UI is refused here rather than reading whatever
/// folder happens to be at a path the frontend supplied.
pub fn diagnose_project(id: &str) -> Result<ProjectDiagnosis, AppError> {
    let project = super::projects::scan_projects()?
        .into_iter()
        .find(|project| project.id == id)
        .ok_or_else(|| AppError::ProjectNotFound(id.to_string()))?;

    diagnose(Path::new(&project.path))
}

/// Checks one project folder against the active PHP.
pub fn diagnose(project_dir: &Path) -> Result<ProjectDiagnosis, AppError> {
    let php_exe = php::active_exe()?;
    let php_version = php::active_id();

    let Ok(composer_json) = std::fs::read_to_string(project_dir.join("composer.json")) else {
        return Ok(ProjectDiagnosis {
            php_version,
            has_composer_json: false,
            extensions: Vec::new(),
            missing: Vec::new(),
        });
    };

    let required = required_extensions(&composer_json);
    // Nothing to ask PHP about, so don't pay for the process.
    if required.is_empty() {
        return Ok(ProjectDiagnosis {
            php_version,
            has_composer_json: true,
            extensions: Vec::new(),
            missing: Vec::new(),
        });
    }

    let loaded = loaded_modules(&php_exe)?;
    let mut extensions: Vec<ExtensionCheck> = required
        .into_iter()
        .map(|(name, dev_only)| ExtensionCheck {
            loaded: loaded.contains(&normalize(&name)),
            name,
            dev_only,
        })
        .collect();

    // Whatever needs acting on comes first: a list that opens with the two
    // broken ones gets read, a list that buries them under twelve satisfied
    // ones gets skimmed.
    extensions.sort_by_key(|check| (check.loaded, check.dev_only));

    Ok(ProjectDiagnosis {
        php_version,
        has_composer_json: true,
        missing: missing_from(&extensions),
        extensions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LARAVEL_ISH: &str = r#"{
        "require": {
            "php": "^8.2",
            "ext-intl": "*",
            "ext-redis": "*",
            "laravel/framework": "^11.0"
        },
        "require-dev": {
            "ext-xdebug": "*",
            "phpunit/phpunit": "^11.0"
        }
    }"#;

    #[test]
    fn only_ext_requirements_are_read_and_dev_only_ones_are_marked() {
        let found = required_extensions(LARAVEL_ISH);

        assert_eq!(
            found,
            vec![
                ("intl".to_string(), false),
                ("redis".to_string(), false),
                ("xdebug".to_string(), true),
            ],
            "packages and the `php` constraint itself are not extensions"
        );
    }

    /// A file being edited is not a failure worth surfacing.
    #[test]
    fn malformed_composer_json_yields_nothing_rather_than_an_error() {
        assert!(required_extensions("{ not json").is_empty());
        assert!(required_extensions("").is_empty());
    }

    /// An extension named in both sections is a real requirement, not a
    /// dev-only one.
    #[test]
    fn a_requirement_in_both_sections_is_not_dev_only() {
        let both = r#"{"require":{"ext-intl":"*"},"require-dev":{"ext-intl":"*"}}"#;
        assert_eq!(required_extensions(both), vec![("intl".to_string(), false)]);
    }

    /// The spellings that would otherwise report a loaded extension as
    /// missing: `php -m` and `composer.json` don't agree on case, spaces or
    /// separators.
    #[test]
    fn module_names_match_across_the_spellings_the_two_sides_use() {
        let modules =
            parse_modules("[PHP Modules]\nCore\npdo_mysql\nZend OPcache\n\n[Zend Modules]\n");

        assert!(modules.contains(&normalize("pdo_mysql")));
        assert!(modules.contains(&normalize("zend-opcache")));
        assert!(modules.contains(&normalize("Core")));
        // Section headers are not modules.
        assert!(!modules.iter().any(|m| m.contains("phpmodules")));
    }

    /// The list has to open with what's broken, or it doesn't get read.
    #[test]
    fn missing_requirements_sort_ahead_of_satisfied_and_dev_only_ones() {
        let mut checks = vec![
            ExtensionCheck {
                name: "curl".into(),
                loaded: true,
                dev_only: false,
            },
            ExtensionCheck {
                name: "xdebug".into(),
                loaded: false,
                dev_only: true,
            },
            ExtensionCheck {
                name: "redis".into(),
                loaded: false,
                dev_only: false,
            },
        ];
        checks.sort_by_key(|check| (check.loaded, check.dev_only));

        let order: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(order, vec!["redis", "xdebug", "curl"]);

        // Only the one the served app actually needs: a dev-only miss is
        // not something a running site is broken by.
        assert_eq!(missing_from(&checks), vec!["redis".to_string()]);
    }

    /// Runs the whole check against the really-installed PHP, on a project
    /// synthesized to need one extension that is on by default and one that
    /// no official Windows build ships. Run with:
    /// `cargo test --lib services::doctor::tests::print_diagnosis -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_diagnosis() {
        let dir = std::env::temp_dir().join("rezure-test-doctor-real");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("composer.json"),
            r#"{"require":{"php":"^8.2","ext-intl":"*","ext-redis":"*","ext-mbstring":"*"},
                "require-dev":{"ext-xdebug":"*"}}"#,
        )
        .unwrap();

        let diagnosis = diagnose(&dir).unwrap();
        println!("php {}", diagnosis.php_version);
        for check in &diagnosis.extensions {
            println!(
                "  {} {}{}",
                if check.loaded { "ok  " } else { "MISS" },
                check.name,
                if check.dev_only { " (dev only)" } else { "" }
            );
        }
        println!("missing: {:?}", diagnosis.missing);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A project with no `composer.json` is a result, not a failure — most of
    /// `www` is WordPress and static folders.
    #[test]
    fn a_project_without_composer_json_reports_nothing_to_check() {
        let dir = std::env::temp_dir().join(format!("rezure-test-doctor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Skipped when no PHP is installed: the check is about the active
        // one, and there is nothing to be active.
        if php::active_exe().is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        let diagnosis = diagnose(&dir).unwrap();
        assert!(!diagnosis.has_composer_json);
        assert!(diagnosis.extensions.is_empty());
        assert!(diagnosis.missing.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
