mod commands;
mod config;
mod db;
mod services;
mod utils;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(services::php::seed_php_versions())
        .invoke_handler(tauri::generate_handler![
            commands::services::list_services,
            commands::services::start_service,
            commands::services::stop_service,
            commands::services::restart_service,
            commands::php::list_php_versions,
            commands::php::set_active_php_version,
            commands::projects::list_projects,
            commands::projects::sync_hosts,
            commands::binaries::list_binaries,
            commands::binaries::install_binary,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            // Needs a live `AppHandle` (to emit `service://log` events),
            // which only exists once the app is actually starting up.
            app.manage(services::real_services(app.handle().clone()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
