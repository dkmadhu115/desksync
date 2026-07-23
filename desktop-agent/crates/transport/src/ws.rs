//! WebSocket implementation of [`SignalingTransport`].
//!
//! Connects to the backend signaling service over (secure) WebSockets, passing
//! the short-lived signaling ticket as a query parameter, and exchanges
//! [`SignalEnvelope`] messages as JSON text frames. Reconnection and heartbeats
//! are driven by the agent runtime; this type owns only the socket and the
//! monotonic nonce. It uses `tokio-tungstenite` with rustls, so it has no
//! system-library dependencies and builds/tests on headless CI.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use async_trait::async_trait;
use desksync_core::error::{AgentError, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::{SignalEnvelope, SignalingTransport};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A WebSocket-backed signaling transport.
pub struct WebSocketSignaling {
    url: String,
    session_id: String,
    write: Mutex<Option<SplitSink<WsStream, Message>>>,
    read: Mutex<Option<SplitStream<WsStream>>>,
    connected: AtomicBool,
    nonce: AtomicU64,
}

impl WebSocketSignaling {
    /// Build a transport for a session. `base_url` is the signaling WebSocket
    /// URL from the session response (e.g. `wss://…/api/v1/signaling/ws`);
    /// `ticket` authorizes the upgrade and `role` is `"controller"` or
    /// `"agent"`.
    pub fn new(
        base_url: impl AsRef<str>,
        session_id: impl Into<String>,
        ticket: impl AsRef<str>,
        role: impl AsRef<str>,
    ) -> Self {
        let session_id = session_id.into();
        let sep = if base_url.as_ref().contains('?') { '&' } else { '?' };
        let url = format!(
            "{base}{sep}ticket={ticket}&session={session}&role={role}",
            base = base_url.as_ref(),
            ticket = ticket.as_ref(),
            session = session_id,
            role = role.as_ref(),
        );
        Self {
            url,
            session_id,
            write: Mutex::new(None),
            read: Mutex::new(None),
            connected: AtomicBool::new(false),
            nonce: AtomicU64::new(0),
        }
    }

    /// The session id this transport is bound to.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Return the next monotonic nonce for an outgoing envelope.
    pub fn next_nonce(&self) -> u64 {
        self.nonce.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn mark_disconnected(&self) {
        self.connected.store(false, Ordering::SeqCst);
    }
}

#[async_trait]
impl SignalingTransport for WebSocketSignaling {
    async fn connect(&self) -> Result<()> {
        let (stream, _resp) = connect_async(&self.url)
            .await
            .map_err(|e| AgentError::Transport(format!("signaling connect: {e}")))?;
        let (write, read) = stream.split();
        *self.write.lock().await = Some(write);
        *self.read.lock().await = Some(read);
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, envelope: SignalEnvelope) -> Result<()> {
        let text = serde_json::to_string(&envelope)?;
        let mut guard = self.write.lock().await;
        let sink = guard
            .as_mut()
            .ok_or_else(|| AgentError::Transport("send before connect".into()))?;
        sink.send(Message::text(text)).await.map_err(|e| {
            self.mark_disconnected();
            AgentError::Transport(format!("signaling send: {e}"))
        })
    }

    async fn recv(&self) -> Result<Option<SignalEnvelope>> {
        let mut guard = self.read.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| AgentError::Transport("recv before connect".into()))?;

        loop {
            match stream.next().await {
                None => {
                    self.mark_disconnected();
                    return Ok(None);
                }
                Some(Err(e)) => {
                    self.mark_disconnected();
                    return Err(AgentError::Transport(format!("signaling recv: {e}")));
                }
                Some(Ok(Message::Text(text))) => {
                    let env: SignalEnvelope = serde_json::from_str(text.as_str())?;
                    return Ok(Some(env));
                }
                Some(Ok(Message::Binary(bytes))) => {
                    let env: SignalEnvelope = serde_json::from_slice(&bytes)?;
                    return Ok(Some(env));
                }
                Some(Ok(Message::Close(_))) => {
                    self.mark_disconnected();
                    return Ok(None);
                }
                // Ping/Pong/Frame are handled by the library or ignored.
                Some(Ok(_)) => continue,
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SignalPayload;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    // A minimal in-process signaling stand-in: on connect it announces the peer
    // joined, then echoes the first message it receives (as a relay would).
    async fn spawn_echo_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(tcp).await.unwrap();

            let joined = SignalEnvelope::new(
                "sess-1",
                0,
                SignalPayload::PeerJoined {
                    role: "controller".into(),
                },
            );
            ws.send(Message::text(serde_json::to_string(&joined).unwrap()))
                .await
                .unwrap();

            if let Some(Ok(Message::Text(t))) = ws.next().await {
                ws.send(Message::text(t)).await.unwrap();
            }
        });

        format!("ws://{addr}/api/v1/signaling/ws")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_send_recv_roundtrip() {
        let base = spawn_echo_server().await;
        let t = WebSocketSignaling::new(base, "sess-1", "ticket-abc", "agent");

        t.connect().await.expect("connect");
        assert!(t.is_connected());

        // Server announces presence first.
        let first = t.recv().await.expect("recv").expect("some");
        assert!(matches!(first.payload, SignalPayload::PeerJoined { .. }));

        // Send an offer; the echo server relays it back.
        let nonce = t.next_nonce();
        assert_eq!(nonce, 1);
        t.send(SignalEnvelope::new(
            "sess-1",
            nonce,
            SignalPayload::Offer { sdp: "v=0".into() },
        ))
        .await
        .expect("send");

        let echoed = t.recv().await.expect("recv").expect("some");
        match echoed.payload {
            SignalPayload::Offer { sdp } => assert_eq!(sdp, "v=0"),
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn recv_returns_none_when_server_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let ws = accept_async(tcp).await.unwrap();
            drop(ws); // close immediately
        });

        let t = WebSocketSignaling::new(format!("ws://{addr}/ws"), "sess-1", "ticket", "agent");
        t.connect().await.expect("connect");
        // Either a clean close (None) or a connection-reset error is acceptable;
        // both leave the transport marked disconnected.
        let _ = t.recv().await;
        assert!(!t.is_connected());
    }

    #[tokio::test]
    async fn send_before_connect_errors() {
        let t = WebSocketSignaling::new("ws://127.0.0.1:1/ws", "s", "tk", "agent");
        let err = t
            .send(SignalEnvelope::new("s", 1, SignalPayload::Heartbeat))
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::Transport(_)));
    }
}
