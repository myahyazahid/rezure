//! Windows services that hold a port Rezure wants, and stopping one.
//!
//! A server installed as a Windows service (Odoo's or EDB's PostgreSQL, a
//! MySQL installed with Oracle's installer, IIS) runs under another account,
//! so Rezure can't read its path or kill it — and killing it would be the
//! wrong fix anyway: the Service Control Manager may restart it, and it comes
//! back with Windows regardless. What frees the port is stopping the service,
//! which needs admin rights. This module names the service (`ports::holder`
//! uses it to say *which* one) and stops it through the same one-off UAC
//! prompt the hosts file uses (`utils::elevation`), only when the user asks.

use std::fs;

use serde::Serialize;
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW, QueryServiceConfigW,
    ENUM_SERVICE_STATUS_PROCESSW, QUERY_SERVICE_CONFIGW, SC_ENUM_PROCESS_INFO, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_AUTO_START,
    SERVICE_QUERY_CONFIG, SERVICE_WIN32, SERVICE_WIN32_SHARE_PROCESS,
};

use crate::utils::elevation::{self, Elevated};
use crate::utils::error::AppError;
use crate::utils::paths;
use crate::utils::powershell::quote_ps;

/// A running Windows service, as the user would find it in `services.msc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsService {
    /// The key name (`PostgreSQL_For_Odoo`) — what `Stop-Service` takes.
    pub name: String,
    /// The name `services.msc` lists it under.
    pub display_name: String,
    /// Start type Automatic: stopping it frees the port only until the next
    /// boot, which the UI says, and offers to change.
    pub starts_with_windows: bool,
}

/// One row of the running-services list.
struct Running {
    pid: u32,
    name: String,
    display_name: String,
    /// Hosted in a shared `svchost.exe` alongside other services.
    shared: bool,
}

/// The service running as `pid`, or as its parent `parent`.
///
/// The parent counts because a service often listens through a child:
/// `pg_ctl.exe` is the service, the `postgres.exe` it starts holds 5432, and
/// Apache's service process hands the socket to a child `httpd.exe`. Only
/// one level up, though — further than that, an ordinary program started from
/// a shell inside an SSH session would be blamed on `sshd`.
///
/// A process shared by several services (`svchost.exe`) gives no answer:
/// stopping one of them is a guess that would take the others down with it.
pub fn owning(pid: u32, parent: Option<u32>) -> Option<WindowsService> {
    let running = running_services();
    [Some(pid), parent]
        .into_iter()
        .flatten()
        .find_map(|candidate| {
            let mut hosted = running.iter().filter(|s| s.pid == candidate);
            let service = hosted.next()?;
            if service.shared || hosted.next().is_some() {
                return None;
            }
            Some(WindowsService {
                name: service.name.clone(),
                display_name: service.display_name.clone(),
                starts_with_windows: starts_automatically(&service.name),
            })
        })
}

/// Every active Win32 service with the process it runs in, from the Service
/// Control Manager. Enumerating needs no admin rights. Empty if the SCM
/// can't be asked — the holder is then described as before, not wrongly.
fn running_services() -> Vec<Running> {
    // SAFETY: the SCM handle is closed on every path once opened. The buffer
    // is sized from the first call's `needed` and handed back with that
    // length, so the second call writes only inside it; the entries it
    // returns, and the strings they point to, live inside that same buffer,
    // which outlives every read below. `u64` storage keeps it aligned for
    // `ENUM_SERVICE_STATUS_PROCESSW`.
    unsafe {
        let scm = OpenSCManagerW(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        );
        if scm.is_null() {
            return Vec::new();
        }

        let mut needed = 0u32;
        let mut count = 0u32;
        let mut resume = 0u32;
        // Sizing call: fails with ERROR_MORE_DATA and reports `needed`.
        EnumServicesStatusExW(
            scm,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_ACTIVE,
            std::ptr::null_mut(),
            0,
            &mut needed,
            &mut count,
            &mut resume,
            std::ptr::null(),
        );
        // A service starting between the two calls grows the list; the slack
        // keeps that from failing the second call.
        let size = needed as usize + 4096;
        let mut buffer = vec![0u64; size.div_ceil(8)];
        resume = 0;
        let ok = EnumServicesStatusExW(
            scm,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_ACTIVE,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
            &mut count,
            &mut resume,
            std::ptr::null(),
        );
        CloseServiceHandle(scm);
        if ok == 0 {
            return Vec::new();
        }

        let entries = std::slice::from_raw_parts(
            buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
            count as usize,
        );
        entries
            .iter()
            .filter(|entry| entry.ServiceStatusProcess.dwProcessId != 0)
            .map(|entry| Running {
                pid: entry.ServiceStatusProcess.dwProcessId,
                name: wide_to_string(entry.lpServiceName),
                display_name: wide_to_string(entry.lpDisplayName),
                shared: entry.ServiceStatusProcess.dwServiceType & SERVICE_WIN32_SHARE_PROCESS != 0,
            })
            .collect()
    }
}

/// Whether `name`'s start type is Automatic. False when it can't be read,
/// which only costs the "don't start it with Windows" offer.
fn starts_automatically(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both handles are closed on every path once opened, and the
    // config buffer is sized from the first call's `needed` before the second
    // writes into it. `u64` storage keeps it aligned for
    // `QUERY_SERVICE_CONFIGW`.
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return false;
        }
        let service: SC_HANDLE = OpenServiceW(scm, wide.as_ptr(), SERVICE_QUERY_CONFIG);
        if service.is_null() {
            CloseServiceHandle(scm);
            return false;
        }

        let mut needed = 0u32;
        QueryServiceConfigW(service, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        let ok = QueryServiceConfigW(
            service,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
        );
        CloseServiceHandle(service);
        CloseServiceHandle(scm);

        ok != 0
            && (*buffer.as_ptr().cast::<QUERY_SERVICE_CONFIGW>()).dwStartType == SERVICE_AUTO_START
    }
}

/// A NUL-terminated UTF-16 string from the SCM, as a `String`.
///
/// # Safety
/// `ptr` must be null or point to a NUL-terminated UTF-16 string.
unsafe fn wide_to_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
}

/// The elevated script that stops `name`, and with `disable_autostart` sets
/// it to start Manually so it doesn't take the port back at the next boot.
///
/// No `-Force` on `Stop-Service`: that would quietly stop every service that
/// depends on this one too. Without it, a service with dependents fails with
/// a message saying so, which is what the user should see.
fn stop_script(name: &str, disable_autostart: bool, error_log: &str) -> String {
    let name = quote_ps(name);
    let manual = if disable_autostart {
        format!("    Set-Service -Name {name} -StartupType Manual -ErrorAction Stop\n")
    } else {
        String::new()
    };
    format!(
        "try {{\n    Stop-Service -Name {name} -ErrorAction Stop\n{manual}}} catch {{\n    $_.Exception.Message | Out-File -FilePath {} -Encoding utf8\n    exit 1\n}}\n",
        quote_ps(error_log),
    )
}

/// Stops `service` through a UAC prompt.
///
/// Success here means the elevated script reported no error; whether the
/// port actually came free is for the caller to check (`ports::reclaim`
/// does), the same split `hosts::elevate_copy` makes — the elevated process's
/// exit code isn't trusted (see `elevation::run_script`).
pub fn stop(service: &WindowsService, disable_autostart: bool) -> Result<(), AppError> {
    let failed = |reason: String| AppError::WindowsServiceStopFailed {
        name: service.display_name.clone(),
        reason,
    };

    let dir = paths::data()?.join("ports");
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    let script_path = dir.join("stop-service.ps1");
    let error_log_path = dir.join("stop-service.error.log");
    let _ = fs::remove_file(&error_log_path);

    let script = stop_script(
        &service.name,
        disable_autostart,
        &error_log_path.display().to_string(),
    );
    fs::write(&script_path, script)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", script_path.display())))?;

    log::info!(
        "stopping Windows service {} (disable autostart: {disable_autostart})",
        service.name
    );
    if elevation::run_script(&script_path).map_err(|e| failed(e.to_string()))? == Elevated::Declined
    {
        return Err(AppError::WindowsServiceStopCancelled(
            service.display_name.clone(),
        ));
    }

    match fs::read_to_string(&error_log_path) {
        // `Out-File -Encoding utf8` on Windows PowerShell writes a BOM.
        Ok(detail) if !detail.trim_start_matches('\u{feff}').trim().is_empty() => Err(failed(
            detail.trim_start_matches('\u{feff}').trim().to_string(),
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name reaches PowerShell as one literal, whatever it contains.
    #[test]
    fn the_stop_script_quotes_the_service_name() {
        let script = stop_script("it's a service", false, r"C:\x\log.txt");
        assert!(script.contains("Stop-Service -Name 'it''s a service' -ErrorAction Stop"));
        assert!(!script.contains("Set-Service"));
        assert!(!script.contains("-Force"));
    }

    #[test]
    fn the_start_type_is_only_changed_when_asked() {
        let script = stop_script("pg", true, r"C:\x\log.txt");
        assert!(script.contains("Set-Service -Name 'pg' -StartupType Manual -ErrorAction Stop"));
    }

    /// This test process was started by cargo, not by the SCM.
    #[test]
    fn an_ordinary_process_belongs_to_no_service() {
        assert_eq!(owning(std::process::id(), None), None);
    }

    /// The SCM answers without admin rights, and every Windows machine runs
    /// a few services in a process of their own.
    #[test]
    fn running_services_can_be_listed() {
        let running = running_services();
        assert!(!running.is_empty());
        assert!(running.iter().all(|s| s.pid != 0 && !s.name.is_empty()));
    }
}
