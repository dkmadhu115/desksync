//! The client side of the local IPC channel, used by the CLI and the UI.

use std::path::Path;

use subtle::ConstantTimeEq;

use crate::protocol::{Envelope, IpcError, Request, Response, Result};
use crate::server::{socket_path, token_path};

/// Send one request to the service and return its response.
///
/// Returns [`IpcError::NotRunning`] when there is nothing to talk to, which is
/// the common case (service not installed or stopped) and deserves a clear
/// message rather than a raw connection error.
#[cfg(unix)]
pub async fn request(config_dir: &Path, request: Request) -> Result<Response> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let socket = socket_path(config_dir);
    let token = tokio::fs::read_to_string(token_path(config_dir))
        .await
        .map_err(|_| IpcError::NotRunning)?
        .trim()
        .to_string();

    let stream = match UnixStream::connect(&socket).await {
        Ok(s) => s,
        // A refused connection or missing socket both mean "no service", not a
        // failure the caller can act on differently.
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(IpcError::NotRunning)
        }
        Err(e) => return Err(IpcError::Io(e)),
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut encoded = serde_json::to_vec(&Envelope { token, request })
        .map_err(|e| IpcError::Protocol(format!("encoding request: {e}")))?;
    encoded.push(b'\n');
    write_half.write_all(&encoded).await?;
    write_half.flush().await?;

    let mut line = String::new();
    let mut reader = BufReader::new(read_half);
    if reader.read_line(&mut line).await? == 0 {
        return Err(IpcError::Protocol(
            "service closed the connection without replying".into(),
        ));
    }
    serde_json::from_str(&line).map_err(|e| IpcError::Protocol(format!("decoding response: {e}")))
}

#[cfg(not(unix))]
pub async fn request(_config_dir: &Path, _request: Request) -> Result<Response> {
    Err(IpcError::Unsupported)
}

/// Compare two IPC tokens without leaking their contents through timing.
pub(crate) fn tokens_match(provided: &str, expected: &str) -> bool {
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn a_missing_service_is_reported_as_not_running() {
        let dir = tempdir().unwrap();
        let err = request(dir.path(), Request::Ping).await.unwrap_err();
        assert!(matches!(err, IpcError::NotRunning), "got: {err}");
        // The message is user-facing, so it must read as an explanation.
        assert!(err.to_string().contains("not running"));
    }

    #[test]
    fn token_comparison_accepts_only_an_exact_match() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abc", "abd"));
        assert!(!tokens_match("abc", "abcd"), "a prefix must not be accepted");
        assert!(!tokens_match("", "abc"));
    }
}
