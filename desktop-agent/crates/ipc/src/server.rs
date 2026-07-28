//! The service side of the local IPC channel.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::protocol::{Envelope, IpcError, Request, Response, Result, StatusSource, PROTOCOL_VERSION};

/// File name of the socket inside the agent config directory.
const SOCKET_FILE: &str = "service.sock";
/// File name of the per-install auth token.
const TOKEN_FILE: &str = "service.token";

/// Path of the IPC socket for a given agent config directory.
pub fn socket_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SOCKET_FILE)
}

/// Path of the IPC auth token for a given agent config directory.
pub fn token_path(config_dir: &Path) -> PathBuf {
    config_dir.join(TOKEN_FILE)
}

/// A listening IPC server. Dropping it removes the socket file.
pub struct IpcServer {
    socket: PathBuf,
}

impl IpcServer {
    /// The socket this server is listening on.
    pub fn socket(&self) -> &Path {
        &self.socket
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // A stale socket file makes clients report "not running" only after a
        // connect attempt fails, so clean it up on the way out.
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Start serving IPC requests for `status` in `config_dir`, spawning a task per
/// connection.
///
/// Rebinding is deliberate: a socket file left behind by a crashed service would
/// otherwise make the address permanently unavailable.
#[cfg(unix)]
pub async fn listen(config_dir: &Path, status: Arc<dyn StatusSource>) -> Result<IpcServer> {
    use tokio::net::UnixListener;

    let socket = socket_path(config_dir);
    let token = ensure_token(config_dir).await?;

    // Remove a leftover socket, but only if nothing is listening on it — that
    // check is what stops a second service instance from stealing the channel.
    if socket.exists() {
        if tokio::net::UnixStream::connect(&socket).await.is_ok() {
            return Err(IpcError::Protocol(
                "another service instance is already listening".into(),
            ));
        }
        tokio::fs::remove_file(&socket).await?;
    }

    let listener = UnixListener::bind(&socket)?;
    // The config directory is typically group/world-traversable, so narrow the
    // socket itself: the token is defence in depth, not the only barrier.
    set_owner_only(&socket)?;
    tracing::info!(socket = %socket.display(), "service ipc listening");

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let status = Arc::clone(&status);
                    let token = token.clone();
                    tokio::spawn(async move {
                        if let Err(e) = serve_connection(stream, status, token).await {
                            tracing::debug!(error = %e, "ipc connection ended");
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "ipc accept failed");
                    return;
                }
            }
        }
    });

    Ok(IpcServer { socket })
}

#[cfg(not(unix))]
pub async fn listen(_config_dir: &Path, _status: Arc<dyn StatusSource>) -> Result<IpcServer> {
    Err(IpcError::Unsupported)
}

/// Handle requests on one connection until the peer closes it.
#[cfg(unix)]
async fn serve_connection(stream: tokio::net::UnixStream, status: Arc<dyn StatusSource>, token: String) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Envelope>(&line) {
            Ok(envelope) if !crate::client::tokens_match(&envelope.token, &token) => {
                tracing::warn!("rejected ipc request with an invalid token");
                Response::Error {
                    message: "authentication failed".into(),
                }
            }
            Ok(envelope) => handle(envelope.request, status.as_ref()).await,
            Err(e) => Response::Error {
                message: format!("malformed request: {e}"),
            },
        };

        let mut encoded =
            serde_json::to_vec(&response).map_err(|e| IpcError::Protocol(format!("encoding response: {e}")))?;
        encoded.push(b'\n');
        write_half.write_all(&encoded).await?;
        write_half.flush().await?;
    }
    Ok(())
}

/// Produce the response for one request.
async fn handle(request: Request, status: &dyn StatusSource) -> Response {
    match request {
        Request::Ping => Response::Pong {
            protocol_version: PROTOCOL_VERSION,
        },
        Request::GetStatus => Response::Status(status.status().await),
        Request::GetLogPath => Response::LogPath {
            path: status.log_path(),
        },
    }
}

/// Read the per-install token, creating an owner-only one on first run.
#[cfg(unix)]
async fn ensure_token(config_dir: &Path) -> Result<String> {
    let path = token_path(config_dir);
    if let Ok(existing) = tokio::fs::read_to_string(&path).await {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return Ok(existing);
        }
    }

    let token = random_token();
    tokio::fs::write(&path, &token).await?;
    // Written before narrowing permissions, so narrow immediately: the token is
    // the one secret in this directory that a client is expected to read.
    set_owner_only(&path)?;
    Ok(token)
}

/// Restrict a file to the owning user (0600).
#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

/// A 256-bit random token, hex encoded.
#[cfg(unix)]
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("OS entropy is unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::protocol::{CaptureStatus, ServiceStatus};
    use async_trait::async_trait;
    use tempfile::tempdir;

    struct FakeStatus;

    #[async_trait]
    impl StatusSource for FakeStatus {
        async fn status(&self) -> ServiceStatus {
            ServiceStatus {
                version: "0.1.0".into(),
                uptime_secs: 7,
                signed_in: true,
                device_id: "device-1".into(),
                api_url: "http://localhost:8080".into(),
                capture: CaptureStatus {
                    target_fps: 20,
                    max_height: 720,
                    producing_frames: false,
                },
                signing_in: false,
                active_sessions: 2,
                permissions: Vec::new(),
                last_error: Some("heartbeat failed".into()),
            }
        }

        fn log_path(&self) -> Option<String> {
            Some("/tmp/desksync.log".into())
        }
    }

    #[tokio::test]
    async fn client_can_query_status_over_the_socket() {
        let dir = tempdir().unwrap();
        let _server = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();

        let response = crate::client::request(dir.path(), Request::GetStatus).await.unwrap();
        match response {
            Response::Status(status) => {
                assert_eq!(status.device_id, "device-1");
                assert_eq!(status.active_sessions, 2);
                assert_eq!(status.last_error.as_deref(), Some("heartbeat failed"));
            }
            other => panic!("expected a status response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ping_reports_the_protocol_version() {
        let dir = tempdir().unwrap();
        let _server = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();

        let response = crate::client::request(dir.path(), Request::Ping).await.unwrap();
        assert_eq!(
            response,
            Response::Pong {
                protocol_version: PROTOCOL_VERSION
            }
        );
    }

    #[tokio::test]
    async fn log_path_is_reported_so_clients_need_no_platform_paths() {
        let dir = tempdir().unwrap();
        let _server = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();

        let response = crate::client::request(dir.path(), Request::GetLogPath).await.unwrap();
        assert_eq!(
            response,
            Response::LogPath {
                path: Some("/tmp/desksync.log".into())
            }
        );
    }

    #[tokio::test]
    async fn a_wrong_token_is_rejected() {
        let dir = tempdir().unwrap();
        let _server = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();
        // Overwrite the token the client will read, simulating a caller that
        // guessed the socket path but cannot read the real token.
        tokio::fs::write(token_path(dir.path()), "not-the-token").await.unwrap();

        let response = crate::client::request(dir.path(), Request::GetStatus).await.unwrap();
        match response {
            Response::Error { message } => assert!(message.contains("authentication")),
            other => panic!("expected an auth error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_token_and_socket_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let _server = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();

        let token_mode = std::fs::metadata(token_path(dir.path())).unwrap().permissions().mode();
        assert_eq!(token_mode & 0o777, 0o600, "token must not be readable by other users");

        let socket_mode = std::fs::metadata(socket_path(dir.path())).unwrap().permissions().mode();
        assert_eq!(
            socket_mode & 0o777,
            0o600,
            "another local user must not be able to connect"
        );
    }

    #[tokio::test]
    async fn a_stale_socket_file_does_not_block_startup() {
        // Simulates a crashed service: the socket file exists but nothing is
        // listening. Binding must reclaim it rather than fail forever.
        let dir = tempdir().unwrap();
        std::fs::write(socket_path(dir.path()), b"").unwrap();

        let _server = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();
        assert!(crate::client::request(dir.path(), Request::Ping).await.is_ok());
    }

    #[tokio::test]
    async fn a_second_listener_is_refused() {
        let dir = tempdir().unwrap();
        let _first = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();

        let second = listen(dir.path(), Arc::new(FakeStatus)).await;
        assert!(second.is_err(), "the channel must not be stolen from a live service");
    }

    #[tokio::test]
    async fn dropping_the_server_removes_the_socket() {
        let dir = tempdir().unwrap();
        {
            let _server = listen(dir.path(), Arc::new(FakeStatus)).await.unwrap();
            assert!(socket_path(dir.path()).exists());
        }
        assert!(!socket_path(dir.path()).exists());
    }
}
