//! Error type for the developer-tools engine.

use thiserror::Error;

/// Result alias for devtools operations.
pub type Result<T> = std::result::Result<T, DevToolsError>;

/// Errors raised while validating or executing developer actions.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DevToolsError {
    /// The referenced workspace id is not registered.
    #[error("unknown workspace: {0}")]
    UnknownWorkspace(String),

    /// The referenced SSH host id is not registered.
    #[error("unknown ssh host: {0}")]
    UnknownHost(String),

    /// The requested shortcut is not in the catalog for the tool.
    #[error("unknown shortcut '{shortcut}' for {tool}")]
    UnknownShortcut {
        /// Tool name.
        tool: String,
        /// Shortcut id.
        shortcut: String,
    },

    /// The action cannot be satisfied on the current platform (e.g. Windows
    /// Terminal on macOS).
    #[error("action not supported on this platform: {0}")]
    Unsupported(String),

    /// A registry entry failed validation.
    #[error("invalid {kind}: {reason}")]
    Invalid {
        /// What was being validated ("workspace"/"ssh host").
        kind: &'static str,
        /// Why it failed.
        reason: String,
    },

    /// The command failed to spawn or run.
    #[error("execution failed: {0}")]
    Execution(String),
}

impl DevToolsError {
    /// Build an [`DevToolsError::Invalid`] error.
    pub fn invalid(kind: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            kind,
            reason: reason.into(),
        }
    }
}
