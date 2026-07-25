//! HTTP client for the DeskSync backend REST API.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::error::{BackendError, Result};
use crate::models::{Device, DeviceRegistration, PairingChallenge, PendingSession, PendingSessions, TokenPair};

/// The backend REST surface the agent needs to enroll and pair. Defined as a
/// trait so the enrollment orchestration can be unit-tested against a fake
/// without a network.
#[async_trait]
pub trait BackendApi: Send + Sync {
    /// Exchange email + password for a token pair.
    async fn login(&self, email: &str, password: &str) -> Result<TokenPair>;

    /// Rotate a refresh token for a fresh token pair.
    async fn refresh(&self, refresh_token: &str) -> Result<TokenPair>;

    /// Register (or idempotently re-register) this device; returns its record.
    async fn register_device(&self, access_token: &str, reg: &DeviceRegistration) -> Result<Device>;

    /// Initiate a pairing for one of the user's desktop devices.
    async fn initiate_pairing(&self, access_token: &str, desktop_device_id: &str) -> Result<PairingChallenge>;

    /// Report device presence and refresh its last-seen timestamp.
    async fn heartbeat(&self, access_token: &str, device_id: &str) -> Result<()>;

    /// List connecting sessions this desktop device should answer, each with a
    /// signaling ticket + ICE configuration.
    async fn pending_sessions(&self, access_token: &str, device_id: &str) -> Result<Vec<PendingSession>>;
}

/// A reqwest-backed [`BackendApi`] talking to the gateway base URL.
#[derive(Debug, Clone)]
pub struct BackendClient {
    http: reqwest::Client,
    base_url: String,
}

impl BackendClient {
    /// Build a client for the given gateway base URL (e.g.
    /// `https://api.desksync.example`). A trailing slash is trimmed.
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(|e| BackendError::Http(e.to_string()))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        })
    }

    /// Construct from an existing reqwest client (useful for custom timeouts).
    pub fn with_client(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

/// The uniform error envelope returned by the services.
#[derive(Debug, Deserialize, Default)]
struct ErrorEnvelope {
    #[serde(default)]
    error: String,
    #[serde(default)]
    message: String,
}

/// Send a request and decode a JSON body, mapping non-2xx responses to
/// [`BackendError::Api`] via the uniform error envelope.
async fn decode_json<T: DeserializeOwned>(req: reqwest::RequestBuilder) -> Result<T> {
    let resp = req.send().await.map_err(|e| BackendError::Http(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| BackendError::Http(e.to_string()))?;
    if status.is_success() {
        serde_json::from_str::<T>(&text).map_err(|e| BackendError::Decode(e.to_string()))
    } else {
        Err(api_error(status.as_u16(), &text))
    }
}

/// Send a request expecting no body, mapping non-2xx to [`BackendError::Api`].
async fn expect_success(req: reqwest::RequestBuilder) -> Result<()> {
    let resp = req.send().await.map_err(|e| BackendError::Http(e.to_string()))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let text = resp.text().await.unwrap_or_default();
    Err(api_error(status.as_u16(), &text))
}

fn api_error(status: u16, body: &str) -> BackendError {
    let env = serde_json::from_str::<ErrorEnvelope>(body).unwrap_or_default();
    let code = if env.error.is_empty() {
        "unknown".to_string()
    } else {
        env.error
    };
    let message = if env.message.is_empty() {
        body.to_string()
    } else {
        env.message
    };
    BackendError::Api { status, code, message }
}

#[async_trait]
impl BackendApi for BackendClient {
    async fn login(&self, email: &str, password: &str) -> Result<TokenPair> {
        let body = serde_json::json!({ "email": email, "password": password });
        decode_json(self.http.post(self.url("/api/v1/auth/login")).json(&body)).await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<TokenPair> {
        let body = serde_json::json!({ "refresh_token": refresh_token });
        decode_json(self.http.post(self.url("/api/v1/auth/refresh")).json(&body)).await
    }

    async fn register_device(&self, access_token: &str, reg: &DeviceRegistration) -> Result<Device> {
        decode_json(
            self.http
                .post(self.url("/api/v1/devices"))
                .bearer_auth(access_token)
                .json(reg),
        )
        .await
    }

    async fn initiate_pairing(&self, access_token: &str, desktop_device_id: &str) -> Result<PairingChallenge> {
        let body = serde_json::json!({ "desktop_device_id": desktop_device_id });
        decode_json(
            self.http
                .post(self.url("/api/v1/pairing/initiate"))
                .bearer_auth(access_token)
                .json(&body),
        )
        .await
    }

    async fn heartbeat(&self, access_token: &str, device_id: &str) -> Result<()> {
        let path = format!("/api/v1/devices/{device_id}/heartbeat");
        expect_success(
            self.http
                .post(self.url(&path))
                .bearer_auth(access_token)
                .json(&serde_json::json!({ "status": "online" })),
        )
        .await
    }

    async fn pending_sessions(&self, access_token: &str, device_id: &str) -> Result<Vec<PendingSession>> {
        let path = format!("/api/v1/sessions/pending?device_id={device_id}");
        let resp: PendingSessions =
            decode_json(self.http.get(self.url(&path)).bearer_auth(access_token)).await?;
        Ok(resp.sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    /// Start a one-shot HTTP server that captures a single request and replies
    /// with the given status line and JSON body. Returns the base URL and a
    /// handle resolving to the raw request text (for assertions).
    async fn serve_once(status_line: &'static str, body: &'static str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            let mut content_len: Option<usize> = None;
            let mut header_end: Option<usize> = None;
            loop {
                let n = sock.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = find(&buf, b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let head = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                        content_len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok());
                    }
                }
                if let (Some(end), cl) = (header_end, content_len.unwrap_or(0)) {
                    if buf.len() >= end + cl {
                        break;
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf).to_string();
            let resp = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            sock.flush().await.unwrap();
            req
        });
        (base, handle)
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    #[tokio::test]
    async fn login_parses_tokens_and_posts_credentials() {
        let (base, handle) = serve_once(
            "200 OK",
            r#"{"access_token":"a1","refresh_token":"r1","token_type":"Bearer","expires_in":900}"#,
        )
        .await;
        let client = BackendClient::new(base).unwrap();

        let tokens = client.login("dev@example.com", "secret").await.unwrap();
        assert_eq!(tokens.access_token, "a1");
        assert_eq!(tokens.refresh_token, "r1");

        let req = handle.await.unwrap();
        assert!(req.starts_with("POST /api/v1/auth/login"));
        assert!(req.contains("dev@example.com"));
    }

    #[tokio::test]
    async fn initiate_pairing_sends_bearer_and_parses_challenge() {
        let (base, handle) = serve_once(
            "201 Created",
            r#"{"pairing_id":"pid-1","qr_payload":"desksync://pair?v=1&pid=pid-1&code=12345678","manual_code":"12345678","expires_at":"2030-01-01T00:00:00Z"}"#,
        )
        .await;
        let client = BackendClient::new(base).unwrap();

        let challenge = client.initiate_pairing("token-xyz", "desk-1").await.unwrap();
        assert_eq!(challenge.pairing_id, "pid-1");
        assert_eq!(challenge.manual_code, "12345678");

        let req = handle.await.unwrap();
        assert!(req.contains("POST /api/v1/pairing/initiate"));
        assert!(req.to_lowercase().contains("authorization: bearer token-xyz"));
        assert!(req.contains("desk-1"));
    }

    #[tokio::test]
    async fn non_2xx_maps_to_api_error_with_envelope() {
        let (base, handle) = serve_once(
            "401 Unauthorized",
            r#"{"error":"unauthorized","message":"invalid or expired token"}"#,
        )
        .await;
        let client = BackendClient::new(base).unwrap();

        let err = client.initiate_pairing("bad", "desk-1").await.unwrap_err();
        match err {
            BackendError::Api { status, code, message } => {
                assert_eq!(status, 401);
                assert_eq!(code, "unauthorized");
                assert!(message.contains("invalid or expired"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
        let _ = handle.await;
    }
}
