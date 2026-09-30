//! Asks `rezure-dashboard` whether a newer major line has shipped — see
//! `GET /version/upgrade` in `api-documentation/telemetry-api.md` (sibling
//! repo). The auto-updater only ever offers releases from this install's own
//! major line (3.x gets 3.x), so this is how a 3.x user hears that 4.0 is
//! out.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::api;
use crate::utils::error::AppError;

/// A newer major line, announced by a maintainer. Always a link to the
/// website: moving to a new major is the user's call, never an in-app update.
///
/// Every field is a single word, so the API's snake_case and the IPC
/// boundary's camelCase agree without a separate wire struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeNotice {
    pub major: u64,
    pub message: String,
    pub url: String,
}

/// The major of a `MAJOR.MINOR.PATCH` version string (`3` for `3.0.2`).
fn major_of(version: &str) -> Option<u64> {
    version.split('.').next()?.parse().ok()
}

/// The server already filters by major, but the banner is cheap to get
/// wrong in a visible way — a 4.x install told "v4 is out", or a button
/// that `open_external_link` would refuse — so both are checked here too.
fn worth_showing(notice: &UpgradeNotice, current_version: &str) -> bool {
    let is_newer = major_of(current_version).map_or(true, |current| notice.major > current);
    let is_web_link = notice.url.starts_with("https://") || notice.url.starts_with("http://");
    is_newer && is_web_link
}

/// Live fetch — 10s timeout, no auth, same "small, non-sensitive public
/// read" framing as the changelog. `Ok(None)` is the server's `204`: no
/// notice for this version.
async fn fetch_live(current_version: &str) -> Result<Option<UpgradeNotice>, AppError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Io(format!("could not set up the request: {e}")))?;
    let url = reqwest::Url::parse_with_params(
        &format!("{}/api/v1/version/upgrade", api::base_url()),
        &[("current_version", current_version)],
    )
    .map_err(|e| AppError::Io(format!("could not build the request URL: {e}")))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Io(e.to_string()))?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(AppError::Io(format!(
            "server returned {}",
            response.status()
        )));
    }
    let notice = response
        .json::<UpgradeNotice>()
        .await
        .map_err(|e| AppError::Io(format!("unexpected response: {e}")))?;

    Ok(Some(notice))
}

/// The notice for an install on `current_version`, if there is one.
///
/// Infallible on purpose and never cached: a notice that fails to load is
/// just no banner, and a cached "v4 is out" would outlive the maintainer
/// switching it off.
pub async fn fetch(current_version: &str) -> Option<UpgradeNotice> {
    match fetch_live(current_version).await {
        Ok(notice) => notice.filter(|notice| worth_showing(notice, current_version)),
        Err(err) => {
            log::warn!("could not fetch the upgrade notice: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(major: u64, url: &str) -> UpgradeNotice {
        UpgradeNotice {
            major,
            message: "Rezure 4 is out.".to_string(),
            url: url.to_string(),
        }
    }

    #[test]
    fn reads_the_major_off_a_version() {
        assert_eq!(major_of("3.0.2"), Some(3));
        assert_eq!(major_of("12.1.0"), Some(12));
        assert_eq!(major_of("v3.0.2"), None);
        assert_eq!(major_of(""), None);
    }

    #[test]
    fn shows_a_newer_major_to_an_older_install() {
        assert!(worth_showing(&notice(4, "https://rezure.test/v4"), "3.0.2"));
        assert!(worth_showing(&notice(5, "http://rezure.test/v5"), "3.0.2"));
    }

    #[test]
    fn hides_a_notice_for_the_installs_own_major_or_older() {
        assert!(!worth_showing(
            &notice(4, "https://rezure.test/v4"),
            "4.0.0"
        ));
        assert!(!worth_showing(
            &notice(3, "https://rezure.test/v3"),
            "4.1.0"
        ));
    }

    #[test]
    fn hides_a_notice_whose_link_the_browser_should_not_be_handed() {
        assert!(!worth_showing(&notice(4, "file:///C:/Windows"), "3.0.2"));
        assert!(!worth_showing(&notice(4, "javascript:alert(1)"), "3.0.2"));
    }

    #[test]
    fn decodes_the_wire_shape() {
        let body = r#"{"major":4,"message":"Rezure 4 is out.","url":"https://rezure.test/v4"}"#;
        let decoded: UpgradeNotice = serde_json::from_str(body).expect("valid notice");
        assert_eq!(decoded, notice(4, "https://rezure.test/v4"));
    }
}
