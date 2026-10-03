//! Installs the two Microsoft packages SQL Server support needs and that
//! can't be portable: the ODBC Driver, and SQL Server Express LocalDB.
//!
//! # Why these aren't zips like everything else
//!
//! Every other runtime Rezure installs is a portable archive it unpacks into
//! its own folder (`services::binaries`). Microsoft publishes neither of
//! these that way — only as `.msi` installers — and neither could work
//! portably anyway: an ODBC driver is found through its registration under
//! `HKLM\SOFTWARE\ODBC`, and LocalDB through its own `HKLM` keys. So
//! installing them is a real Windows install: one UAC prompt, an entry in
//! Settings → Apps, and something that outlives Rezure if Rezure is
//! uninstalled.
//!
//! # What is pinned and checked
//!
//! * The download URL is the versioned `download.microsoft.com` path a
//!   release lives at — not the `fwlink` on Microsoft's download page, which
//!   always points at the newest build and so could never match a pinned
//!   checksum.
//! * The SHA-256 is pinned here, verified the same way as every other
//!   download (`binaries::verify_checksum`).
//! * The file's Authenticode signature must be valid and Microsoft's. With a
//!   pinned hash this is a second lock on the same door, but it is the one a
//!   reader can check for themselves in the file's Properties dialog.
//!
//! # The license
//!
//! Both installers refuse to run silently unless a property saying the
//! license was accepted is set on the command line. Rezure only ever sets it
//! when the caller passes `license_accepted` — which the UI only does after
//! the user ticked a box next to a link to the license. It is never set on
//! the user's behalf.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use super::binaries::{self, BinaryStatus, InstallProgress, InstallStage};
use super::{mssql_localdb, odbc};
use crate::utils::elevation::{self, Elevated};
use crate::utils::error::AppError;
use crate::utils::paths;
use crate::utils::powershell::{self, quote_ps};

pub struct MsiPackage {
    /// Shared with `binaries` ids, so one Install button and one progress
    /// event stream serve both kinds.
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub download_url: &'static str,
    pub sha256: &'static str,
    /// Shown beside the checkbox the user ticks before installing.
    pub license_url: &'static str,
    /// The public property the installer insists on before a silent run.
    pub license_property: &'static str,
    /// Whether the package is already there — the only reliable success
    /// check after the installer has run (see [`install`]).
    pub is_installed: fn() -> bool,
}

/// Verified against the downloads on 2026-10-03: checksum below, and a valid
/// Authenticode signature from `CN=Microsoft Corporation` on both.
pub const PACKAGES: &[MsiPackage] = &[
    MsiPackage {
        id: "msodbcsql",
        name: "Microsoft ODBC Driver 18 for SQL Server",
        // 18.7 is the first release that no longer needs the Visual C++
        // Redistributable installed beforehand — an older pin would mean a
        // second installer, or a driver that registers and then won't load.
        version: "18.7.1.1",
        download_url: "https://download.microsoft.com/download/d624e1c6-293b-4d6f-91b8-6e515a5d6a77/amd64/1033/msodbcsql.msi",
        sha256: "21ef69e4b942f18aced55fa7c3a8d7263004e113a96690bac7509645e1b1a310",
        // The license text the installer itself shows, as Microsoft hosts it.
        license_url: "https://aka.ms/odbc18eularedist",
        license_property: "IACCEPTMSODBCSQLLICENSETERMS",
        is_installed: odbc::is_installed,
    },
    MsiPackage {
        id: "sqllocaldb",
        name: "SQL Server 2022 Express LocalDB",
        version: "16.0.1000.6",
        download_url: "https://download.microsoft.com/download/3/8/d/38de7036-2433-4207-8eae-06e247e17b25/SqlLocalDB.msi",
        sha256: "224d483992ef60368dac70cea174dcfaf43a3ca06ada331c67dc6119a26490f6",
        // LocalDB is an edition of SQL Server Express; this is Microsoft's
        // published license for the 2022 Developer/Express/Evaluation
        // editions.
        license_url: "https://www.microsoft.com/content/dam/microsoft/usetm/documents/sql-server/sql-server-2022-developer-express-evaluation/retail-packaged/SQLServer2022_SQLServer2022DeveloperExpressEvaluation_English.pdf",
        license_property: "IACCEPTSQLLOCALDBLICENSETERMS",
        is_installed: mssql_localdb::is_installed,
    },
];

pub fn find(id: &str) -> Option<&'static MsiPackage> {
    PACKAGES.iter().find(|pkg| pkg.id == id)
}

pub fn status_of(pkg: &MsiPackage) -> BinaryStatus {
    BinaryStatus {
        id: pkg.id.to_string(),
        name: pkg.name.to_string(),
        version: pkg.version.to_string(),
        installed: (pkg.is_installed)(),
        license_url: Some(pkg.license_url.to_string()),
    }
}

/// `C:\rezure\data\installers` — where an installer is staged while it runs,
/// and where its log stays afterwards for anyone who needs to read why it
/// failed.
fn staging_dir() -> Result<PathBuf, AppError> {
    let dir = paths::data()?.join("installers");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Io(format!("could not create {}: {e}", dir.display())))?;
    Ok(dir)
}

fn progress(app: &AppHandle, id: &str, stage: InstallStage) {
    binaries::emit_progress(
        app,
        &InstallProgress {
            id: id.to_string(),
            stage,
            downloaded_bytes: 0,
            total_bytes: None,
        },
    );
}

/// Refuses anything not signed by Microsoft with a currently valid
/// signature. `Get-AuthenticodeSignature` is the same check Explorer's
/// "Digital Signatures" tab performs.
fn verify_signature(pkg: &MsiPackage, msi: &Path) -> Result<(), AppError> {
    let script = format!(
        "$s = Get-AuthenticodeSignature -LiteralPath {}; \"$($s.Status)|$($s.SignerCertificate.Subject)\"",
        quote_ps(&msi.display().to_string())
    );
    let output = powershell::run(&script, "checking the installer's signature")?;
    let (status, subject) = output.split_once('|').unwrap_or((output.as_str(), ""));
    if status.trim() == "Valid" && subject.contains("O=Microsoft Corporation") {
        Ok(())
    } else {
        Err(AppError::InstallerFailed {
            name: pkg.name.to_string(),
            reason: format!(
                "the downloaded installer isn't validly signed by Microsoft ({status}, {subject})"
            ),
        })
    }
}

/// The elevated script: runs the installer silently and writes its exit
/// code to `exit_file`.
///
/// Inside the elevated PowerShell, `Start-Process -Wait -PassThru` *does*
/// report a real exit code — the bogus-code problem `utils::elevation`
/// describes only affects the hop that crosses the UAC boundary. Each path
/// carries its own double quotes for the same reason that module's launcher
/// does: `-ArgumentList` joins elements with bare spaces.
fn install_script(pkg: &MsiPackage, msi: &Path, log: &Path, exit_file: &Path) -> String {
    let quoted = |path: &Path| quote_ps(&format!("\"{}\"", path.display()));
    format!(
        "$p = Start-Process -FilePath msiexec.exe -ArgumentList @('/i',{msi},'/qn','/norestart','{property}=YES','/l*v',{log}) -Wait -PassThru\n\
         $p.ExitCode | Out-File -FilePath {exit} -Encoding ascii\n",
        msi = quoted(msi),
        property = pkg.license_property,
        log = quoted(log),
        exit = quote_ps(&exit_file.display().to_string()),
    )
}

/// What a Windows Installer exit code means for the user — the documented
/// codes someone is realistically going to hit, in words, with the log as
/// the place to look for anything else.
fn explain_exit_code(code: i32, log: &Path) -> Option<String> {
    let reason = match code {
        // Success, and success-but-reboot-later.
        0 | 3010 | 1641 => return None,
        1602 => "the installer was cancelled".to_string(),
        1618 => "another installation is already running — wait for it to finish (Windows Update often holds this), then try again".to_string(),
        1638 => "another version of it is already installed — remove that one from Settings → Apps first".to_string(),
        1925 | 1303 => "Windows refused the install for lack of administrator rights".to_string(),
        other => format!(
            "Windows Installer exited with code {other} — the full log is at {}",
            log.display()
        ),
    };
    Some(reason)
}

/// Downloads, verifies and installs one package.
///
/// Idempotent: a package already present returns without touching the
/// network or raising a prompt. `license_accepted` must be true — see the
/// module docs.
pub async fn install(app: &AppHandle, id: &str, license_accepted: bool) -> Result<(), AppError> {
    let pkg = find(id).ok_or_else(|| AppError::UnknownBinary(id.to_string()))?;
    if !license_accepted {
        return Err(AppError::LicenseNotAccepted(pkg.name.to_string()));
    }
    if (pkg.is_installed)() {
        return Ok(());
    }

    let bytes = binaries::download(app, pkg.id, pkg.download_url).await?;
    progress(app, pkg.id, InstallStage::Verifying);
    binaries::verify_checksum(pkg.id, pkg.sha256, &bytes)?;

    progress(app, pkg.id, InstallStage::Installing);
    tokio::task::spawn_blocking(move || run_installer(pkg, &bytes))
        .await
        .map_err(|e| AppError::Io(format!("installer task panicked: {e}")))??;

    progress(app, pkg.id, InstallStage::Done);
    log::info!("installed {} {}", pkg.name, pkg.version);
    Ok(())
}

/// Everything after a verified download: stage the file, check its
/// signature, run it elevated, and confirm Windows really has the package
/// now. Blocks for as long as the UAC prompt and the installer take.
fn run_installer(pkg: &MsiPackage, bytes: &[u8]) -> Result<(), AppError> {
    let dir = staging_dir()?;
    let msi = dir.join(format!("{}-{}.msi", pkg.id, pkg.version));
    let log = dir.join(format!("{}-{}.log", pkg.id, pkg.version));
    let exit_file = dir.join(format!("{}.exitcode", pkg.id));
    let script_path = dir.join(format!("install-{}.ps1", pkg.id));
    std::fs::write(&msi, bytes)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", msi.display())))?;
    let _ = std::fs::remove_file(&exit_file);

    let outcome = verify_signature(pkg, &msi).and_then(|()| {
        std::fs::write(&script_path, install_script(pkg, &msi, &log, &exit_file))
            .map_err(|e| AppError::Io(format!("could not write {}: {e}", script_path.display())))?;
        elevation::run_script(&script_path).map_err(|e| AppError::InstallerFailed {
            name: pkg.name.to_string(),
            reason: e.to_string(),
        })
    });

    // The installer is 10–60 MB and has done its job either way; the log is
    // what's worth keeping.
    let _ = std::fs::remove_file(&msi);
    let _ = std::fs::remove_file(&script_path);

    if outcome? == Elevated::Declined {
        return Err(AppError::InstallerCancelled(pkg.name.to_string()));
    }

    let code = std::fs::read_to_string(&exit_file)
        .ok()
        .and_then(|raw| raw.trim().parse::<i32>().ok());
    let _ = std::fs::remove_file(&exit_file);
    if let Some(reason) = code.and_then(|code| explain_exit_code(code, &log)) {
        return Err(AppError::InstallerFailed {
            name: pkg.name.to_string(),
            reason,
        });
    }

    // The exit code is a claim; this is the fact. Windows has to actually
    // know about the package now, or nothing downstream will find it.
    if !(pkg.is_installed)() {
        return Err(AppError::InstallerFailed {
            name: pkg.name.to_string(),
            reason: format!(
                "the installer finished but Windows doesn't list it — see {}",
                log.display()
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_is_a_sha256_and_a_versioned_microsoft_url() {
        for pkg in PACKAGES {
            assert_eq!(pkg.sha256.len(), 64, "{}", pkg.id);
            assert!(pkg.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            // An fwlink always resolves to the newest build, which a pinned
            // checksum can never match.
            assert!(
                pkg.download_url
                    .starts_with("https://download.microsoft.com/download/"),
                "{}",
                pkg.download_url
            );
            assert!(pkg.license_url.starts_with("https://"), "{}", pkg.id);
        }
    }

    /// The ids share a namespace with `binaries::MANIFEST`, since one
    /// Install command serves both.
    #[test]
    fn no_package_id_collides_with_a_portable_one() {
        for pkg in PACKAGES {
            assert!(binaries::find(pkg.id).is_err(), "{} is taken", pkg.id);
        }
    }

    #[test]
    fn the_license_property_is_only_ever_set_to_yes_inside_the_script() {
        let pkg = find("msodbcsql").unwrap();
        let script = install_script(
            pkg,
            Path::new(r"C:\Users\Jane Doe\rezure\x.msi"),
            Path::new(r"C:\Users\Jane Doe\rezure\x.log"),
            Path::new(r"C:\Users\Jane Doe\rezure\x.exitcode"),
        );
        assert!(
            script.contains("'IACCEPTMSODBCSQLLICENSETERMS=YES'"),
            "{script}"
        );
        // A spaced path must reach msiexec as one argument.
        assert!(
            script.contains(r#"'"C:\Users\Jane Doe\rezure\x.msi"'"#),
            "{script}"
        );
        assert!(script.contains("/qn"), "{script}");
    }

    #[test]
    fn success_codes_are_not_errors_and_the_common_failures_are_explained() {
        let log = Path::new(r"C:\rezure\data\installers\x.log");
        assert_eq!(explain_exit_code(0, log), None);
        assert_eq!(explain_exit_code(3010, log), None);
        assert!(explain_exit_code(1618, log)
            .unwrap()
            .contains("another installation"));
        assert!(explain_exit_code(1603, log).unwrap().contains(r"x.log"));
    }

    /// Installs one package for real — the same path an Install click
    /// takes after its download — and **raises a UAC prompt** someone has
    /// to approve. Picks the package from `REZURE_TEST_MSI` so it can't run
    /// by accident. Run with, e.g.:
    /// `$env:REZURE_TEST_MSI='msodbcsql'; cargo test --lib services::msi::tests::installs_for_real -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn installs_for_real() {
        let id = std::env::var("REZURE_TEST_MSI").expect("set REZURE_TEST_MSI to a package id");
        let pkg = find(&id).expect("no such package");
        if (pkg.is_installed)() {
            println!("{} is already installed — nothing to do", pkg.name);
            return;
        }
        let bytes = reqwest::get(pkg.download_url)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        binaries::verify_checksum(pkg.id, pkg.sha256, &bytes).unwrap();
        tokio::task::spawn_blocking(move || run_installer(pkg, &bytes))
            .await
            .unwrap()
            .unwrap();
        assert!(
            (pkg.is_installed)(),
            "{} must be detected afterwards",
            pkg.name
        );
        println!("installed {} {}", pkg.name, pkg.version);
    }

    /// Downloads both installers for real and checks each against its pin
    /// and its signature — the only way to catch a moved file or a
    /// mistyped hash before a user's Install click does. Run with:
    /// `cargo test --lib services::msi::tests::every_pinned_installer_matches_its_download -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn every_pinned_installer_matches_its_download() {
        for pkg in PACKAGES {
            let bytes = reqwest::get(pkg.download_url)
                .await
                .and_then(|r| r.error_for_status())
                .unwrap_or_else(|e| panic!("{}: {e}", pkg.download_url))
                .bytes()
                .await
                .unwrap();
            binaries::verify_checksum(pkg.id, pkg.sha256, &bytes).unwrap();
            let path = std::env::temp_dir().join(format!("rezure-test-{}.msi", pkg.id));
            std::fs::write(&path, &bytes).unwrap();
            verify_signature(pkg, &path).unwrap();
            let _ = std::fs::remove_file(&path);
            println!(
                "{} {}: {} bytes, checksum and signature ok",
                pkg.id,
                pkg.version,
                bytes.len()
            );
        }
    }
}
