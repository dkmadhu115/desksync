//! The wire contract: requests, responses, and the status they carry.
//!
//! Both sides depend only on this module, so the service and its clients can be
//! built and versioned independently. Adding a variant is backwards compatible;
//! changing the meaning of one requires bumping [`PROTOCOL_VERSION`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Version of this contract, echoed in every response so a mismatched client can
/// say something useful instead of failing to parse.
pub const PROTOCOL_VERSION: u32 = 1;

/// What a client asks the service to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    /// Liveness probe.
    Ping,
    /// Current service status: sign-in, device, capture settings, sessions.
    GetStatus,
    /// Where the service writes its logs, so a client can tail or bundle them
    /// without hard-coding platform paths.
    GetLogPath,
}

/// What the service answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum Response {
    /// Reply to [`Request::Ping`].
    Pong {
        /// The contract version the service speaks.
        protocol_version: u32,
    },
    /// Reply to [`Request::GetStatus`].
    Status(ServiceStatus),
    /// Reply to [`Request::GetLogPath`]. `None` when the service was started in
    /// the foreground and logs to the terminal.
    LogPath {
        /// Absolute path to the log file, if any.
        path: Option<String>,
    },
    /// The request could not be served.
    Error {
        /// Human-readable reason, safe to print.
        message: String,
    },
}

/// A snapshot of what the service is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatus {
    /// Agent version (crate version of the running binary).
    pub version: String,
    /// How long the service has been up.
    pub uptime_secs: u64,
    /// Whether usable credentials were found at startup.
    pub signed_in: bool,
    /// Backend-assigned device id, or the `unregistered` placeholder.
    pub device_id: String,
    /// REST base URL the service is talking to, so a client can tell which
    /// backend it is pointed at.
    pub api_url: String,
    /// Capture configuration and liveness.
    pub capture: CaptureStatus,
    /// Remote-control sessions currently being served.
    pub active_sessions: u32,
    /// The most recent error worth reporting, if any. This is the field that
    /// answers "it says offline, why?" without reading the log file.
    pub last_error: Option<String>,
}

/// Capture pipeline configuration and whether frames are actually being produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStatus {
    /// Configured frame rate target.
    pub target_fps: u32,
    /// Configured maximum frame height (downscale ceiling).
    pub max_height: u32,
    /// Whether at least one frame has been captured. False here with a running
    /// service is the signature of a missing screen-recording permission.
    pub producing_frames: bool,
}

/// Supplies the current status when a client asks for it.
///
/// Implemented by the service over its live runtime state; this indirection keeps
/// the IPC crate free of any dependency on agent internals.
#[async_trait]
pub trait StatusSource: Send + Sync {
    /// Snapshot the current status.
    async fn status(&self) -> ServiceStatus;

    /// Absolute path to the log file, when the service writes to one.
    fn log_path(&self) -> Option<String> {
        None
    }
}

/// A request wrapped with the caller's authentication token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// Per-install token proving the caller can read the owner-only token file.
    pub token: String,
    /// The request itself.
    #[serde(flatten)]
    pub request: Request,
}

/// Things that can go wrong talking over the local socket.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// The socket, token file, or config directory could not be used.
    #[error("ipc io error: {0}")]
    Io(#[from] std::io::Error),

    /// A message could not be encoded or decoded.
    #[error("ipc protocol error: {0}")]
    Protocol(String),

    /// Nothing is listening: the service is not running.
    #[error("the DeskSync service is not running")]
    NotRunning,

    /// The token was missing or wrong.
    #[error("ipc authentication failed")]
    Unauthorized,

    /// This platform has no IPC transport implementation yet.
    #[error("local IPC is not supported on this platform yet")]
    Unsupported,
}

/// Result alias for IPC operations.
pub type Result<T> = std::result::Result<T, IpcError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_status() -> ServiceStatus {
        ServiceStatus {
            version: "0.1.0".into(),
            uptime_secs: 42,
            signed_in: true,
            device_id: "device-1".into(),
            api_url: "http://localhost:8080".into(),
            capture: CaptureStatus {
                target_fps: 20,
                max_height: 720,
                producing_frames: true,
            },
            active_sessions: 1,
            last_error: None,
        }
    }

    #[test]
    fn requests_roundtrip_through_json() {
        for req in [Request::Ping, Request::GetStatus, Request::GetLogPath] {
            let encoded = serde_json::to_string(&req).unwrap();
            assert_eq!(serde_json::from_str::<Request>(&encoded).unwrap(), req);
        }
    }

    #[test]
    fn responses_roundtrip_through_json() {
        let responses = [
            Response::Pong {
                protocol_version: PROTOCOL_VERSION,
            },
            Response::Status(sample_status()),
            Response::LogPath {
                path: Some("/tmp/agent.log".into()),
            },
            Response::Error {
                message: "nope".into(),
            },
        ];
        for res in responses {
            let encoded = serde_json::to_string(&res).unwrap();
            assert_eq!(serde_json::from_str::<Response>(&encoded).unwrap(), res);
        }
    }

    #[test]
    fn envelope_carries_the_token_alongside_the_request() {
        // The request is flattened, so the wire form stays a flat object rather
        // than a nested one — easier to inspect by hand with `nc`.
        let envelope = Envelope {
            token: "secret".into(),
            request: Request::GetStatus,
        };
        let encoded = serde_json::to_string(&envelope).unwrap();
        assert_eq!(encoded, r#"{"token":"secret","request":"get_status"}"#);
        assert_eq!(serde_json::from_str::<Envelope>(&encoded).unwrap(), envelope);
    }

    #[test]
    fn messages_are_single_line_so_line_framing_is_safe() {
        // Framing is newline-delimited; a serialized message containing a raw
        // newline would silently truncate. serde_json's compact form never emits
        // one, and control characters in strings are escaped.
        let status = ServiceStatus {
            last_error: Some("line one\nline two".into()),
            ..sample_status()
        };
        let encoded = serde_json::to_string(&Response::Status(status)).unwrap();
        assert!(!encoded.contains('\n'), "encoded message must be one line: {encoded}");
    }
}
