use thiserror::Error;

/// Central error type for saqlain-code.
#[derive(Debug, Error)]
#[allow(dead_code)] // Some variants are part of the public error surface for future call sites.
pub enum AgentError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("path '{0}' is outside of the workspace and is not allowed")]
    PathOutsideWorkspace(String),

    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("unknown tool: {0}")]
    UnknownTool(String),

    #[error("invalid tool arguments for '{tool}': {reason}")]
    InvalidToolArguments { tool: String, reason: String },

    #[error("edit failed: {0}")]
    EditFailed(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("command execution failed: {0}")]
    CommandFailed(String),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("maximum iterations reached")]
    MaxIterationsReached,

    #[error("operation cancelled by user")]
    Cancelled,

    #[error("configuration error: {0}")]
    Config(String),

    #[error("other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AgentError>;
