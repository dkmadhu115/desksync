//! Browser-based sign-in for the desktop agent (OAuth "loopback" flow).
//!
//! The agent is a native app, so it cannot safely hold the Google client secret.
//! Instead the secret stays in the backend and the agent drives this flow:
//!
//! ```text
//!  agent                       browser                backend            Google
//!    │  bind 127.0.0.1:<port>                             │                 │
//!    │  open /auth/oauth/google/start?redirect_port&challenge ──────────────►│
//!    │                            │  consent screen ──────┼────────────────►│
//!    │                            │◄──────── redirect to backend callback ──┤
//!    │                            │◄─ 302 http://127.0.0.1:<port>/callback?code=…
//!    │◄─ one-time code ───────────┤                       │                 │
//!    │  POST /auth/oauth/desktop/exchange {code, verifier} ─────────────────►│
//!    │◄─ access + refresh tokens ─────────────────────────┤                 │
//! ```
//!
//! Security properties:
//! * The provider client secret never leaves the backend.
//! * PKCE binds redemption to this process: the agent sends only
//!   `S256(verifier)` when starting, and must present the `verifier` to redeem.
//!   A one-time code leaked from the loopback URL is therefore useless alone.
//! * The listener is bound to loopback only, and the backend hard-codes
//!   `127.0.0.1` in the redirect (it accepts only a port), so the result can
//!   never be redirected off-machine.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::client::BackendClient;
use crate::error::{BackendError, Result};
use crate::models::TokenPair;

/// How long to wait for the user to complete sign-in in the browser.
pub const DEFAULT_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// Guard against an unbounded read from whatever connects to our loopback port.
const MAX_REQUEST_LINE: usize = 8 * 1024;

/// A PKCE verifier and its derived S256 challenge (RFC 7636).
#[derive(Debug, Clone)]
pub struct PkcePair {
    /// High-entropy secret kept in memory and presented at redemption.
    pub verifier: String,
    /// `base64url(SHA-256(verifier))`, sent when starting the flow.
    pub challenge: String,
}

impl PkcePair {
    /// Generate a fresh pair from 32 bytes of OS entropy.
    pub fn generate() -> Result<Self> {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes)
            .map_err(|e| BackendError::Invalid(format!("failed to generate PKCE verifier: {e}")))?;
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let challenge = Self::challenge_for(&verifier);
        Ok(Self { verifier, challenge })
    }

    /// Derive the S256 challenge for a verifier.
    pub fn challenge_for(verifier: &str) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    }
}

/// Sign in with Google through the system browser and return the token pair.
pub async fn google_login(api_base: &str) -> Result<TokenPair> {
    login_with_provider(api_base, "google", DEFAULT_LOGIN_TIMEOUT).await
}

/// Sign in with any backend-configured provider through the system browser.
pub async fn login_with_provider(api_base: &str, provider: &str, timeout: Duration) -> Result<TokenPair> {
    let pkce = PkcePair::generate()?;

    // Bind before opening the browser so the port in the URL is already live.
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(transport)?;
    let port = listener.local_addr().map_err(transport)?.port();

    let url = start_url(api_base, provider, port, &pkce.challenge);
    if open_browser(&url) {
        println!("Opened your browser to finish signing in…");
    } else {
        println!("Could not open a browser automatically. Open this URL to sign in:\n\n{url}\n");
    }

    let code = wait_for_code(&listener, timeout).await?;
    BackendClient::new(api_base)?
        .exchange_desktop_code(&code, &pkce.verifier)
        .await
}

/// Build the backend URL that begins a native sign-in.
///
/// The challenge is unpadded base64url, so it needs no percent-encoding.
fn start_url(api_base: &str, provider: &str, port: u16, challenge: &str) -> String {
    format!(
        "{}/api/v1/auth/oauth/{provider}/start?redirect_port={port}&code_challenge={challenge}",
        api_base.trim_end_matches('/')
    )
}

/// Accept one loopback request, reply with a human-friendly page, and return the
/// one-time code the backend put in the query string.
async fn wait_for_code(listener: &TcpListener, timeout: Duration) -> Result<String> {
    let accepted = tokio::time::timeout(timeout, listener.accept())
        .await
        .map_err(|_| BackendError::Invalid("timed out waiting for the browser sign-in".into()))?;
    let (mut sock, _peer) = accepted.map_err(transport)?;

    let target = read_request_target(&mut sock).await?;
    let outcome = parse_callback(&target);

    // Always answer the browser so the user sees the result rather than a
    // connection error, even when the sign-in failed.
    let page = match &outcome {
        Ok(_) => page_html("Signed in", "You can close this window and return to DeskSync."),
        Err(e) => page_html("Sign-in failed", &e.to_string()),
    };
    let _ = write_response(&mut sock, &page).await;
    outcome
}

/// Read just the HTTP request line and return its target (e.g. `/callback?...`).
async fn read_request_target(sock: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    loop {
        let n = sock.read(&mut chunk).await.map_err(transport)?;
        if n == 0 {
            return Err(BackendError::Invalid(
                "the browser closed the connection before sending a request".into(),
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(2).position(|w| w == b"\r\n") {
            let line = String::from_utf8_lossy(&buf[..pos]).into_owned();
            return request_target(&line);
        }
        if buf.len() > MAX_REQUEST_LINE {
            return Err(BackendError::Invalid("callback request line was too long".into()));
        }
    }
}

/// Extract the target from a request line: `GET /callback?code=… HTTP/1.1`.
fn request_target(line: &str) -> Result<String> {
    line.split_whitespace()
        .nth(1)
        .map(str::to_string)
        .ok_or_else(|| BackendError::Invalid("malformed callback request".into()))
}

/// Pull `code` (or a backend-reported `error`) out of the callback target.
fn parse_callback(target: &str) -> Result<String> {
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = None;
    let mut error = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "code" => code = Some(percent_decode(value)),
            "error" => error = Some(percent_decode(value)),
            _ => {}
        }
    }
    if let Some(message) = error.filter(|m| !m.is_empty()) {
        return Err(BackendError::Invalid(message));
    }
    code.filter(|c| !c.is_empty())
        .ok_or_else(|| BackendError::Invalid("the sign-in callback did not include a code".into()))
}

/// Minimal `application/x-www-form-urlencoded` decoding for query values.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(decoded) => {
                    out.push(decoded);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn write_response(sock: &mut TcpStream, body: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(response.as_bytes()).await.map_err(transport)?;
    sock.flush().await.map_err(transport)
}

fn page_html(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>DeskSync</title></head>\
<body style=\"font-family:-apple-system,Segoe UI,Roboto,sans-serif;text-align:center;padding:64px 24px;color:#1b2437\">\
<h1 style=\"font-size:22px\">{}</h1><p style=\"color:#54617a\">{}</p></body></html>",
        escape_html(title),
        escape_html(message)
    )
}

/// Escape text interpolated into the response page. The message can contain a
/// backend-supplied error string, so it must never be trusted as markup.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Best-effort system-browser launch. Returns false if the platform opener could
/// not be started, in which case the caller prints the URL instead.
fn open_browser(url: &str) -> bool {
    use std::process::Command;
    let spawned = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        // The empty argument is `start`'s window-title placeholder.
        Command::new("cmd").args(["/C", "start", ""]).arg(url).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
    spawned.is_ok()
}

fn transport(e: std::io::Error) -> BackendError {
    BackendError::Http(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        // RFC 7636 Appendix B reference vector — must agree with the backend's
        // Go implementation for the exchange to succeed.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            PkcePair::challenge_for(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_pkce_is_high_entropy_and_self_consistent() {
        let a = PkcePair::generate().unwrap();
        let b = PkcePair::generate().unwrap();
        assert_ne!(a.verifier, b.verifier, "verifiers must be random");
        // 32 bytes → 43 unpadded base64url chars, satisfying the backend's
        // 43..=128 challenge length check.
        assert_eq!(a.challenge.len(), 43);
        assert_eq!(a.challenge, PkcePair::challenge_for(&a.verifier));
    }

    #[test]
    fn start_url_includes_port_and_challenge() {
        let url = start_url("http://api.example.com/", "google", 49152, "chal");
        assert_eq!(
            url,
            "http://api.example.com/api/v1/auth/oauth/google/start?redirect_port=49152&code_challenge=chal"
        );
    }

    #[test]
    fn request_target_extracted_from_request_line() {
        assert_eq!(
            request_target("GET /callback?code=abc HTTP/1.1").unwrap(),
            "/callback?code=abc"
        );
        assert!(request_target("garbage").is_err());
    }

    #[test]
    fn parse_callback_returns_code() {
        assert_eq!(parse_callback("/callback?code=abc123").unwrap(), "abc123");
        // Order and extra params must not matter.
        assert_eq!(parse_callback("/callback?state=x&code=abc123&y=1").unwrap(), "abc123");
    }

    #[test]
    fn parse_callback_surfaces_backend_error() {
        let err = parse_callback("/callback?error=sign-in+failed%3A+nope").unwrap_err();
        assert!(err.to_string().contains("sign-in failed: nope"), "got {err}");
    }

    #[test]
    fn parse_callback_rejects_missing_code() {
        assert!(parse_callback("/callback").is_err());
        assert!(parse_callback("/callback?code=").is_err());
    }

    #[test]
    fn percent_decoding_handles_escapes_and_plus() {
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("100%25"), "100%");
        // Malformed escapes are passed through rather than panicking.
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn page_html_escapes_untrusted_message() {
        let page = page_html("Sign-in failed", "<script>alert(1)</script>");
        assert!(!page.contains("<script>"));
        assert!(page.contains("&lt;script&gt;"));
    }

    #[tokio::test]
    async fn wait_for_code_reads_loopback_callback() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            sock.write_all(b"GET /callback?code=the-code HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            // Drain the response so the server's write cannot block.
            let mut sink = Vec::new();
            let _ = sock.read_to_end(&mut sink).await;
        });

        let code = wait_for_code(&listener, Duration::from_secs(5)).await.unwrap();
        assert_eq!(code, "the-code");
    }

    #[tokio::test]
    async fn wait_for_code_times_out_without_a_callback() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let err = wait_for_code(&listener, Duration::from_millis(50)).await.unwrap_err();
        assert!(err.to_string().contains("timed out"), "got {err}");
    }
}
