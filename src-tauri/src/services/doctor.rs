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
//! started with the same generated ini and the same environment
//! ([`php_ini::apply_process_env`]) the FastCGI service gets. Asking a
//! differently configured PHP would produce an answer that is true of nothing.
//!
//! It also asks one question no `composer.json` states: can that PHP verify
//! an HTTPS certificate at all? Without a CA bundle the answer is no, and the
//! symptom — cURL error 60 on the first outbound API call — lands far from
//! the cause. See [`TlsCheck`].

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use super::process::MAILPIT_SMTP_PORT;
use super::{ca_bundle, php, php_ini};
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
    /// How the project's `.env` sends mail, when it's to an SMTP server meant
    /// to be on this machine — the case Mailpit catches. `None` for anything
    /// else (`log`, a real provider, no `.env`).
    pub mail: Option<MailSetup>,
}

/// A project's local-SMTP mail settings, checked against Mailpit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSetup {
    /// `MAIL_HOST` as written.
    pub host: String,
    /// `MAIL_PORT`, when it's set to a number.
    pub port: Option<u16>,
    /// False for `mailpit` — the Docker service name Laravel Sail writes,
    /// which resolves inside Sail's network and nowhere else.
    pub host_reachable: bool,
    /// Whether `port` is the one Mailpit accepts mail on.
    pub port_matches: bool,
    /// Filled in by the command layer, which is what knows about services.
    pub mailpit_installed: bool,
    pub mailpit_running: bool,
}

/// The value of `key` in a `.env` file, read the way phpdotenv (Laravel's
/// loader) reads it for the plain cases: the first definition wins, an
/// optional `export ` prefix is ignored, surrounding quotes are dropped, and
/// an unquoted value ends at a ` #` comment.
fn env_value(env: &str, key: &str) -> Option<String> {
    env.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        let value = value.trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
        Some(match unquoted {
            Some(inner) => inner.to_string(),
            None => value
                .split(" #")
                .next()
                .unwrap_or_default()
                .trim()
                .to_string(),
        })
    })
}

/// Reads a project's `.env` for mail sent to a local SMTP server. Only the
/// `smtp` mailer on a local host (or Sail's `mailpit` hostname) counts —
/// `log`, `array` and real providers are deliberate choices with nothing to
/// fix. `MAIL_DRIVER` is what Laravel called the setting before 7.x.
pub fn mail_setup(env: &str) -> Option<MailSetup> {
    let mailer = env_value(env, "MAIL_MAILER").or_else(|| env_value(env, "MAIL_DRIVER"))?;
    if !mailer.eq_ignore_ascii_case("smtp") {
        return None;
    }
    let host = env_value(env, "MAIL_HOST")?;
    let host_reachable = match host.to_ascii_lowercase().as_str() {
        "127.0.0.1" | "localhost" | "::1" => true,
        "mailpit" => false,
        _ => return None,
    };
    let port = env_value(env, "MAIL_PORT").and_then(|port| port.parse().ok());
    Some(MailSetup {
        host,
        port,
        host_reachable,
        port_matches: port == Some(MAILPIT_SMTP_PORT),
        mailpit_installed: false,
        mailpit_running: false,
    })
}

/// What an outbound HTTPS request from the serving PHP ran into.
///
/// A separate check from [`diagnose`], not a field of it: it's about the PHP,
/// not the project — a WordPress site with no `composer.json` breaks on it
/// just the same — and it waits on the network, which the extension check
/// shouldn't have to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TlsOutcome {
    /// The certificate verified.
    Verified,
    /// The connection worked but the certificate couldn't be verified — the
    /// cURL error 60 case, almost always a missing or stale CA bundle.
    Untrusted,
    /// Never got as far as a certificate (offline, DNS, a firewall). Says
    /// nothing about the bundle either way.
    Unreachable,
    /// Couldn't be tested: `curl` isn't loaded, or PHP didn't run.
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TlsCheck {
    /// Whether `etc/cacert.pem` exists — reported on its own because it's
    /// the fix, and it's knowable even when the network test is not.
    pub bundle_installed: bool,
    pub outcome: TlsOutcome,
    /// cURL's own message, for anything but [`TlsOutcome::Verified`].
    pub detail: Option<String>,
}

/// Where the HTTPS test goes. Packagist because it's what Composer talks to
/// first — a PHP that can't reach it can't install anything either.
const TLS_PROBE_URL: &str = "https://repo.packagist.org/packages.json";

/// Prints `<curl errno> <curl error>`, or `nocurl`. No double quotes, so it
/// passes through Windows argument quoting untouched. `{url}` is replaced
/// with [`TLS_PROBE_URL`].
const TLS_PROBE_SCRIPT: &str = "if (!function_exists('curl_init')) { echo 'nocurl'; exit; } \
    $c = curl_init('{url}'); \
    curl_setopt_array($c, [CURLOPT_NOBODY => true, CURLOPT_CONNECTTIMEOUT => 4, CURLOPT_TIMEOUT => 6]); \
    curl_exec($c); \
    echo curl_errno($c), ' ', curl_error($c);";

/// cURL error codes that mean "reached the server, didn't trust it":
/// 60 `CURLE_PEER_FAILED_VERIFICATION`, 77 `CURLE_SSL_CACERT_BADFILE`.
const UNTRUSTED_CODES: [u32; 2] = [60, 77];

/// Reads the probe's output into an outcome and, unless it verified, why.
fn classify_tls(output: &str) -> (TlsOutcome, Option<String>) {
    let output = output.trim();
    if output == "nocurl" {
        return (
            TlsOutcome::Unavailable,
            Some("the curl extension isn't loaded".to_string()),
        );
    }
    let (code, message) = output.split_once(' ').unwrap_or((output, ""));
    let message = Some(message.trim().to_string()).filter(|m| !m.is_empty());
    match code.parse::<u32>() {
        Ok(0) => (TlsOutcome::Verified, None),
        Ok(code) if UNTRUSTED_CODES.contains(&code) => (TlsOutcome::Untrusted, message),
        Ok(_) => (TlsOutcome::Unreachable, message),
        Err(_) => (
            TlsOutcome::Unavailable,
            Some(format!("unexpected output from PHP: {output}")),
        ),
    }
}

/// Runs the HTTPS probe under the serving configuration. Never an error:
/// whatever goes wrong is itself the finding.
fn check_tls(php_exe: &Path) -> TlsCheck {
    let script = TLS_PROBE_SCRIPT.replace("{url}", TLS_PROBE_URL);
    let ran = serving_php(php_exe).and_then(|mut cmd| {
        cmd.arg("-r")
            .arg(&script)
            .output()
            .map_err(|e| AppError::Io(format!("could not run {}: {e}", php_exe.display())))
    });
    let (outcome, detail) = match ran {
        Ok(output) => classify_tls(&String::from_utf8_lossy(&output.stdout)),
        Err(err) => (TlsOutcome::Unavailable, Some(err.to_string())),
    };
    TlsCheck {
        bundle_installed: ca_bundle::installed().is_some(),
        outcome,
        detail,
    }
}

/// [`check_tls`] against the active PHP — the one serving every project that
/// doesn't pin its own version.
pub fn check_active_tls() -> Result<TlsCheck, AppError> {
    Ok(check_tls(&php::active_exe()?))
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

/// `php_exe` set up exactly the way a served request's PHP is — the
/// generated ini and the same environment — ready for its arguments.
fn serving_php(php_exe: &Path) -> Result<Command, AppError> {
    let ini_path = php_ini::ensure_php_ini(php_exe)?;
    let mut cmd = Command::new(php_exe);
    php_ini::apply_process_env(&mut cmd, php_exe)?;
    cmd.arg("-c").arg(&ini_path).hidden();
    Ok(cmd)
}

/// Runs `php -m` under exactly the configuration a served request gets.
fn loaded_modules(php_exe: &Path) -> Result<BTreeSet<String>, AppError> {
    let output = serving_php(php_exe)?
        .arg("-m")
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
    let mail = std::fs::read_to_string(project_dir.join(".env"))
        .ok()
        .and_then(|env| mail_setup(&env));

    let Ok(composer_json) = std::fs::read_to_string(project_dir.join("composer.json")) else {
        return Ok(ProjectDiagnosis {
            php_version,
            has_composer_json: false,
            extensions: Vec::new(),
            missing: Vec::new(),
            mail,
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
            mail,
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
        mail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_values_are_read_the_way_phpdotenv_reads_them() {
        let env = "# comment\nMAIL_HOST=first\nMAIL_HOST=second\nexport MAIL_PORT=\"1025\"\nMAIL_FROM_NAME='${APP_NAME}'\nMAIL_USERNAME=null # unused\nEMPTY=\n";
        assert_eq!(env_value(env, "MAIL_HOST").as_deref(), Some("first"));
        assert_eq!(env_value(env, "MAIL_PORT").as_deref(), Some("1025"));
        assert_eq!(
            env_value(env, "MAIL_FROM_NAME").as_deref(),
            Some("${APP_NAME}")
        );
        assert_eq!(env_value(env, "MAIL_USERNAME").as_deref(), Some("null"));
        assert_eq!(env_value(env, "EMPTY").as_deref(), Some(""));
        assert_eq!(env_value(env, "MISSING"), None);
        // A key that merely starts with the one asked for isn't it.
        assert_eq!(env_value("MAIL_HOSTNAME=x", "MAIL_HOST"), None);
    }

    #[test]
    fn smtp_to_mailpits_port_on_localhost_matches() {
        let setup = mail_setup("MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT=1025\n").unwrap();
        assert!(setup.host_reachable);
        assert!(setup.port_matches);
    }

    /// Laravel's own `.env.example` ships `MAIL_PORT=2525`.
    #[test]
    fn a_local_smtp_on_another_port_is_flagged_not_ignored() {
        let setup = mail_setup("MAIL_MAILER=smtp\nMAIL_HOST=localhost\nMAIL_PORT=2525\n").unwrap();
        assert!(setup.host_reachable);
        assert!(!setup.port_matches);
        assert_eq!(setup.port, Some(2525));
    }

    #[test]
    fn sails_mailpit_hostname_is_recognized_as_unreachable_here() {
        let setup = mail_setup("MAIL_MAILER=smtp\nMAIL_HOST=mailpit\nMAIL_PORT=1025\n").unwrap();
        assert!(!setup.host_reachable);
        assert!(setup.port_matches);
    }

    #[test]
    fn the_pre_laravel_7_driver_key_counts_too() {
        assert!(mail_setup("MAIL_DRIVER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT=1025\n").is_some());
    }

    #[test]
    fn deliberate_non_local_mail_setups_are_left_alone() {
        for env in [
            "MAIL_MAILER=log\nMAIL_HOST=127.0.0.1\n",
            "MAIL_MAILER=smtp\nMAIL_HOST=smtp.mailgun.org\nMAIL_PORT=587\n",
            "MAIL_MAILER=ses\n",
            "APP_NAME=Laravel\n",
        ] {
            assert_eq!(mail_setup(env), None, "{env}");
        }
    }

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

    #[test]
    fn a_verified_probe_carries_no_detail() {
        assert_eq!(classify_tls("0 "), (TlsOutcome::Verified, None));
    }

    /// The case this check exists for.
    #[test]
    fn error_60_reads_as_an_untrusted_certificate() {
        let (outcome, detail) =
            classify_tls("60 SSL certificate problem: unable to get local issuer certificate");
        assert_eq!(outcome, TlsOutcome::Untrusted);
        assert_eq!(
            detail.as_deref(),
            Some("SSL certificate problem: unable to get local issuer certificate")
        );
        assert_eq!(
            classify_tls("77 error setting certificate file").0,
            TlsOutcome::Untrusted
        );
    }

    /// Offline says nothing about the bundle, so it mustn't read as a fault.
    #[test]
    fn network_failures_are_unreachable_not_untrusted() {
        assert_eq!(
            classify_tls("6 Could not resolve host: repo.packagist.org").0,
            TlsOutcome::Unreachable
        );
        assert_eq!(
            classify_tls("28 Connection timed out").0,
            TlsOutcome::Unreachable
        );
    }

    #[test]
    fn a_php_without_curl_or_with_garbled_output_is_unavailable() {
        assert_eq!(classify_tls("nocurl").0, TlsOutcome::Unavailable);
        assert_eq!(
            classify_tls("PHP Warning: something").0,
            TlsOutcome::Unavailable
        );
    }

    /// Runs the real probe against the installed PHP. Run with:
    /// `cargo test --lib services::doctor::tests::print_tls_check -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_tls_check() {
        println!("{:?}", check_active_tls().unwrap());
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
