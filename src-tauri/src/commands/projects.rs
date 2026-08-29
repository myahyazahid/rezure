use tauri::State;

use crate::db::projects::{ProjectInfo, ProjectStore};

#[tauri::command]
pub fn list_projects(store: State<'_, ProjectStore>) -> Vec<ProjectInfo> {
    store.list()
}
