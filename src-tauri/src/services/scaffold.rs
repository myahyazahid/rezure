//! Creates a new project from a starting-point template.
//!
//! Only templates Rezure can actually build with what it already bundles
//! are offered: Laravel (via a downloaded `composer.phar`, run through the
//! bundled PHP — no separate Composer binary needed), WordPress (the
//! official core zip, no CLI required), and two zero-dependency
//! skeletons. A Node/npm-based template (Vue, Next, ...) isn't offered —
//! Rezure doesn't bundle Node.js yet (see `services::binaries::MANIFEST`),
//! so there's nothing to run `npm create` with.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use super::php_ini;
use super::projects::www_root;
use crate::utils::command::HiddenWindow;
use crate::utils::error::AppError;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Short tag shown next to the template in the UI — names what
    /// actually builds it, not a vague category.
    pub tag: &'static str,
}

pub const TEMPLATES: &[ProjectTemplate] = &[
    ProjectTemplate {
        id: "laravel",
        name: "Laravel 11",
        description: "PHP app skeleton, artisan ready",
        tag: "composer",
    },
    ProjectTemplate {
        id: "wordpress",
        name: "WordPress",
        description: "Latest core, ready for a database",
        tag: "zip",
    },
    ProjectTemplate {
        id: "blank-php",
        name: "Blank PHP",
        description: "A single index.php to start from",
        tag: "php",
    },
    ProjectTemplate {
        id: "static-html",
        name: "Static HTML",
        description: "Just an index.html, no build step",
        tag: "html",
    },
];

pub fn find_template(id: &str) -> Result<&'static ProjectTemplate, AppError> {
    TEMPLATES
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::UnknownTemplate(id.to_string()))
}

/// Folder- and domain-safe project names only: lowercase letters, digits,
/// and hyphens, 1-64 characters, can't start or end with a hyphen. Checked
/// before the name is ever concatenated into a filesystem path or a
/// generated `.test` domain — rejects path traversal and invalid Windows
/// filenames by construction rather than trying to escape them.
pub fn validate_name(name: &str) -> Result<(), AppError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');

    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidProjectName(name.to_string()))
    }
}

/// Creates `name` under `www_root()` from `template_id`. The caller is
/// expected to re-run `projects::scan_projects` afterward to pick up the
/// new folder — this doesn't construct a `ProjectInfo` itself, since the
/// filesystem scan is the single source of truth for what's detected.
pub async fn create_project(name: &str, template_id: &str) -> Result<(), AppError> {
    validate_name(name)?;
    let template = find_template(template_id)?;

    let target = www_root()?.join(name);
    if target.exists() {
        return Err(AppError::ProjectAlreadyExists(name.to_string()));
    }

    match template.id {
        "laravel" => scaffold_laravel(&target).await,
        "wordpress" => scaffold_wordpress(&target).await,
        "blank-php" => scaffold_blank_php(&target),
        "static-html" => scaffold_static_html(&target),
        _ => unreachable!("find_template only ever returns a known id"),
    }
}

async fn download_bytes(url: &str) -> Result<Vec<u8>, AppError> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| AppError::Download(format!("{url}: {e}")))?;

    if !response.status().is_success() {
        return Err(AppError::Download(format!(
            "{url} responded with {}",
            response.status()
        )));
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| AppError::Download(format!("{url}: {e}")))
}

/// Extracts a zip archive into `dest_dir`, rejecting any entry whose path
/// would escape it (zip-slip). If every entry shares a common single
/// top-level folder (e.g. WordPress's `wordpress/` wrapper), that prefix
/// is stripped so `dest_dir` ends up with the project's files directly in
/// it, not nested one level deeper.
fn extract_zip_flattened(bytes: &[u8], dest_dir: &Path) -> Result<(), AppError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| AppError::Extract(e.to_string()))?;

    let prefix = common_top_level_dir(&mut archive)?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| AppError::Extract(e.to_string()))?;

        let relative_path = match entry.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => {
                return Err(AppError::Extract(format!(
                    "archive entry {} has an unsafe path",
                    entry.name()
                )));
            }
        };

        let relative_path = match &prefix {
            Some(prefix) => match relative_path.strip_prefix(prefix) {
                Ok(stripped) if stripped.as_os_str().is_empty() => continue, // the prefix dir entry itself
                Ok(stripped) => stripped.to_path_buf(),
                Err(_) => relative_path,
            },
            None => relative_path,
        };

        let out_path = dest_dir.join(relative_path);

        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| AppError::Io(e.to_string()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
        }

        let mut out_file = fs::File::create(&out_path).map_err(|e| AppError::Io(e.to_string()))?;
        std::io::copy(&mut entry, &mut out_file).map_err(|e| AppError::Io(e.to_string()))?;
    }

    Ok(())
}

/// The single top-level directory every entry in `archive` lives under, if
/// there is one (`Some("wordpress")`), or `None` if entries sit at
/// multiple top-level paths already.
fn common_top_level_dir(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<Option<PathBuf>, AppError> {
    let mut common: Option<PathBuf> = None;

    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| AppError::Extract(e.to_string()))?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Some(top) = path.components().next() else {
            continue;
        };
        let top = PathBuf::from(top.as_os_str());

        match &common {
            None => common = Some(top),
            Some(existing) if *existing == top => {}
            Some(_) => return Ok(None),
        }
    }

    Ok(common)
}

fn composer_phar_path() -> Result<PathBuf, AppError> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        AppError::Io("could not resolve the local app data directory".to_string())
    })?;
    Ok(base
        .join("Rezure")
        .join("bin")
        .join("composer")
        .join("composer.phar"))
}

/// Whether `composer.phar` has been downloaded — there's no fixed version
/// to switch between (`ensure_composer` always fetches whatever's current
/// at Composer's stable download URL), just "have we cached it yet".
pub fn composer_installed() -> bool {
    composer_phar_path().map(|p| p.is_file()).unwrap_or(false)
}

/// Downloads `composer.phar` if it isn't cached yet — the explicit,
/// user-triggered counterpart to `ensure_composer`'s lazy on-demand fetch
/// during a Laravel scaffold.
pub async fn install_composer() -> Result<(), AppError> {
    ensure_composer().await.map(|_| ())
}

/// Composer self-updates constantly, so unlike the pinned-version binaries
/// in `services::binaries`, this always fetches whatever's current at the
/// official, HTTPS-only download URL and caches it — there's no stable
/// per-version checksum to pin against.
async fn ensure_composer() -> Result<PathBuf, AppError> {
    let path = composer_phar_path()?;
    if path.is_file() {
        return Ok(path);
    }

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    }

    let bytes = download_bytes("https://getcomposer.org/composer.phar").await?;
    fs::write(&path, bytes)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))?;
    Ok(path)
}

async fn scaffold_laravel(target: &Path) -> Result<(), AppError> {
    // Whichever PHP version is currently active in the Switch UI — same
    // resolution `services::process` uses for the FastCGI service.
    let php_exe = super::php::active_exe()?;
    if !php_exe.is_file() {
        return Err(AppError::BinaryNotInstalled("PHP".to_string()));
    }
    let ini_path = php_ini::ensure_php_ini(&php_exe)?;
    let composer_phar = ensure_composer().await?;

    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let output = Command::new(&php_exe)
            .arg("-c")
            .arg(&ini_path)
            .arg(&composer_phar)
            .arg("create-project")
            .arg("laravel/laravel")
            .arg(&target)
            .arg("--prefer-dist")
            .arg("--no-interaction")
            .stdin(Stdio::null())
            .hidden()
            .output()
            .map_err(|e| AppError::ScaffoldFailed(format!("could not run composer: {e}")))?;

        if !output.status.success() {
            let _ = fs::remove_dir_all(&target);
            return Err(AppError::ScaffoldFailed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::ScaffoldFailed(format!("background task panicked: {e}")))?
}

async fn scaffold_wordpress(target: &Path) -> Result<(), AppError> {
    let bytes = download_bytes("https://wordpress.org/latest.zip").await?;

    fs::create_dir_all(target)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", target.display())))?;

    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || extract_zip_flattened(&bytes, &target))
        .await
        .map_err(|e| AppError::Extract(format!("background task panicked: {e}")))?
}

fn scaffold_blank_php(target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", target.display())))?;
    fs::write(
        target.join("index.php"),
        "<?php\n\necho 'Hello from Rezure!';\n",
    )
    .map_err(|e| AppError::Io(e.to_string()))
}

fn scaffold_static_html(target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", target.display())))?;
    fs::write(
        target.join("index.html"),
        "<!doctype html>\n<html>\n<head><title>New project</title></head>\n<body><h1>Hello from Rezure!</h1></body>\n</html>\n",
    )
    .map_err(|e| AppError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names_are_accepted() {
        assert!(validate_name("blog").is_ok());
        assert!(validate_name("my-shop-2").is_ok());
        assert!(validate_name("a").is_ok());
    }

    #[test]
    fn invalid_names_are_rejected() {
        assert!(validate_name("").is_err());
        assert!(validate_name("-leading-hyphen").is_err());
        assert!(validate_name("trailing-hyphen-").is_err());
        assert!(validate_name("Has Spaces").is_err());
        assert!(validate_name("../traversal").is_err());
        assert!(validate_name("Upper_Case").is_err());
        assert!(validate_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn find_template_matches_known_ids() {
        assert_eq!(find_template("laravel").unwrap().id, "laravel");
        assert!(find_template("nonexistent").is_err());
    }

    #[test]
    fn every_template_has_stable_metadata() {
        for template in TEMPLATES {
            assert!(!template.name.is_empty());
            assert!(!template.description.is_empty());
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rezure-test-scaffold-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn blank_php_writes_a_working_index() {
        let dir = temp_dir("blank-php");
        scaffold_blank_php(&dir).unwrap();
        assert!(dir.join("index.php").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn static_html_writes_an_index() {
        let dir = temp_dir("static-html");
        scaffold_static_html(&dir).unwrap();
        assert!(dir.join("index.html").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Builds a tiny in-memory zip with every entry under one shared
    /// top-level folder, matching WordPress's own `wordpress/...` layout.
    fn zip_with_common_prefix() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            writer.add_directory("app/", options).unwrap();
            writer.start_file("app/index.php", options).unwrap();
            std::io::Write::write_all(&mut writer, b"<?php echo 'hi';").unwrap();
            writer
                .start_file("app/wp-config-sample.php", options)
                .unwrap();
            std::io::Write::write_all(&mut writer, b"<?php").unwrap();
            writer.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn extract_zip_flattened_strips_the_shared_top_level_folder() {
        let dir = temp_dir("wp-extract");
        fs::create_dir_all(&dir).unwrap();

        extract_zip_flattened(&zip_with_common_prefix(), &dir).unwrap();

        assert!(
            dir.join("index.php").is_file(),
            "should be flattened, not nested under app/"
        );
        assert!(dir.join("wp-config-sample.php").is_file());
        assert!(!dir.join("app").exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    /// Downloads and extracts the real WordPress core zip through the
    /// actual `create_project` entry point. Needs network access. Run
    /// with:
    /// `cargo test --lib services::scaffold::tests::wordpress_scaffold_creates_a_real_site -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn wordpress_scaffold_creates_a_real_site() {
        let name = "rezure-test-wp-scaffold";
        let target = www_root().unwrap().join(name);
        let _ = fs::remove_dir_all(&target);

        create_project(name, "wordpress").await.unwrap();

        assert!(target.join("wp-config-sample.php").is_file());
        assert!(target.join("wp-load.php").is_file());
        assert!(target.join("wp-content").is_dir());
        assert!(
            !target.join("wordpress").exists(),
            "should be flattened into the project root"
        );

        fs::remove_dir_all(&target).unwrap();
    }

    /// Runs the real Composer `create-project` through the actual bundled
    /// PHP, through the actual `create_project` entry point. Needs
    /// nginx/php/mariadb's PHP binary installed and network access — takes
    /// several minutes (Composer resolves and downloads Laravel's full
    /// dependency tree). Run with:
    /// `cargo test --lib services::scaffold::tests::laravel_scaffold_creates_a_working_app -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn laravel_scaffold_creates_a_working_app() {
        let name = "rezure-test-laravel-scaffold";
        let target = www_root().unwrap().join(name);
        let _ = fs::remove_dir_all(&target);

        create_project(name, "laravel")
            .await
            .expect("php must be installed and network reachable to run this test");

        assert!(target.join("artisan").is_file());
        assert!(target.join("vendor/autoload.php").is_file());
        assert!(target.join(".env").is_file());

        fs::remove_dir_all(&target).unwrap();
    }

    #[test]
    fn create_project_rejects_a_name_that_already_exists() {
        let name = "rezure-test-existing-project";
        let target = www_root().unwrap().join(name);
        fs::create_dir_all(&target).unwrap();

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(create_project(name, "blank-php"));

        assert!(matches!(result, Err(AppError::ProjectAlreadyExists(_))));
        fs::remove_dir_all(&target).unwrap();
    }
}
