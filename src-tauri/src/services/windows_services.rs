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
//!
//! It also answers "is this service installed, and is it running" for apps
//! Rezure connects to its own servers (`services::odoo`), which come as a
//! Windows service of their own.

use serde::Serialize;
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW, QueryServiceConfigW,
    ENUM_SERVICE_STATUS_PROCESSW, QUERY_SERVICE_CONFIGW, SC_ENUM_PROCESS_INFO, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_AUTO_START,
    SERVICE_QUERY_CONFIG, SERVICE_RUNNING, SERVICE_STATE_ALL, SERVICE_WIN32,
    SERVICE_WIN32_SHARE_PROCESS,
};

use crate::utils::elevation::{self, Logged};
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

/// An installed Windows service and whether it's up — what an app that
/// runs as one (Odoo) is shown with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    pub name: String,
    pub display_name: String,
    pub running: bool,
    pub starts_with_windows: bool,
}

/// One row of the SCM's service list.
struct Listed {
    /// 0 while it isn't running.
    pid: u32,
    name: String,
    display_name: String,
    /// Hosted in a shared `svchost.exe` alongside other services.
    shared: bool,
    running: bool,
}

/// Every Win32 service, running or not, for which `matches(name,
/// display_name)` holds — with its start type, which is read per service and
/// so only for the ones asked about.
pub fn find(matches: impl Fn(&str, &str) -> bool) -> Vec<ServiceState> {
    listed(SERVICE_STATE_ALL)
        .into_iter()
        .filter(|s| matches(&s.name, &s.display_name))
        .map(|s| ServiceState {
            starts_with_windows: starts_automatically(&s.name),
            name: s.name,
            display_name: s.display_name,
            running: s.running,
        })
        .collect()
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
    let running = listed(SERVICE_ACTIVE);
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

/// Every Win32 service in `state` (`SERVICE_ACTIVE`, `SERVICE_STATE_ALL`)
/// with the process it runs in, from the Service Control Manager.
/// Enumerating needs no admin rights. Empty if the SCM can't be asked —
/// callers then know of no service, rather than a wrong one.
fn listed(state: u32) -> Vec<Listed> {
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
            state,
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
            state,
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
            .map(|entry| Listed {
                pid: entry.ServiceStatusProcess.dwProcessId,
                name: wide_to_string(entry.lpServiceName),
                display_name: wide_to_string(entry.lpDisplayName),
                shared: entry.ServiceStatusProcess.dwServiceType & SERVICE_WIN32_SHARE_PROCESS != 0,
                running: entry.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING,
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

/// The elevated statements that stop `name`, and with `disable_autostart`
/// set it to start Manually so it doesn't take the port back at the next
/// boot.
///
/// No `-Force` on `Stop-Service`: that would quietly stop every service that
/// depends on this one too. Without it, a service with dependents fails with
/// a message saying so, which is what the user should see.
pub fn stop_statements(name: &str, disable_autostart: bool) -> String {
    let name = quote_ps(name);
    let mut statements = format!("Stop-Service -Name {name} -ErrorAction Stop\n");
    if disable_autostart {
        statements.push_str(&format!(
            "Set-Service -Name {name} -StartupType Manual -ErrorAction Stop\n"
        ));
    }
    statements
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
    log::info!(
        "stopping Windows service {} (disable autostart: {disable_autostart})",
        service.name
    );
    match elevation::run_logged(
        &paths::data()?.join("ports"),
        "stop-service",
        &stop_statements(&service.name, disable_autostart),
    )
    .map_err(|e| failed(e.to_string()))?
    {
        Logged::Done => Ok(()),
        Logged::Declined => Err(AppError::WindowsServiceStopCancelled(
            service.display_name.clone(),
        )),
        Logged::Failed(reason) => Err(failed(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name reaches PowerShell as one literal, whatever it contains.
    #[test]
    fn the_stop_script_quotes_the_service_name() {
        let script = stop_statements("it's a service", false);
        assert!(script.contains("Stop-Service -Name 'it''s a service' -ErrorAction Stop"));
        assert!(!script.contains("Set-Service"));
        assert!(!script.contains("-Force"));
    }

    #[test]
    fn the_start_type_is_only_changed_when_asked() {
        let script = stop_statements("pg", true);
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
        let running = listed(SERVICE_ACTIVE);
        assert!(!running.is_empty());
        assert!(running.iter().all(|s| s.pid != 0 && !s.name.is_empty()));
    }
}
