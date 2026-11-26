//! Why SQL Server LocalDB won't start on this machine, and the one fix
//! Rezure can apply for the most common reason.
//!
//! # The disk sector problem
//!
//! SQL Server — every version, LocalDB included — only runs on disks whose
//! physical sector is 512 or 4096 bytes. Windows 11's own NVMe driver
//! (`stornvme`) reports what the drive itself says, and some NVMe drives say
//! 8 KB, 16 KB or more. On those, `sqlservr.exe` dies during startup with
//! "misaligned log IOs" in its error log, and `SqlLocalDB start` reports
//! only a generic failure. Nothing about the SQL Server version fixes it,
//! which is why a newer LocalDB is not the answer.
//!
//! Microsoft's documented workaround is one registry value that makes
//! `stornvme` report 4 KB again, as Windows 10 did, and a restart:
//! <https://learn.microsoft.com/en-us/troubleshoot/sql/database-engine/database-file-operations/troubleshoot-os-4kb-disk-sector-size>.
//! [`apply_sector_fix`] sets it through one UAC prompt, and only when the
//! user asks for it.
//!
//! # How it's detected
//!
//! The sector size is asked of Windows directly
//! (`GetFileInformationByHandleEx` → `FILE_STORAGE_INFO`), not read out of
//! `fsutil fsinfo sectorinfo`: `fsutil` needs administrator rights, and
//! prints in the language Windows is set to. The value checked is
//! `PhysicalBytesPerSectorForAtomicity`, the one the Microsoft article says
//! to check. When it can't be read, a "misaligned" or "sector size" line in
//! the instance's own `error.log` is taken as the answer instead.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use super::mssql_localdb;
use crate::utils::elevation::{self, Logged};
use crate::utils::error::AppError;
use crate::utils::paths;
use crate::utils::powershell::quote_ps;

/// The largest physical sector SQL Server accepts.
const MAX_SUPPORTED_SECTOR: u32 = 4096;

/// Where Microsoft's workaround lives, under `HKEY_LOCAL_MACHINE`.
const NVME_KEY: &str = r"SYSTEM\CurrentControlSet\Services\stornvme\Parameters\Device";
const NVME_VALUE: &str = "ForcedPhysicalSectorSizeInBytes";
/// The value Microsoft's article gives: every device (`*`) reports 4095,
/// which `stornvme` rounds to a 4 KB sector.
const NVME_FIX: &str = "* 4095";

/// What the disk under LocalDB means for starting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SectorVerdict {
    /// 512 or 4096 bytes: whatever stops LocalDB, it isn't this.
    Supported,
    /// Too large, and the registry fix isn't there.
    NeedsFix,
    /// Too large, the fix is there, and Windows hasn't restarted since it
    /// was set.
    RestartPending,
    /// Too large even though the fix was in place before this boot: the disk
    /// is driven by something other than Windows' own NVMe driver, which is
    /// the only one the fix reaches.
    FixIneffective,
    /// Couldn't be read, and nothing in the error log points at it.
    Unknown,
}

/// The LocalDB disk check, as the Services page and Project Doctor show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDbHealth {
    pub verdict: SectorVerdict,
    /// The physical sector size Windows reports for the disk LocalDB keeps
    /// its files on, in bytes. `None` when it couldn't be read.
    pub sector_bytes: Option<u32>,
    /// Microsoft's registry workaround is set.
    pub fix_applied: bool,
}

/// Whether the registry fix is there, and whether this boot has picked it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixState {
    Absent,
    /// Set since Windows last started — not in effect yet.
    SetSinceBoot,
    /// Set before Windows last started — in effect, if it applies at all.
    Active,
}

fn verdict(sector: Option<u32>, log_mentions_sector: bool, fix: FixState) -> SectorVerdict {
    // The live reading wins over the log: an error.log line can be left
    // over from before the fix and the restart.
    let too_large = match sector {
        Some(bytes) => bytes > MAX_SUPPORTED_SECTOR,
        None => log_mentions_sector,
    };
    if !too_large {
        return if sector.is_some() {
            SectorVerdict::Supported
        } else {
            SectorVerdict::Unknown
        };
    }
    match fix {
        FixState::Absent => SectorVerdict::NeedsFix,
        FixState::SetSinceBoot => SectorVerdict::RestartPending,
        FixState::Active => SectorVerdict::FixIneffective,
    }
}

/// `%LOCALAPPDATA%\Microsoft\Microsoft SQL Server Local DB\Instances\Rezure`
/// — where LocalDB keeps the instance's system databases and its
/// `error.log`. Fixed by LocalDB; not something Rezure chooses.
fn instance_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("Microsoft")
            .join("Microsoft SQL Server Local DB")
            .join("Instances")
            .join(mssql_localdb::INSTANCE),
    )
}

/// Checks the disk LocalDB runs from. Cheap — a file handle and a registry
/// read — so the Services page can ask on every load.
pub fn check() -> LocalDbHealth {
    let sector = [instance_dir(), paths::data().ok()]
        .into_iter()
        .flatten()
        .filter_map(|path| physical_sector_bytes(&path))
        .max();
    let log_mentions_sector =
        sector.is_none() && read_error_log().is_some_and(|text| mentions_sector(&text));
    let fix = fix_state();
    LocalDbHealth {
        verdict: verdict(sector, log_mentions_sector, fix),
        sector_bytes: sector,
        fix_applied: fix != FixState::Absent,
    }
}

/// How the sector size reads in a message.
fn describe_sector(bytes: Option<u32>) -> String {
    match bytes {
        Some(bytes) => format!("{bytes}-byte disk sectors"),
        None => format!("disk sectors larger than {MAX_SUPPORTED_SECTOR} bytes"),
    }
}

/// The error a failed `SqlLocalDB create`/`start` should surface: the disk
/// problem when that's what it is, otherwise the tool's own reason with
/// whatever the instance logged since `since`.
pub fn start_failure(reason: String, since: SystemTime) -> AppError {
    let health = check();
    let sector = describe_sector(health.sector_bytes);
    match health.verdict {
        SectorVerdict::NeedsFix => AppError::LocalDbSectorSize { sector },
        SectorVerdict::RestartPending => AppError::LocalDbRestartPending { sector },
        SectorVerdict::FixIneffective => AppError::LocalDbFixIneffective { sector },
        SectorVerdict::Supported | SectorVerdict::Unknown => {
            let logged = recent_error_log(since).and_then(|text| logged_errors(&text));
            AppError::LocalDbStartFailed {
                reason: match logged {
                    Some(logged) => format!("{reason} (error.log: {logged})"),
                    None => reason,
                },
            }
        }
    }
}

/// Sets Microsoft's registry workaround through one UAC prompt, then reads
/// the disk check again. Takes effect after Windows restarts.
///
/// The key is only created when it's missing: `New-Item -Force` on a
/// registry key that already exists recreates it empty, which would drop
/// any other value someone set there.
pub fn apply_sector_fix() -> Result<LocalDbHealth, AppError> {
    let key = quote_ps(&format!(r"HKLM:\{NVME_KEY}"));
    let statements = format!(
        "if (-not (Test-Path -LiteralPath {key})) {{ New-Item -Path {key} -Force -ErrorAction Stop | Out-Null }}\n\
         New-ItemProperty -LiteralPath {key} -Name {name} -PropertyType MultiString -Value @({value}) -Force -ErrorAction Stop | Out-Null",
        name = quote_ps(NVME_VALUE),
        value = quote_ps(NVME_FIX),
    );
    let dir = paths::data()?.join("installers");
    match elevation::run_logged(&dir, "localdb-sector-fix", &statements)
        .map_err(|e| AppError::LocalDbFixFailed(e.to_string()))?
    {
        Logged::Declined => return Err(AppError::LocalDbFixCancelled),
        Logged::Failed(reason) => return Err(AppError::LocalDbFixFailed(reason)),
        Logged::Done => {}
    }
    // Judged by the registry, not by the elevated script's exit.
    if fix_state() == FixState::Absent {
        return Err(AppError::LocalDbFixFailed(
            "the registry value isn't there after the script ran".to_string(),
        ));
    }
    log::info!("applied the NVMe sector size workaround for SQL Server LocalDB");
    Ok(check())
}

/// The closest existing folder at or above `path` — the instance folder
/// doesn't exist before the instance does, but its drive is what matters.
fn existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|p| p.is_dir())
}

/// `PhysicalBytesPerSectorForAtomicity` of the volume `path` is on.
fn physical_sector_bytes(path: &Path) -> Option<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FileStorageInfo, GetFileInformationByHandleEx, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FILE_STORAGE_INFO, OPEN_EXISTING,
    };

    let dir = existing_ancestor(path)?;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call;
    // the handle is closed on every path after it was opened; and `info` is
    // a `FILE_STORAGE_INFO` whose exact size is what the call may write.
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            // Needed to open a directory at all.
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info: FILE_STORAGE_INFO = std::mem::zeroed();
        let ok = GetFileInformationByHandleEx(
            handle,
            FileStorageInfo,
            (&mut info as *mut FILE_STORAGE_INFO).cast(),
            std::mem::size_of::<FILE_STORAGE_INFO>() as u32,
        );
        CloseHandle(handle);
        (ok != 0 && info.PhysicalBytesPerSectorForAtomicity > 0)
            .then_some(info.PhysicalBytesPerSectorForAtomicity)
    }
}

/// Whether [`NVME_VALUE`] holds the fix, and whether it was written before
/// Windows last started.
fn fix_state() -> FixState {
    let Some((value, written)) = read_nvme_value() else {
        return FixState::Absent;
    };
    if !value.iter().any(|entry| entry.trim() == NVME_FIX) {
        return FixState::Absent;
    }
    match (written, boot_time()) {
        (Some(written), Some(boot)) if written > boot => FixState::SetSinceBoot,
        _ => FixState::Active,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// The `REG_MULTI_SZ` entries of [`NVME_VALUE`], and when its key was last
/// written. `None` when the key or the value isn't there.
fn read_nvme_value() -> Option<(Vec<String>, Option<SystemTime>)> {
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, FILETIME};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ, RRF_RT_REG_MULTI_SZ,
    };

    let key_name = wide(NVME_KEY);
    let value_name = wide(NVME_VALUE);
    // SAFETY: every pointer handed over is either null where the API allows
    // it or points at a live local sized as the API is told; the key is
    // closed on every path after it was opened.
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, key_name.as_ptr(), 0, KEY_READ, &mut key)
            != ERROR_SUCCESS
        {
            return None;
        }
        let mut size = 0u32;
        let found = RegGetValueW(
            key,
            std::ptr::null(),
            value_name.as_ptr(),
            RRF_RT_REG_MULTI_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        ) == ERROR_SUCCESS;
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let read = found
            && RegGetValueW(
                key,
                std::ptr::null(),
                value_name.as_ptr(),
                RRF_RT_REG_MULTI_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            ) == ERROR_SUCCESS;
        let mut written = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let has_time = RegQueryInfoKeyW(
            key,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut written,
        ) == ERROR_SUCCESS;
        RegCloseKey(key);
        if !read {
            return None;
        }
        let entries = String::from_utf16_lossy(&buffer)
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .map(str::to_string)
            .collect();
        let written = has_time.then(|| {
            filetime_to_system_time(
                (u64::from(written.dwHighDateTime) << 32) | u64::from(written.dwLowDateTime),
            )
        });
        Some((entries, written))
    }
}

/// A `FILETIME` — 100-nanosecond ticks since 1601 — as a `SystemTime`.
fn filetime_to_system_time(ticks: u64) -> SystemTime {
    const UNIX_EPOCH_IN_TICKS: u64 = 116_444_736_000_000_000;
    SystemTime::UNIX_EPOCH + Duration::from_nanos(ticks.saturating_sub(UNIX_EPOCH_IN_TICKS) * 100)
}

/// When Windows last started.
fn boot_time() -> Option<SystemTime> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    // SAFETY: takes nothing and can't fail.
    let uptime = Duration::from_millis(unsafe { GetTickCount64() });
    SystemTime::now().checked_sub(uptime)
}

/// The instance's `error.log` as text. SQL Server writes it as UTF-16.
fn read_error_log() -> Option<String> {
    let bytes = std::fs::read(instance_dir()?.join("error.log")).ok()?;
    Some(decode_log(&bytes))
}

/// The `error.log`, but only if it was written to at or after `since` — an
/// older one is about some earlier run, not the start that just failed.
fn recent_error_log(since: SystemTime) -> Option<String> {
    let path = instance_dir()?.join("error.log");
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    // A little slack: the file's timestamp and the clock read before the
    // start aren't taken at the same instant.
    if modified + Duration::from_secs(2) < since {
        return None;
    }
    Some(decode_log(&std::fs::read(path).ok()?))
}

fn decode_log(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether SQL Server logged the disk sector problem.
fn mentions_sector(log: &str) -> bool {
    let lower = log.to_ascii_lowercase();
    lower.contains("misaligned") || lower.contains("sector size")
}

/// A log line without its `2026-11-26 10:05:19.46 spid31s` prefix.
fn strip_log_prefix(line: &str) -> &str {
    let mut rest = line.trim();
    for _ in 0..3 {
        match rest.split_once(char::is_whitespace) {
            Some((_, after)) => rest = after.trim_start(),
            None => return rest,
        }
    }
    rest
}

/// The last few errors SQL Server logged — each `Error: N, Severity: …` line
/// is followed by the sentence that explains it, which is the part shown.
fn logged_errors(log: &str) -> Option<String> {
    let lines: Vec<&str> = log.lines().map(strip_log_prefix).collect();
    let mut found: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("Error:"))
        .filter_map(|(i, _)| lines.get(i + 1).copied())
        .filter(|message| !message.is_empty())
        .collect();
    found.dedup();
    let start = found.len().saturating_sub(2);
    let tail = &found[start..];
    (!tail.is_empty()).then(|| tail.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_4k_or_smaller_disk_is_fine_whatever_the_log_says() {
        assert_eq!(
            verdict(Some(4096), true, FixState::Absent),
            SectorVerdict::Supported
        );
        assert_eq!(
            verdict(Some(512), false, FixState::Absent),
            SectorVerdict::Supported
        );
    }

    #[test]
    fn a_larger_sector_needs_the_fix_then_a_restart() {
        assert_eq!(
            verdict(Some(16384), false, FixState::Absent),
            SectorVerdict::NeedsFix
        );
        assert_eq!(
            verdict(Some(16384), false, FixState::SetSinceBoot),
            SectorVerdict::RestartPending
        );
    }

    #[test]
    fn a_fix_that_survived_a_restart_without_effect_is_reported_as_such() {
        assert_eq!(
            verdict(Some(8192), false, FixState::Active),
            SectorVerdict::FixIneffective
        );
    }

    #[test]
    fn without_a_reading_the_log_decides() {
        assert_eq!(
            verdict(None, true, FixState::Absent),
            SectorVerdict::NeedsFix
        );
        assert_eq!(
            verdict(None, false, FixState::Absent),
            SectorVerdict::Unknown
        );
    }

    #[test]
    fn the_misaligned_io_line_is_recognised() {
        assert!(mentions_sector(
            "2026-11-26 10:05:19.46 spid9s  There have been 256 misaligned log IOs which required falling back to synchronous IO."
        ));
        assert!(!mentions_sector("Recovery is complete."));
    }

    #[test]
    fn logged_errors_are_the_sentences_after_each_error_line() {
        let log = "2026-11-26 10:05:19.46 spid9s      Starting up database 'master'.\r\n\
            2026-11-26 10:05:19.47 spid9s      Error: 5172, Severity: 16, State: 15.\r\n\
            2026-11-26 10:05:19.47 spid9s      The header for file 'master.mdf' is not a valid database file header.\r\n\
            2026-11-26 10:05:19.48 spid9s      Error: 17204, Severity: 16, State: 1.\r\n\
            2026-11-26 10:05:19.48 spid9s      FCB::Open failed: Could not open file master.mdf.\r\n";
        assert_eq!(
            logged_errors(log).as_deref(),
            Some("The header for file 'master.mdf' is not a valid database file header. FCB::Open failed: Could not open file master.mdf.")
        );
        assert_eq!(
            logged_errors("2026-11-26 10:05:19.46 spid9s  Recovery is complete."),
            None
        );
    }

    #[test]
    fn the_log_is_read_as_utf16() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("Error: 1".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_log(&bytes), "Error: 1");
    }

    #[test]
    fn a_filetime_converts_to_unix_time() {
        // 2024-01-01T00:00:00Z
        let ticks = 133_485_408_000_000_000;
        assert_eq!(
            filetime_to_system_time(ticks)
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1_704_067_200
        );
    }

    /// The real reading on this machine.
    /// `cargo test --lib services::localdb_health::tests::print_real_health -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_real_health() {
        println!("{:?}", check());
    }
}
