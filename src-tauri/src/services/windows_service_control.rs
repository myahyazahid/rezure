//! Starting and stopping one Windows service from Rezure — and, with the
//! user's one-time say-so, without an admin prompt each time.
//!
//! Starting or stopping a service takes admin rights by default, so every
//! click would raise UAC. Windows lets a service's own permissions grant
//! start and stop to a particular account, though, and that's what
//! [`allow`] does: one UAC prompt adds an entry for *this* Windows account,
//! on *this* service only — start, stop, status — to its security
//! descriptor (`sc.exe sdset`). From then on [`control`] talks to the
//! Service Control Manager directly. [`disallow`] takes the entry back out.
//!
//! Without the entry, [`control`] still works; it just goes through UAC
//! (`utils::elevation`) every time.

use std::process::Command;
use std::time::{Duration, Instant};

use serde::Deserialize;
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_NOT_ACTIVE,
};
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
    StartServiceW, SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
    SERVICE_START, SERVICE_STATUS, SERVICE_STOP, SERVICE_STOPPED,
};

use crate::utils::command::HiddenWindow;
use crate::utils::elevation::{self, Logged};
use crate::utils::powershell::quote_ps;

/// How long a start or stop is waited on before it's reported as stuck.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(45);

/// What to do to a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Start,
    Stop,
    /// Stop, then start; a stopped service is simply started.
    Restart,
}

/// How an elevated fallback ended — mirrors `elevation::Logged`, for
/// callers that word their own errors.
pub enum Outcome {
    Done,
    Declined,
    Failed(String),
}

/// The rights [`allow`] grants: start (`RP`), stop (`WP`), query status
/// (`LC`) and interrogate (`LO`) — nothing that changes the service.
const RIGHTS: &str = "RPWPLCLO";

fn ace(sid: &str) -> String {
    format!("(A;;{RIGHTS};;;{sid})")
}

/// `sddl` with an allow entry for `sid` added to its DACL — before the SACL
/// (`S:`) if there is one, at the end otherwise. Unchanged if it's there.
fn with_ace(sddl: &str, sid: &str) -> String {
    let entry = ace(sid);
    if sddl.contains(&entry) {
        return sddl.to_string();
    }
    match sddl.find("S:") {
        Some(at) => format!("{}{entry}{}", &sddl[..at], &sddl[at..]),
        None => format!("{sddl}{entry}"),
    }
}

/// `sddl` with the entry [`with_ace`] adds taken back out.
fn without_ace(sddl: &str, sid: &str) -> String {
    sddl.replace(&ace(sid), "")
}

/// The signed-in account's SID (`S-1-5-21-…`), the one Rezure runs as.
fn current_sid() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: the token is closed on every path once opened. The buffer is
    // sized from the first call's `needed`, `u64`-backed so it's aligned for
    // `TOKEN_USER`, and outlives the SID inside it, which is only read by
    // `ConvertSidToStringSidW`. That call's string is read, then freed with
    // `LocalFree` as its documentation requires.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut needed = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
        );
        CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 || text.is_null() {
            return None;
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Some(sid)
    }
}

/// `name`'s security descriptor as SDDL, from `sc.exe sdshow` — readable
/// without admin rights for ordinary services.
fn sddl(name: &str) -> Option<String> {
    let output = Command::new("sc.exe")
        .args(["sdshow", name])
        .hidden()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("D:") || line.starts_with("O:"))
        .map(str::to_string)
}

/// The elevated statements that set `name`'s security descriptor to `sddl`.
/// `sc.exe` is a native program, so its exit code is checked by hand —
/// `-ErrorAction` doesn't reach it.
fn sdset_statements(name: &str, sddl: &str) -> String {
    format!(
        "& sc.exe sdset {} {}\nif ($LASTEXITCODE -ne 0) {{ throw \"sc.exe sdset failed with exit code $LASTEXITCODE\" }}\n",
        quote_ps(name),
        quote_ps(sddl)
    )
}

/// Opens `name` with the start, stop and status rights, or `None` without
/// them — which is exactly the question [`can_control`] asks.
///
/// # Safety
/// The returned handles must be closed with `CloseServiceHandle`.
unsafe fn open(name: &str) -> Option<(*mut core::ffi::c_void, *mut core::ffi::c_void)> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
    if scm.is_null() {
        return None;
    }
    let service = OpenServiceW(
        scm,
        wide.as_ptr(),
        SERVICE_START | SERVICE_STOP | SERVICE_QUERY_STATUS,
    );
    if service.is_null() {
        CloseServiceHandle(scm);
        return None;
    }
    Some((scm, service))
}

/// Whether this account may start and stop `name` without elevating — true
/// once [`allow`] has run (or when Rezure runs as administrator).
pub fn can_control(name: &str) -> bool {
    // SAFETY: both handles are closed right after they're opened.
    unsafe {
        match open(name) {
            Some((scm, service)) => {
                CloseServiceHandle(service);
                CloseServiceHandle(scm);
                true
            }
            None => false,
        }
    }
}

/// Grants this account start and stop on `name`, through one UAC prompt.
pub fn allow(name: &str, stem: &str) -> std::io::Result<Outcome> {
    change_sddl(name, stem, with_ace)
}

/// Takes back what [`allow`] granted. Nothing to do if it was never granted.
pub fn disallow_statements(name: &str) -> Option<String> {
    let sid = current_sid()?;
    let current = sddl(name)?;
    let next = without_ace(&current, &sid);
    (next != current).then(|| sdset_statements(name, &next))
}

fn change_sddl(
    name: &str,
    stem: &str,
    change: fn(&str, &str) -> String,
) -> std::io::Result<Outcome> {
    let (Some(sid), Some(current)) = (current_sid(), sddl(name)) else {
        return Ok(Outcome::Failed(format!(
            "couldn't read {name}'s permissions or this account's identity"
        )));
    };
    let next = change(&current, &sid);
    if next == current {
        return Ok(Outcome::Done);
    }
    log::info!("granting this account start/stop on {name}");
    run_elevated(stem, &sdset_statements(name, &next))
}

fn run_elevated(stem: &str, statements: &str) -> std::io::Result<Outcome> {
    let dir = crate::utils::paths::data()
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .join("services");
    Ok(match elevation::run_logged(&dir, stem, statements)? {
        Logged::Done => Outcome::Done,
        Logged::Declined => Outcome::Declined,
        Logged::Failed(reason) => Outcome::Failed(reason),
    })
}

/// The service's current state, or `None` if it can't be read.
///
/// # Safety
/// `service` must be a live handle opened with `SERVICE_QUERY_STATUS`.
unsafe fn state(service: *mut core::ffi::c_void) -> Option<u32> {
    let mut status: SERVICE_STATUS = std::mem::zeroed();
    (QueryServiceStatus(service, &mut status) != 0).then_some(status.dwCurrentState)
}

/// Waits for `service` to reach `target`.
///
/// # Safety
/// As for [`state`].
unsafe fn settle(service: *mut core::ffi::c_void, target: u32) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < SETTLE_TIMEOUT {
        if state(service) == Some(target) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(format!(
        "it didn't {} within {}s",
        if target == SERVICE_RUNNING {
            "start"
        } else {
            "stop"
        },
        SETTLE_TIMEOUT.as_secs()
    ))
}

/// Does `action` to `name` directly, waiting until it's done. Requires the
/// rights [`can_control`] checks for.
fn control_direct(name: &str, action: Action) -> Result<(), String> {
    // SAFETY: the handles from `open` are closed on every path below, and
    // only ever used while open.
    unsafe {
        let Some((scm, service)) = open(name) else {
            return Err("this account isn't allowed to control it".to_string());
        };
        let result = (|| {
            if matches!(action, Action::Stop | Action::Restart)
                && state(service) != Some(SERVICE_STOPPED)
            {
                let mut status: SERVICE_STATUS = std::mem::zeroed();
                if ControlService(service, SERVICE_CONTROL_STOP, &mut status) == 0
                    && GetLastError() != ERROR_SERVICE_NOT_ACTIVE
                {
                    return Err(format!(
                        "stopping it failed (Windows error {})",
                        GetLastError()
                    ));
                }
                settle(service, SERVICE_STOPPED)?;
            }
            if matches!(action, Action::Start | Action::Restart) {
                if StartServiceW(service, 0, std::ptr::null()) == 0
                    && GetLastError() != ERROR_SERVICE_ALREADY_RUNNING
                {
                    return Err(format!(
                        "starting it failed (Windows error {})",
                        GetLastError()
                    ));
                }
                settle(service, SERVICE_RUNNING)?;
            }
            Ok(())
        })();
        CloseServiceHandle(service);
        CloseServiceHandle(scm);
        result
    }
}

/// The elevated equivalent of [`control_direct`].
fn control_statements(name: &str, action: Action) -> String {
    let name = quote_ps(name);
    match action {
        Action::Start => format!("Start-Service -Name {name} -ErrorAction Stop\n"),
        Action::Stop => format!("Stop-Service -Name {name} -ErrorAction Stop\n"),
        // `Restart-Service` starts a stopped service too.
        Action::Restart => format!("Restart-Service -Name {name} -ErrorAction Stop\n"),
    }
}

/// Starts, stops or restarts `name`: directly when this account is allowed
/// to ([`allow`]), through a UAC prompt otherwise.
pub fn control(name: &str, action: Action, stem: &str) -> std::io::Result<Outcome> {
    if can_control(name) {
        log::info!("{action:?} {name} (no prompt needed)");
        return Ok(match control_direct(name, action) {
            Ok(()) => Outcome::Done,
            Err(reason) => Outcome::Failed(reason),
        });
    }
    log::info!("{action:?} {name} through UAC");
    run_elevated(stem, &control_statements(name, action))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "S-1-5-21-1-2-3-1001";

    #[test]
    fn the_entry_goes_into_the_dacl_once_and_comes_back_out() {
        let original = "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCLCSWLOCRRC;;;IU)";
        let granted = with_ace(original, SID);
        assert_eq!(
            granted,
            "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCLCSWLOCRRC;;;IU)(A;;RPWPLCLO;;;S-1-5-21-1-2-3-1001)"
        );
        assert_eq!(with_ace(&granted, SID), granted, "added once only");
        assert_eq!(without_ace(&granted, SID), original);
    }

    /// An audit list (`S:`) stays last: the entry goes before it.
    #[test]
    fn the_entry_goes_before_an_audit_list() {
        let original = "D:(A;;CCLCSWLOCRRC;;;IU)S:(AU;FA;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;WD)";
        assert_eq!(
            with_ace(original, SID),
            "D:(A;;CCLCSWLOCRRC;;;IU)(A;;RPWPLCLO;;;S-1-5-21-1-2-3-1001)S:(AU;FA;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;WD)"
        );
    }

    #[test]
    fn the_sdset_statement_quotes_and_checks_the_native_exit_code() {
        let statements = sdset_statements("odoo-server-20.0", "D:(A;;RPWPLCLO;;;S-1-5)");
        assert!(
            statements.starts_with("& sc.exe sdset 'odoo-server-20.0' 'D:(A;;RPWPLCLO;;;S-1-5)'\n")
        );
        assert!(statements.contains("$LASTEXITCODE -ne 0"));
    }

    #[test]
    fn this_accounts_sid_and_a_services_sddl_can_be_read() {
        let sid = current_sid().expect("the signed-in account has a SID");
        assert!(sid.starts_with("S-1-"), "{sid}");
        // Every Windows install has the Event Log service.
        let sddl = sddl("EventLog").expect("EventLog's permissions are readable");
        assert!(sddl.contains("D:"), "{sddl}");
    }
}
