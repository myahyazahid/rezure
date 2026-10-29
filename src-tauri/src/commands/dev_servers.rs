//! The Dev servers dialog: each `proxy` project's dev server, its settings,
//! and starting, stopping and installing it. See `services::dev_servers`.
//!
//! No telemetry here, on purpose: a dev server's id and name are the
//! project's, which isn't Rezure's to send anywhere.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, State};

use crate::commands::projects::sync_vhosts_and_reload;
use crate::db::DbState;
use crate::services::dev_servers::{self, DevServerChange, DevServerRow, DevSpec};
use crate::services::process::{LogSink, ServiceLogLine, LOG_EVENT};
use crate::services::{projects, ServiceHandle, ServiceInfo, ServiceManager, ServiceStatus};
use crate::utils::error::AppError;

/// How long a start already under way elsewhere (the dialog's, while Open
/// is clicked) is waited on. Longer than the dev server's own limit, which
/// ends it first.
const JOIN_START_TIMEOUT: Duration = Duration::from_secs(320);

/// Starts `service` and waits until it answers — or until a start that was
/// already under way does, since `start` returns at once in that case.
pub(crate) async fn start_and_wait(service: ServiceHandle) -> Result<ServiceInfo, AppError> {
    tokio::task::spawn_blocking(move || {
        let mut info = service.start()?;
        let deadline = Instant::now() + JOIN_START_TIMEOUT;
        while info.status == ServiceStatus::Starting && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            info = service.info();
        }
        match info.status {
            ServiceStatus::Running => Ok(info),
            _ => Err(AppError::DevServerFailed {
                name: info.name,
                reason: "it stopped while starting - see its log in Dev servers".to_string(),
            }),
        }
    })
    .await
    .map_err(|e| AppError::Io(format!("background task panicked: {e}")))?
}

/// Runs a blocking action off the async runtime.
async fn blocking<T: Send + 'static>(
    action: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(action)
        .await
        .map_err(|e| AppError::Io(format!("background task panicked: {e}")))?
}

fn rows(manager: &ServiceManager, db_state: &DbState) -> Vec<DevServerRow> {
    let specs = sync_vhosts_and_reload(manager, db_state, "while listing dev servers");
    dev_servers::rows(&specs, manager)
}

/// The project's dev server, registered and current — or why it has none.
fn dev_service(
    id: &str,
    manager: &ServiceManager,
    db_state: &DbState,
) -> Result<ServiceHandle, AppError> {
    let specs = sync_vhosts_and_reload(manager, db_state, "before using a dev server");
    if !specs.iter().any(|spec| spec.project_id == id) {
        let project = projects::find(id)?;
        return Err(AppError::NotADevServerProject(project.name));
    }
    manager.find(&dev_servers::service_id(id))
}

/// Every `proxy` project's dev server, for the dialog.
#[tauri::command]
pub fn list_dev_servers(
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Vec<DevServerRow> {
    rows(&manager, &db_state)
}

/// Each dev server's status by project id — cheap enough to poll, unlike
/// [`list_dev_servers`], which rescans the projects.
#[tauri::command]
pub fn dev_server_statuses(manager: State<'_, ServiceManager>) -> HashMap<String, ServiceStatus> {
    manager
        .handles()
        .into_iter()
        .filter_map(|service| {
            let project = dev_servers::project_id_of(service.id())?.to_string();
            Some((project, service.info().status))
        })
        .collect()
}

/// Saves a project's dev server settings, live at once: the vhost moves to
/// a new port, and a stopped dev server is rebuilt with them.
#[tauri::command]
pub fn set_dev_server_config(
    id: String,
    change: DevServerChange,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<Vec<DevServerRow>, AppError> {
    let all = projects::scan_projects()?;
    let project = all
        .iter()
        .find(|project| project.id == id)
        .ok_or_else(|| AppError::ProjectNotFound(id.clone()))?;
    let running = manager
        .find(&dev_servers::service_id(&id))
        .is_ok_and(|service| service.info().status != ServiceStatus::Stopped);
    dev_servers::set_config(project, &all, change, running)?;
    Ok(rows(&manager, &db_state))
}

#[tauri::command]
pub async fn start_dev_server(
    id: String,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<Vec<DevServerRow>, AppError> {
    let service = dev_service(&id, &manager, &db_state)?;
    start_and_wait(service).await?;
    Ok(rows(&manager, &db_state))
}

#[tauri::command]
pub async fn stop_dev_server(
    id: String,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<Vec<DevServerRow>, AppError> {
    let service = manager.find(&dev_servers::service_id(&id))?;
    blocking(move || service.stop()).await?;
    Ok(rows(&manager, &db_state))
}

/// Stops every dev server that's running.
#[tauri::command]
pub async fn stop_all_dev_servers(
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<Vec<DevServerRow>, AppError> {
    let running: Vec<ServiceHandle> = manager
        .handles()
        .into_iter()
        .filter(|service| dev_servers::is_dev_service(service.id()))
        .collect();
    blocking(move || {
        for service in running {
            if service.info().status != ServiceStatus::Stopped {
                if let Err(err) = service.stop() {
                    log::warn!("failed to stop {}: {err}", service.id());
                }
            }
        }
        Ok(())
    })
    .await?;
    Ok(rows(&manager, &db_state))
}

/// Runs the project's package manager `install`, its output going to the
/// dev server's log.
#[tauri::command]
pub async fn install_dev_dependencies(
    id: String,
    app: AppHandle,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<Vec<DevServerRow>, AppError> {
    let spec = dev_spec(&id, &manager, &db_state)?;
    let sink: LogSink = Arc::new(move |service_id, stream, line| {
        let payload = ServiceLogLine {
            service_id: service_id.to_string(),
            stream,
            line: line.to_string(),
        };
        if let Err(err) = app.emit(LOG_EVENT, payload) {
            log::warn!("failed to emit an install log line: {err}");
        }
    });
    blocking(move || dev_servers::install(&spec, &sink)).await?;
    Ok(rows(&manager, &db_state))
}

/// Stops the dev server running for this project outside Rezure (one
/// started from a terminal), so Rezure's own can start in its place.
#[tauri::command]
pub async fn stop_outside_dev_server(
    id: String,
    manager: State<'_, ServiceManager>,
    db_state: State<'_, DbState>,
) -> Result<Vec<DevServerRow>, AppError> {
    let spec = dev_spec(&id, &manager, &db_state)?;
    blocking(move || dev_servers::stop_outside(&spec)).await?;
    Ok(rows(&manager, &db_state))
}

/// The project's dev server spec as it is now — or why it has none.
fn dev_spec(id: &str, manager: &ServiceManager, db_state: &DbState) -> Result<DevSpec, AppError> {
    let specs = sync_vhosts_and_reload(manager, db_state, "before using a dev server");
    match specs.into_iter().find(|spec| spec.project_id == id) {
        Some(spec) => Ok(spec),
        None => {
            let project = projects::find(id)?;
            Err(AppError::NotADevServerProject(project.name))
        }
    }
}
