//! Unified error type for the desktop agent.

use thiserror::Error;

/// Convenience result alias used throughout the agent.
pub type Result<T> = std::result::Result<T, AgentError>;

/// The unified error type surfaced by the agent's subsystems and runtime.
#[derive(Debug, Error)]
pub enum AgentError {
    /// Configuration was missing or invalid.
    #[error("configuration error: {0}")]
    Config(String),

    /// A subsystem (capture/input/transport) failed.
    #[error("subsystem '{name}' error: {message}")]
    Subsystem {
        /// Name of the failing subsystem.
        name: &'static str,
        /// Human-readable failure detail.
        message: String,
    },

    /// The transport/signaling layer failed (network, handshake, etc.).
    #[error("transport error: {0}")]
    Transport(String),

    /// An operation was attempted while offline; per spec, no actions execute.
    #[error("offline: operation refused until connectivity is restored")]
    Offline,

    /// A cryptographic operation (key generation, ECDH, encoding) failed.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Reading or writing a secret to the OS credential store (or its file
    /// fallback) failed.
    #[error("secret store error: {0}")]
    Secret(String),

    /// Serialization/deserialization of persisted state failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Wraps an underlying I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl AgentError {
    /// Build a subsystem error.
    pub fn subsystem(name: &'static str, message: impl Into<String>) -> Self {
        Self::Subsystem {
            name,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsystem_error_formats() {
        let e = AgentError::subsystem("capture", "device busy");
        assert!(e.to_string().contains("capture"));
        assert!(e.to_string().contains("device busy"));
    }

    #[test]
    fn offline_error_is_stable() {
        assert_eq!(
            AgentError::Offline.to_string(),
            "offline: operation refused until connectivity is restored"
        );
    }
}
