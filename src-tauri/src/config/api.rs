//! Base URL for the `rezure-dashboard` (`laravel-api`) telemetry/support API —
//! see `api-documentation/telemetry-api.md` (sibling repo) for the full
//! contract this app calls against.

/// `rezure-dashboard` isn't deployed publicly yet, so every build defaults to
/// the local dev server. Once it has a real domain, a release build sets
/// `REZURE_API_BASE_URL` at compile time to point at it — this is the one
/// spot that needs to change.
pub fn base_url() -> String {
    option_env!("REZURE_API_BASE_URL")
        .unwrap_or("https://api.redscale.my.id")
        .to_string()
}
