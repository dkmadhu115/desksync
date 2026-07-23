//! Signaling and WebRTC transport abstraction.
//!
//! The agent connects to the backend signaling service over secure WebSockets
//! to exchange SDP offers/answers and ICE candidates, then establishes a
//! WebRTC peer connection (with TURN relay fallback) for media and data
//! channels. Phase 1 defines the signaling message envelope and the
//! [`SignalingTransport`] trait; the WebRTC implementation lands in Phase 5.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use async_trait::async_trait;
use desksync_core::error::Result;
use serde::{Deserialize, Serialize};

/// The signaling message envelope exchanged over the WebSocket. It mirrors the
/// backend protocol documented in `docs/design/api.md`. Every message carries a
/// monotonic nonce and timestamp for replay protection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalEnvelope {
    /// Protocol version.
    pub v: u8,
    /// Monotonic per-connection nonce (replay protection).
    pub nonce: u64,
    /// Unix epoch milliseconds when the message was created.
    pub ts_ms: u64,
    /// Identifier of the paired session this message belongs to.
    pub session_id: String,
    /// The message payload.
    pub payload: SignalPayload,
}

/// Signaling payload variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignalPayload {
    /// SDP offer from the initiating peer.
    Offer {
        /// SDP string.
        sdp: String,
    },
    /// SDP answer from the responding peer.
    Answer {
        /// SDP string.
        sdp: String,
    },
    /// A single trickled ICE candidate.
    IceCandidate {
        /// Candidate line.
        candidate: String,
        /// Media line index.
        sdp_m_line_index: u16,
    },
    /// Keep-alive heartbeat.
    Heartbeat,
    /// Peer requested the session be torn down.
    Bye,
}

/// Backend-agnostic signaling transport. Implementations manage the WebSocket
/// connection and reconnection; per spec, while offline no messages are sent
/// and pending actions are refused.
#[async_trait]
pub trait SignalingTransport: Send + Sync {
    /// Connect (or reconnect) to the signaling backend.
    async fn connect(&self) -> Result<()>;

    /// Send an envelope to the peer via the backend.
    async fn send(&self, envelope: SignalEnvelope) -> Result<()>;

    /// Receive the next envelope, or `None` when the connection is closed.
    async fn recv(&self) -> Result<Option<SignalEnvelope>>;

    /// Whether the transport currently has a live connection.
    fn is_connected(&self) -> bool;
}

/// Validates replay/ordering constraints on an incoming stream of envelopes.
/// The real transport uses this to reject stale or replayed messages.
#[derive(Debug, Default)]
pub struct ReplayGuard {
    last_nonce: Option<u64>,
}

impl ReplayGuard {
    /// Create a new guard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept an envelope only if its nonce strictly increases. Returns true
    /// when accepted, false when the message is a replay or out of order.
    pub fn accept(&mut self, nonce: u64) -> bool {
        match self.last_nonce {
            Some(last) if nonce <= last => false,
            _ => {
                self.last_nonce = Some(nonce);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_guard_rejects_non_increasing_nonces() {
        let mut g = ReplayGuard::new();
        assert!(g.accept(1));
        assert!(g.accept(2));
        assert!(!g.accept(2)); // replay
        assert!(!g.accept(1)); // out of order
        assert!(g.accept(3));
    }

    #[test]
    fn envelope_roundtrips_json() {
        let env = SignalEnvelope {
            v: 1,
            nonce: 42,
            ts_ms: 1_700_000_000_000,
            session_id: "sess-1".into(),
            payload: SignalPayload::IceCandidate {
                candidate: "candidate:1 1 udp 2130706431 10.0.0.1 54321 typ host".into(),
                sdp_m_line_index: 0,
            },
        };
        let s = serde_json::to_string(&env).unwrap();
        let back: SignalEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn heartbeat_serializes_with_kind_tag() {
        let env = SignalEnvelope {
            v: 1,
            nonce: 1,
            ts_ms: 0,
            session_id: "s".into(),
            payload: SignalPayload::Heartbeat,
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(s.contains("\"kind\":\"heartbeat\""));
    }
}
