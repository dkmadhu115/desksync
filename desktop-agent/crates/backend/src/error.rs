//! Error type for the backend REST client.

use thiserror::Error;

/// Result alias for backend operations.
pub type Result<T> = std::result::Result<T, BackendError>;

/// Errors raised by the backend REST client.
#[derive(Debug, Error)]
pub enum BackendError {
    /// A transport-level failure (DNS, TLS, connection, timeout).
    #[error("http transport error: {0}")]
    Http(String),

    /// The backend returned a non-2xx status with (optionally) a uniform error
    /// envelope.
    #[error("backend returned {status} ({code}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Machine-readable error code from the envelope (or "unknown").
        code: String,
        /// Human-readable message.
        message: String,
    },

    /// A successful response body could not be decoded as expected.
    #[error("failed to decode response: {0}")]
    Decode(String),

    /// The client was asked to do something invalid (e.g. missing credentials).
    #[error("{0}")]
    Invalid(String),
}
