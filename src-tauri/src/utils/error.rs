use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("service not found: {0}")]
    ServiceNotFound(String),

    #[error("php version not installed: {0}")]
    PhpVersionNotFound(String),

    #[error("node version not installed: {0}")]
    NodeVersionNotFound(String),

    #[error("no {runtime} version {version} in the catalog")]
    CatalogVersionNotFound { runtime: String, version: String },

    #[error("PHP {0} is already installed")]
    PhpVersionAlreadyInstalled(String),

    #[error("unknown binary package: {0}")]
    UnknownBinary(String),

    #[error("{id} isn't available for PHP {php_version} yet")]
    ExtensionUnavailable { id: String, php_version: String },

    #[error("unknown PHP extension: {0}")]
    UnknownExtension(String),

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

    #[error("{0} isn't running — start it first")]
    ServiceNotRunning(String),

    #[error("{0} has no web interface to open")]
    NoWebUi(String),

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

    // No engine name in the prefix: the same path now also talks to remote
    // MySQL servers, and labelling their errors "MariaDB" sent people
    // looking in the wrong place.
    #[error("{0}")]
    DatabaseQueryFailed(String),

    #[error("SSH tunnel failed: {0}")]
    TunnelFailed(String),

    #[error("export of \"{0}\" was cancelled")]
    ExportCancelled(String),

    #[error("couldn't share this project: {0}")]
    ShareFailed(String),

    /// Browsing, downloading or removing a sticker. The reason is written for
    /// the person reading it ("that file isn't the sticker the catalog
    /// listed"), not copied from a transport error.
    #[error("{0}")]
    StickerFailed(String),

    #[error("can't reach {host}:{port} — {reason}")]
    ServerUnreachable {
        host: String,
        port: u16,
        reason: String,
    },

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

    #[error("connection not found: {0}")]
    ConnectionNotFound(String),

    #[error("{endpoint} is already saved as the \"{name}\" connection")]
    ConnectionAlreadyExists { endpoint: String, name: String },

    #[error(
        "\"{name}\" is read-only — turn that off in the connection's settings if you really mean to write to it"
    )]
    ConnectionReadOnly { name: String },

    #[error("{0}")]
    InvalidConnection(String),

    #[error("Windows Credential Manager refused the password: {0}")]
    CredentialStore(String),

    #[error("couldn't send the ticket: {0}")]
    TicketSubmitFailed(String),

    #[error("couldn't load your ticket history: {0}")]
    TicketHistoryFailed(String),

    #[error(
        "{0} is installed under Microsoft's license — accept it first to let Rezure install it"
    )]
    LicenseNotAccepted(String),

    #[error(
        "installing {0} was cancelled — click Yes on the admin prompt to let Windows install it"
    )]
    InstallerCancelled(String),

    #[error("{name} didn't install: {reason}")]
    InstallerFailed { name: String, reason: String },

    #[error(
        "the Microsoft ODBC Driver for SQL Server isn't installed — install it from PHP Extensions (or the Databases page) first"
    )]
    OdbcDriverMissing,

    #[error("{0}")]
    UnsupportedOnSqlServer(String),

    #[error("PostgreSQL version not installed: {0}")]
    PostgresVersionNotFound(String),

    #[error(
        "PostgreSQL refuses to run with administrator rights, and Rezure was started as administrator — close it and open it normally, then start PostgreSQL again"
    )]
    PostgresElevated,
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

impl AppError {
    /// The kind of failure, as a stable identifier (the variant's name) —
    /// what telemetry's `error.report` events carry. Never the message:
    /// messages hold paths, project names and hostnames from the user's
    /// machine, and the dashboard groups errors by this exact string, so it
    /// mustn't vary with them either.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ServiceNotFound { .. } => "ServiceNotFound",
            Self::PhpVersionNotFound { .. } => "PhpVersionNotFound",
            Self::NodeVersionNotFound { .. } => "NodeVersionNotFound",
            Self::CatalogVersionNotFound { .. } => "CatalogVersionNotFound",
            Self::PhpVersionAlreadyInstalled { .. } => "PhpVersionAlreadyInstalled",
            Self::UnknownBinary { .. } => "UnknownBinary",
            Self::ExtensionUnavailable { .. } => "ExtensionUnavailable",
            Self::UnknownExtension { .. } => "UnknownExtension",
            Self::Download { .. } => "Download",
            Self::ChecksumMismatch { .. } => "ChecksumMismatch",
            Self::Extract { .. } => "Extract",
            Self::Io { .. } => "Io",
            Self::BinaryNotInstalled { .. } => "BinaryNotInstalled",
            Self::ServiceNotRunning { .. } => "ServiceNotRunning",
            Self::NoWebUi { .. } => "NoWebUi",
            Self::ProcessSpawnFailed { .. } => "ProcessSpawnFailed",
            Self::ProcessBootstrapFailed { .. } => "ProcessBootstrapFailed",
            Self::PortInUse { .. } => "PortInUse",
            Self::HostsUpdateCancelled => "HostsUpdateCancelled",
            Self::HostsUpdateFailed { .. } => "HostsUpdateFailed",
            Self::UnknownTemplate { .. } => "UnknownTemplate",
            Self::InvalidProjectName { .. } => "InvalidProjectName",
            Self::ProjectAlreadyExists { .. } => "ProjectAlreadyExists",
            Self::ScaffoldFailed { .. } => "ScaffoldFailed",
            Self::ProjectNotFound { .. } => "ProjectNotFound",
            Self::OpenFailed { .. } => "OpenFailed",
            Self::InvalidDatabaseName { .. } => "InvalidDatabaseName",
            Self::DatabaseQueryFailed { .. } => "DatabaseQueryFailed",
            Self::TunnelFailed { .. } => "TunnelFailed",
            Self::ExportCancelled { .. } => "ExportCancelled",
            Self::ShareFailed { .. } => "ShareFailed",
            Self::StickerFailed { .. } => "StickerFailed",
            Self::ServerUnreachable { .. } => "ServerUnreachable",
            Self::UnknownDbClient { .. } => "UnknownDbClient",
            Self::Settings { .. } => "Settings",
            Self::Database { .. } => "Database",
            Self::ProfileNotFound { .. } => "ProfileNotFound",
            Self::ProfileUndeletable { .. } => "ProfileUndeletable",
            Self::DatadirAlreadyRegistered { .. } => "DatadirAlreadyRegistered",
            Self::NotADatadir { .. } => "NotADatadir",
            Self::EngineBinaryMissing { .. } => "EngineBinaryMissing",
            Self::EngineMismatch { .. } => "EngineMismatch",
            Self::DatadirInUse { .. } => "DatadirInUse",
            Self::SwitchRolledBack { .. } => "SwitchRolledBack",
            Self::PortHolderProtected { .. } => "PortHolderProtected",
            Self::UnusableProjectPath { .. } => "UnusableProjectPath",
            Self::ProjectAlreadyLinked { .. } => "ProjectAlreadyLinked",
            Self::PortInUseBy { .. } => "PortInUseBy",
            Self::AttachmentRejected { .. } => "AttachmentRejected",
            Self::ConnectionNotFound { .. } => "ConnectionNotFound",
            Self::ConnectionAlreadyExists { .. } => "ConnectionAlreadyExists",
            Self::ConnectionReadOnly { .. } => "ConnectionReadOnly",
            Self::InvalidConnection { .. } => "InvalidConnection",
            Self::CredentialStore { .. } => "CredentialStore",
            Self::TicketSubmitFailed { .. } => "TicketSubmitFailed",
            Self::TicketHistoryFailed { .. } => "TicketHistoryFailed",
            Self::LicenseNotAccepted { .. } => "LicenseNotAccepted",
            Self::InstallerCancelled { .. } => "InstallerCancelled",
            Self::InstallerFailed { .. } => "InstallerFailed",
            Self::OdbcDriverMissing => "OdbcDriverMissing",
            Self::UnsupportedOnSqlServer { .. } => "UnsupportedOnSqlServer",
            Self::PostgresVersionNotFound { .. } => "PostgresVersionNotFound",
            Self::PostgresElevated => "PostgresElevated",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_the_variant_name_and_never_the_message() {
        let err = AppError::PortInUse {
            port: 80,
            name: r"C:\Users\someone\secret-project".to_string(),
        };
        assert_eq!(err.code(), "PortInUse");
        assert_eq!(AppError::Io(r"C:\Users\someone".to_string()).code(), "Io");
        assert_eq!(
            AppError::HostsUpdateCancelled.code(),
            "HostsUpdateCancelled"
        );
    }
}
