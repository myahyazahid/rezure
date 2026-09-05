//! Tauri command handlers exposed to the frontend via `invoke()`.
//! Kept thin — delegates to `services/` for actual logic.

pub mod binaries;
pub mod changelog;
pub mod database;
pub mod db_profiles;
pub mod php;
pub mod projects;
pub mod services;
pub mod settings;
pub mod support;
