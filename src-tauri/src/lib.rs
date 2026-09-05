mod commands;
mod config;
mod db;
mod services;
mod utils;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Restores and focuses the main window — shared by the tray menu's "Show"
/// item and a left-click on the tray icon itself.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app =
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_autostart::init(
                MacosLauncher::LaunchAgent,
                None,
            ))
            .plugin(tauri_plugin_notification::init())
            .invoke_handler(tauri::generate_handler![
                commands::services::list_services,
                commands::services::start_service,
                commands::services::stop_service,
                commands::services::force_stop_service,
                commands::services::port_holder,
                commands::services::free_port,
                commands::services::restart_service,
                commands::php::list_php_versions,
                commands::php::set_active_php_version,
                commands::php::list_php_catalog,
                commands::php::install_php_version,
                commands::php::add_php_from_folder,
                commands::php::remove_php_version,
                commands::php::php_drop_in_dir,
                commands::php::open_php_drop_in_dir,
                commands::php::php_config_dir,
                commands::php::open_php_config_dir,
                commands::php::diagnose_project,
                commands::php::php_extensions,
                commands::php::install_php_extension,
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
                commands::projects::preview_project_link,
                commands::projects::link_project,
                commands::projects::unlink_project,
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
                commands::support::inspect_attachment,
                commands::support::submit_ticket,
                commands::support::fetch_ticket_history,
                commands::changelog::fetch_changelog,
                commands::changelog::last_seen_changelog_version,
                commands::changelog::mark_changelog_seen,
            ])
            .setup(|app| {
                if cfg!(debug_assertions) {
                    app.handle().plugin(
                        tauri_plugin_log::Builder::default()
                            .level(log::LevelFilter::Info)
                            .build(),
                    )?;
                }
                // First, before *anything* resolves a path: an install from
                // before the single-folder layout still has its runtimes, config
                // and projects in the three old OS locations. Reading the new
                // paths without moving those first would look exactly like data
                // loss — an empty project list, and MariaDB bootstrapping a fresh
                // datadir on top of databases that are still on disk.
                let moved = utils::migrate::run();
                if moved > 0 {
                    log::info!("moved {moved} location(s) into {:?}", utils::paths::home());
                }

                // The PATH side of the same migration, run once and then marked.
                // These read the registry through PowerShell, so leaving them on
                // every launch would add real startup latency for a repair that
                // only ever applies to an install predating the layout change.
                if moved > 0 || utils::migrate::needs_startup_repairs() {
                    // Order matters: the second check can't see the problem the
                    // first one fixes. PATH may still name the pre-move link
                    // directory, and `status()` looks for the *current* one — so it
                    // would report the feature as simply off, leaving a dead entry
                    // sitting in front of every other PHP on the machine.
                    let path_ok = match services::php_path::repair_legacy_entry() {
                        Ok(true) => {
                            log::info!("PATH now points at the current PHP link");
                            true
                        }
                        Ok(false) => true,
                        Err(err) => {
                            log::warn!("could not repair the PHP entry in PATH: {err}");
                            false
                        }
                    };

                    // A junction stores an absolute target, so moving `bin` leaves
                    // it aimed at a directory that no longer exists. Until it's
                    // rebuilt, `php` resolves to nothing in every terminal, and the
                    // only other thing that rebuilds it is the user switching
                    // versions — which they have no reason to do.
                    let link_ok = match services::php_path::status() {
                        Ok(status) if status.on_path && !status.in_sync => {
                            log::info!(
                                "the PHP link is stale; re-pointing it at the active version"
                            );
                            match services::php_path::sync() {
                                Ok(()) => true,
                                Err(err) => {
                                    log::warn!("could not re-point the PHP link: {err}");
                                    false
                                }
                            }
                        }
                        Err(err) => {
                            log::warn!("could not read the PHP link status: {err}");
                            false
                        }
                        _ => true,
                    };

                    // Only marked when both actually succeeded, so a repair that
                    // failed is retried rather than silently abandoned.
                    if path_ok && link_ok {
                        utils::migrate::mark_startup_repairs_done();
                    }
                }

                // Every installed version, not just the active one, and
                // deliberately *outside* the one-time gate above.
                //
                // Each PHP folder carries its own `php.ini` holding an absolute
                // `extension_dir`, so moving the folder leaves the path inside it
                // aimed at where it used to be — and PHP answers that by loading no
                // extensions at all. The user only discovers it when they switch to
                // that version and `php -v` prints a wall of warnings.
                //
                // Ungated because this reads a handful of small files and shells
                // out to nothing, and because an install that already ran the gated
                // repairs on an earlier build would otherwise never be fixed.
                {
                    let mut repaired = 0;
                    for runtime in services::php::installed() {
                        match services::php_ini::repair_extension_dir(&runtime.dir) {
                            Ok(true) => repaired += 1,
                            Ok(false) => {}
                            Err(err) => log::warn!(
                                "could not repair php.ini in {}: {err}",
                                runtime.dir.display()
                            ),
                        }
                    }
                    if repaired > 0 {
                        log::info!("repaired extension_dir in {repaired} php.ini file(s)");
                    }
                }

                // Before anything else reads installed-binary state: copies
                // Nginx/PHP in from the installer's bundled resources if this is
                // a fresh install that hasn't downloaded them itself yet — see
                // `services::binaries`'s module doc. A local file copy, fast
                // enough to do synchronously here.
                services::binaries::seed_bundled(app.handle());

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
                // Restoring the choice is only half of it: `services::php`'s
                // `set_active` moves in-memory state, while the junction on the
                // user's PATH lives on disk and can outlast the session that
                // pointed it (a switch interrupted by a crash, a version added
                // by unzipping it into the drop-in folder). Re-pointing here
                // also gives the restored version the `php.ini` a terminal's
                // `php` needs — see `services::php_ini`. Best-effort, and
                // skipped entirely when the user never opted into the switch.
                if services::php_path::status().is_ok_and(|s| s.on_path) {
                    if let Err(err) = services::php_path::sync() {
                        log::warn!("failed to re-point the PHP PATH link on startup: {err}");
                    }
                }
                // Reconciles the persisted flag against the OS's actual autostart
                // registration — the two can drift if the user removes the
                // startup entry from Windows Settings directly. Best-effort,
                // same reasoning as the PHP-path re-point above: this must never
                // block startup.
                let autolaunch = app.autolaunch();
                match autolaunch.is_enabled() {
                    Ok(enabled) if enabled != settings.start_with_windows => {
                        let result = if settings.start_with_windows {
                            autolaunch.enable()
                        } else {
                            autolaunch.disable()
                        };
                        if let Err(err) = result {
                            log::warn!("failed to reconcile autostart on startup: {err}");
                        }
                    }
                    Ok(_) => {}
                    Err(err) => log::warn!("failed to read the current autostart state: {err}"),
                }

                // Opt-in, at most once per session — see `Settings::auto_write_hosts`'s
                // doc comment and `services::hosts`'s module doc for why this is
                // the one place sync ever runs without an explicit user action.
                if settings.auto_write_hosts {
                    tauri::async_runtime::spawn(async {
                        match tokio::task::spawn_blocking(services::hosts::sync_hosts_entries).await
                        {
                            Ok(Ok(_)) => {}
                            Ok(Err(err)) => log::warn!("startup hosts sync failed: {err}"),
                            Err(err) => log::warn!("startup hosts sync task panicked: {err}"),
                        }
                    });
                }

                app.manage(config::settings::SettingsState::new(settings));
                app.manage(config::device::DeviceIdState(config::device::load()));
                app.manage(services::telemetry::SessionIdState(
                    uuid::Uuid::new_v4().to_string(),
                ));

                let show_item = MenuItem::with_id(app, "show", "Show Rezure", true, None::<&str>)?;
                let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

                TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show_main_window(app),
                        "quit" => {
                            if let Some(manager) = app.try_state::<services::ServiceManager>() {
                                manager.stop_all();
                            }
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
                        }
                    })
                    .build(app)?;

                // Only intercepts the close when `keep_in_tray_on_close` is on —
                // otherwise the request falls through unchanged to the existing
                // `RunEvent::ExitRequested` handler below, which still runs
                // `stop_all()` on a normal quit.
                if let Some(window) = app.get_webview_window("main") {
                    let window_handle = window.clone();
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            let keep_in_tray = window_handle
                                .app_handle()
                                .try_state::<config::settings::SettingsState>()
                                .is_some_and(|s| s.0.lock().unwrap().keep_in_tray_on_close);
                            if keep_in_tray {
                                api.prevent_close();
                                let _ = window_handle.hide();
                            }
                        }
                    });
                }

                match db::init() {
                    Ok(conn) => {
                        let share_usage_data = app
                            .state::<config::settings::SettingsState>()
                            .0
                            .lock()
                            .unwrap()
                            .share_usage_data;
                        let device_id = app.state::<config::device::DeviceIdState>().0.clone();
                        let app_version = app.package_info().version.to_string();
                        if let Err(err) = services::telemetry::TelemetryClient::record_event(
                            &conn,
                            share_usage_data,
                            &device_id,
                            "app_opened",
                            None,
                            &app_version,
                        ) {
                            log::warn!("could not record app_opened event: {err}");
                        }

                        app.manage(db::DbState::new(conn));

                        // Heartbeat recorder — queues a "still open" ping every 5
                        // minutes (and once immediately, since `interval`'s first
                        // tick fires right away). Only ever writes to the local
                        // queue; `send_pending` is what actually talks to the
                        // network.
                        let heartbeat_handle = app.handle().clone();
                        tauri::async_runtime::spawn(async move {
                            let mut interval =
                                tokio::time::interval(std::time::Duration::from_secs(5 * 60));
                            loop {
                                interval.tick().await;
                                let settings =
                                    heartbeat_handle.state::<config::settings::SettingsState>();
                                let share = settings.0.lock().unwrap().share_usage_data;
                                if !share {
                                    continue;
                                }
                                let device =
                                    heartbeat_handle.state::<config::device::DeviceIdState>();
                                let session =
                                    heartbeat_handle.state::<services::telemetry::SessionIdState>();
                                let app_version =
                                    heartbeat_handle.package_info().version.to_string();
                                let os = sysinfo::System::long_os_version();
                                let os_version = sysinfo::System::os_version();
                                let db = heartbeat_handle.state::<db::DbState>();
                                let conn = db.0.lock().unwrap();
                                if let Err(err) =
                                    services::telemetry::TelemetryClient::record_heartbeat(
                                        &conn,
                                        share,
                                        &device.0,
                                        &session.0,
                                        &app_version,
                                        os.as_deref(),
                                        os_version.as_deref(),
                                        None,
                                    )
                                {
                                    log::warn!("could not record heartbeat: {err}");
                                }
                            }
                        });

                        // Sender — drains `pending_events` every minute. Never
                        // blocks the UI: every failure inside is logged and
                        // retried on the next tick, see `services::telemetry`.
                        let sender_handle = app.handle().clone();
                        tauri::async_runtime::spawn(async move {
                            let mut interval =
                                tokio::time::interval(std::time::Duration::from_secs(60));
                            loop {
                                interval.tick().await;
                                services::telemetry::send_pending(&sender_handle).await;
                            }
                        });
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
            // Best-effort: queues one last heartbeat with `ended_at` set so
            // the backend can close this session cleanly. Only *queues* it —
            // `pending_events` is a persisted SQLite table, so if the process
            // exits before the sender loop's next tick, this still goes out
            // on the *next* launch rather than being lost.
            let share = app_handle
                .try_state::<config::settings::SettingsState>()
                .is_some_and(|s| s.0.lock().unwrap().share_usage_data);
            if share {
                if let (Some(db), Some(device), Some(session)) = (
                    app_handle.try_state::<db::DbState>(),
                    app_handle.try_state::<config::device::DeviceIdState>(),
                    app_handle.try_state::<services::telemetry::SessionIdState>(),
                ) {
                    let app_version = app_handle.package_info().version.to_string();
                    let ended_at = chrono::Utc::now().to_rfc3339();
                    let conn = db.0.lock().unwrap();
                    if let Err(err) = services::telemetry::TelemetryClient::record_heartbeat(
                        &conn,
                        share,
                        &device.0,
                        &session.0,
                        &app_version,
                        None,
                        None,
                        Some(&ended_at),
                    ) {
                        log::warn!("could not record the closing heartbeat: {err}");
                    }
                }
            }

            if let Some(manager) = app_handle.try_state::<services::ServiceManager>() {
                manager.stop_all();
            }
        }
    });
}
