use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("service not found: {0}")]
    ServiceNotFound(String),

    #[error("php version not installed: {0}")]
    PhpVersionNotFound(String),

    #[error("PHP {0} is already installed")]
    PhpVersionAlreadyInstalled(String),

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

    #[error("unknown project template: {0}")]
    UnknownTemplate(String),

    #[error(
        "\"{0}\" isn't a valid project name — use lowercase letters, digits, and hyphens only"
    )]
    InvalidProjectName(String),

    #[error("a project named \"{0}\" already exists")]
    ProjectAlreadyExists(String),

    #[error("failed to create the project: {0}")]
    ScaffoldFailed(String),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("couldn't open {target}: {reason}")]
    OpenFailed { target: String, reason: String },

    #[error(
        "\"{name}\" isn't a valid {kind} name — use letters, digits, underscores and hyphens only"
    )]
    InvalidDatabaseName { name: String, kind: String },

    #[error("MariaDB: {0}")]
    DatabaseQueryFailed(String),

    #[error("no such SQL client: {0}")]
    UnknownDbClient(String),

    #[error("settings error: {0}")]
    Settings(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("database profile not found: {0}")]
    ProfileNotFound(String),

    #[error("that profile can't be removed — {0}")]
    ProfileUndeletable(String),

    #[error("{path} is already registered as the \"{name}\" profile")]
    DatadirAlreadyRegistered { path: String, name: String },

    #[error(
        "{path} doesn't look like a {engine} data directory — pick the folder that holds ibdata1"
    )]
    NotADatadir { path: String, engine: String },

    #[error(
        "no {engine} {version} binary is installed — add one before switching to this profile"
    )]
    EngineBinaryMissing { engine: String, version: String },

    #[error(
        "this datadir was written by {found}, but the profile says {expected} — opening it with the wrong engine can corrupt it"
    )]
    EngineMismatch { found: String, expected: String },

    #[error(
        "{name}'s database server looks like it's still running against this data directory — stop it there first"
    )]
    DatadirInUse { name: String },

    #[error("switched back to \"{restored}\" — {reason}")]
    SwitchRolledBack { restored: String, reason: String },

    #[error("{reason}")]
    PortHolderProtected { port: u16, reason: String },

    #[error("{path} can't be added as a project — {reason}")]
    UnusableProjectPath { path: String, reason: String },

    #[error("{path} is already a project here (\"{name}\")")]
    ProjectAlreadyLinked { path: String, name: String },

    #[error("can't start {name} — {holder}")]
    PortInUseBy {
        port: u16,
        name: String,
        holder: String,
    },

    #[error("{path} can't be attached — {reason}")]
    AttachmentRejected { path: String, reason: String },

    #[error("couldn't send the ticket: {0}")]
    TicketSubmitFailed(String),

    #[error("couldn't load your ticket history: {0}")]
    TicketHistoryFailed(String),
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
