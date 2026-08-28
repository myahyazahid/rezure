use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
pub enum AppError {
    #[error("service not found: {0}")]
    ServiceNotFound(String),
}
