use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
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
}
