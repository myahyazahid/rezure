use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("service not found: {0}")]
    ServiceNotFound(String),

    #[error("php version not installed: {0}")]
    PhpVersionNotFound(String),

    #[error("unknown binary package: {0}")]
    UnknownBinary(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error("checksum mismatch for {id}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        id: String,
        expected: String,
        actual: String,
    },

    #[error("failed to extract archive: {0}")]
    Extract(String),

    #[error("io error: {0}")]
    Io(String),

    #[error("{0} isn't installed yet — download it from the Binaries panel first")]
    BinaryNotInstalled(String),

    #[error("failed to start {name}: {reason}")]
    ProcessSpawnFailed { name: String, reason: String },

    #[error("failed to prepare {name}'s data directory: {reason}")]
    ProcessBootstrapFailed { name: String, reason: String },

    #[error("port {port} is already in use — stop whatever's using it before starting {name}")]
    PortInUse { port: u16, name: String },

    #[error(
        "hosts file update was cancelled — click Yes on the admin prompt to let project domains resolve in your browser"
    )]
    HostsUpdateCancelled,

    #[error("failed to update the hosts file: {0}")]
    HostsUpdateFailed(String),
}

/// Serialized as its `Display` message (the `#[error("...")]` text) rather
/// than its structural data, so a rejected `invoke()` on the frontend gets
/// a message it can show directly instead of `{ "PortInUse": { ... } }`.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
