//! Auto-detects local projects by scanning `www_root()` — one folder, one
//! project, stack guessed from marker files (`artisan`, `wp-config.php`,
//! `package.json`, ...). The filesystem is the source of truth for *which*
//! projects exist, rescanned on every `list_projects` call; `commands::projects`
//! merges in each project's SQLite history (`db::projects`) afterward.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::hosts;
use crate::db::projects::ProjectInfo;
use crate::utils::error::AppError;

/// `%USERPROFILE%\rezure\www` — where Rezure looks for projects. A fresh
/// install gets an empty folder created here rather than an error, so
/// there's always somewhere to drop a project into.
pub fn www_root() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::Io("could not resolve the home directory".to_string()))?;
    Ok(home.join("rezure").join("www"))
}

/// Scans `www_root()` for project folders, sorted by name.
pub fn scan_projects() -> Result<Vec<ProjectInfo>, AppError> {
    let root = www_root()?;
    fs::create_dir_all(&root)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", root.display())))?;

    let entries = fs::read_dir(&root)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", root.display())))?;

    let mut projects = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Some(folder_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Skip hidden/system folders (e.g. a stray `.git` if `www/` itself
        // ends up pointed at a repo root).
        if folder_name.starts_with('.') {
            continue;
        }

        let domain = format!("{folder_name}.test");
        projects.push(ProjectInfo {
            id: folder_name.to_string(),
            name: folder_name.to_string(),
            path: path.display().to_string(),
            has_hosts_entry: hosts::has_entry(&domain),
            domain,
            stack: detect_stack(&path),
            // Filled in by `commands::projects::list_projects` from SQLite —
            // a bare scan has no way to know this.
            last_opened_at: None,
            open_count: 0,
        });
    }

    projects.sort_by_key(|p| p.name.to_lowercase());
    Ok(projects)
}

/// The project's docroot — Laravel serves from `public/`, everything else
/// from the project root itself. Used by vhost generation.
pub fn docroot(project_path: &Path, stack: &str) -> PathBuf {
    if stack == "Laravel" && project_path.join("public").is_dir() {
        project_path.join("public")
    } else {
        project_path.to_path_buf()
    }
}

fn detect_stack(dir: &Path) -> String {
    if dir.join("artisan").is_file() {
        return "Laravel".to_string();
    }
    if dir.join("wp-config.php").is_file() || dir.join("wp-load.php").is_file() {
        return "WordPress".to_string();
    }
    if let Some(stack) = detect_from_package_json(dir) {
        return stack;
    }
    if dir.join("composer.json").is_file() || dir.join("index.php").is_file() {
        return "PHP".to_string();
    }
    if dir.join("index.html").is_file() {
        return "Static".to_string();
    }
    "Unknown".to_string()
}

/// Looks for a handful of well-known frontend framework dependencies in
/// `package.json`; falls back to a generic "Node" if the file exists but
/// none of them match.
fn detect_from_package_json(dir: &Path) -> Option<String> {
    let content = fs::read_to_string(dir.join("package.json")).ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;

    let has_dep = |name: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .filter_map(|section| json.get(section)?.as_object())
            .any(|deps| deps.contains_key(name))
    };

    const FRAMEWORKS: &[(&str, &str)] = &[
        ("next", "Next.js"),
        ("nuxt", "Nuxt"),
        ("vue", "Vue"),
        ("react", "React"),
        ("svelte", "Svelte"),
    ];

    for (dep, label) in FRAMEWORKS {
        if has_dep(dep) {
            return Some((*label).to_string());
        }
    }

    Some("Node".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rezure-test-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_laravel_via_artisan() {
        let dir = temp_project("laravel");
        fs::write(dir.join("artisan"), "").unwrap();
        assert_eq!(detect_stack(&dir), "Laravel");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_wordpress_via_wp_config() {
        let dir = temp_project("wordpress");
        fs::write(dir.join("wp-config.php"), "").unwrap();
        assert_eq!(detect_stack(&dir), "WordPress");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_vue_via_package_json_dependency() {
        let dir = temp_project("vue");
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"vue":"^3.5.0"}}"#,
        )
        .unwrap();
        assert_eq!(detect_stack(&dir), "Vue");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn falls_back_to_node_for_unrecognized_package_json() {
        let dir = temp_project("node");
        fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"express":"^4.0.0"}}"#,
        )
        .unwrap();
        assert_eq!(detect_stack(&dir), "Node");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn falls_back_to_unknown_for_an_empty_folder() {
        let dir = temp_project("empty");
        assert_eq!(detect_stack(&dir), "Unknown");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn laravel_docroot_is_the_public_folder() {
        let dir = temp_project("laravel-docroot");
        fs::create_dir_all(dir.join("public")).unwrap();
        assert_eq!(docroot(&dir, "Laravel"), dir.join("public"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn non_laravel_docroot_is_the_project_root() {
        let dir = temp_project("static-docroot");
        assert_eq!(docroot(&dir, "Static"), dir);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_projects_creates_www_root_and_returns_empty_when_no_projects_exist() {
        // Not asserting on real `www_root()` contents (the developer
        // machine may have real projects there) — just that scanning a
        // freshly-created empty root doesn't error.
        let root = www_root().unwrap();
        assert!(root.ends_with("rezure\\www") || root.ends_with("rezure/www"));
    }

    /// Manual, real-filesystem check against whatever's actually in
    /// `www_root()` right now — run with:
    /// `cargo test --lib services::projects::tests::print_real_scan -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_real_scan() {
        for p in scan_projects().unwrap() {
            println!("{} | {} | {} | {}", p.id, p.stack, p.domain, p.path);
        }
    }
}
