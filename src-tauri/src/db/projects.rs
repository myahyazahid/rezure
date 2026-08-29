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

fn project(name: &str, folder: &str) -> ProjectInfo {
    ProjectInfo {
        id: folder.to_string(),
        name: name.to_string(),
        path: format!("C:\\rezure\\www\\{folder}"),
        domain: format!("{folder}.test"),
    }
}

/// Seed data standing in for a real scan of the `www/` directory (Phase 3).
pub fn seed_projects() -> ProjectStore {
    ProjectStore {
        projects: vec![
            project("Rezure Site", "rezure-site"),
            project("Laravel Shop", "laravel-shop"),
            project("Client Blog", "client-blog"),
            project("Vue Playground", "vue-playground"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_projects_get_a_local_domain_and_path() {
        let list = seed_projects().list();

        assert_eq!(list.len(), 4);
        assert_eq!(list[0].domain, "rezure-site.test");
        assert!(list[0].path.ends_with("www\\rezure-site"));
    }
}
