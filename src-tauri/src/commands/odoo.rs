//! Thin glue between the Databases page's Odoo panel and `services::odoo`.
//!
//! Connecting and restoring stop and start services and wait on a UAC
//! prompt, so both run off the async runtime.

use tauri::{AppHandle, Manager};

use crate::services::odoo::{self, ConnectOptions, OdooInstall};
use crate::services::windows_service_control::Action;
use crate::services::ServiceManager;
use crate::utils::error::AppError;

fn joined(e: tokio::task::JoinError) -> AppError {
    AppError::Io(format!("background task panicked: {e}"))
}

/// Every Odoo installed on this PC, and how its database settings stand.
#[tauri::command]
pub async fn detect_odoo() -> Result<Vec<OdooInstall>, AppError> {
    tokio::task::spawn_blocking(odoo::detect)
        .await
        .map_err(joined)
}

#[tauri::command]
pub async fn connect_odoo(
    dir: String,
    options: ConnectOptions,
    app: AppHandle,
) -> Result<Vec<OdooInstall>, AppError> {
    tokio::task::spawn_blocking(move || {
        odoo::connect(&app.state::<ServiceManager>(), &dir, &options)
    })
    .await
    .map_err(joined)?
}

#[tauri::command]
pub async fn restore_odoo(dir: String, app: AppHandle) -> Result<Vec<OdooInstall>, AppError> {
    tokio::task::spawn_blocking(move || odoo::restore(&app.state::<ServiceManager>(), &dir))
        .await
        .map_err(joined)?
}

#[tauri::command]
pub fn open_odoo(dir: String) -> Result<(), AppError> {
    odoo::open(&dir)
}

/// Opens the install's odoo.conf in Notepad run as administrator.
#[tauri::command]
pub async fn open_odoo_conf(dir: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || odoo::open_conf(&dir))
        .await
        .map_err(joined)?
}

/// Starts, stops or restarts Odoo's Windows service.
#[tauri::command]
pub async fn control_odoo(
    dir: String,
    action: Action,
    app: AppHandle,
) -> Result<Vec<OdooInstall>, AppError> {
    tokio::task::spawn_blocking(move || odoo::control(&app.state::<ServiceManager>(), &dir, action))
        .await
        .map_err(joined)?
}

/// Lets this account start and stop Odoo's service without a prompt, after
/// one UAC prompt now.
#[tauri::command]
pub async fn allow_odoo_control(dir: String) -> Result<Vec<OdooInstall>, AppError> {
    tokio::task::spawn_blocking(move || odoo::allow_control(&dir))
        .await
        .map_err(joined)?
}

/// Recreates Odoo's login on Rezure's PostgreSQL from odoo.conf.
#[tauri::command]
pub async fn repair_odoo_login(dir: String, app: AppHandle) -> Result<Vec<OdooInstall>, AppError> {
    tokio::task::spawn_blocking(move || odoo::repair_login(&app.state::<ServiceManager>(), &dir))
        .await
        .map_err(joined)?
}
