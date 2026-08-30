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
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::services::list_services,
            commands::services::start_service,
            commands::services::stop_service,
            commands::services::restart_service,
            commands::php::list_php_versions,
            commands::php::set_active_php_version,
            commands::php::list_php_catalog,
            commands::php::install_php_version,
            commands::php::add_php_from_folder,
            commands::php::remove_php_version,
            commands::php::php_drop_in_dir,
            commands::php::open_php_drop_in_dir,
            commands::php::php_path_status,
            commands::php::enable_php_path,
            commands::php::disable_php_path,
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
            commands::database::list_databases,
            commands::database::database_server_info,
            commands::database::list_collations,
            commands::database::create_database,
            commands::database::drop_database,
            commands::database::export_database,
            commands::database::import_sql,
            commands::database::open_dumps_folder,
            commands::database::list_db_clients,
            commands::database::open_in_db_client,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::storage_paths,
            commands::db_profiles::list_db_profiles,
            commands::db_profiles::active_db_profile,
            commands::db_profiles::detect_db_profiles,
            commands::db_profiles::add_db_profile,
            commands::db_profiles::remove_db_profile,
            commands::db_profiles::switch_db_profile,
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

            let settings = config::settings::load();
            // Best-effort: the version may no longer be installed, in which
            // case `services::php`'s own fallback picks the newest one —
            // nothing here needs to treat that as an error.
            if let Some(version) = &settings.active_php_version {
                let _ = services::php::set_active(version);
            }
            app.manage(config::settings::SettingsState::new(settings));

            match db::init() {
                Ok(conn) => {
                    app.manage(db::DbState::new(conn));
                }
                // Degrades rather than crashing the app: commands that need
                // `DbState` will fail with a clear "state not managed"
                // error instead of the whole app refusing to start over a
                // local SQLite file problem (disk full, permissions, ...).
                Err(err) => log::error!("failed to open the project database: {err}"),
            }

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
