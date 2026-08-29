use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
pub enum AppError {
    #[error("service not found: {0}")]
    ServiceNotFound(String),

    #[error("php version not installed: {0}")]
    PhpVersionNotFound(String),
}
