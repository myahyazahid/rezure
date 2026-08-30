mod commands;
mod config;
mod db;
mod services;
mod utils;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::services::list_services,
            commands::services::start_service,
            commands::services::stop_service,
            commands::services::restart_service,
            commands::php::list_php_versions,
            commands::php::set_active_php_version,
            commands::projects::list_projects,
            commands::projects::sync_hosts,
            commands::projects::list_project_templates,
            commands::projects::create_project,
            commands::projects::www_root,
            commands::projects::composer_installed,
            commands::projects::install_composer,
            commands::projects::open_project_site,
            commands::projects::open_project_folder,
            commands::projects::open_project_terminal,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // Best-effort: on a normal quit (window closed, not a crash), stop
        // every running service so it can't outlive this process as an
        // orphan holding its port — see `services::ProcessService::reap_orphan`
        // for the case this can't cover (a crash / force-kill, which never
        // reaches this handler at all).
        if let tauri::RunEvent::ExitRequested { .. } = event {
            if let Some(manager) = app_handle.try_state::<services::ServiceManager>() {
                manager.stop_all();
            }
        }
    });
}
