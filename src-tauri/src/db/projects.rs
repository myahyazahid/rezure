//! Project records.
//!
//! In-memory stand-in for the SQLite `projects` table introduced in Phase 4;
//! the query surface is kept narrow so swapping the backing store is local.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub domain: String,
    /// Detected framework/stack (Laravel, Vue, WordPress, ...), shown as a
    /// badge in the UI.
    pub stack: String,
}

/// Tauri-managed state holding the detected projects.
pub struct ProjectStore {
    projects: Vec<ProjectInfo>,
}

impl ProjectStore {
    pub fn list(&self) -> Vec<ProjectInfo> {
        self.projects.clone()
    }
}

fn project(name: &str, folder: &str, stack: &str) -> ProjectInfo {
    ProjectInfo {
        id: folder.to_string(),
        name: name.to_string(),
        path: format!("C:\\rezure\\www\\{folder}"),
        domain: format!("{folder}.test"),
        stack: stack.to_string(),
    }
}

/// Seed data standing in for a real scan of the `www/` directory (Phase 3).
pub fn seed_projects() -> ProjectStore {
    ProjectStore {
        projects: vec![
            project("blog", "blog", "Laravel"),
            project("shop-api", "shop-api", "Laravel"),
            project("portfolio", "portfolio", "Vue"),
            project("client-cms", "client-cms", "WordPress"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_projects_get_a_local_domain_path_and_stack() {
        let list = seed_projects().list();

        assert_eq!(list.len(), 4);
        assert_eq!(list[0].domain, "blog.test");
        assert!(list[0].path.ends_with("www\\blog"));
        assert_eq!(list[0].stack, "Laravel");
    }
}
