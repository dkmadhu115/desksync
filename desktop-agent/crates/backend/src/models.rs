//! Wire models mirroring the backend OpenAPI contract (auth, devices, pairing).

use serde::{Deserialize, Serialize};

/// An access + refresh token pair issued by the auth service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    /// Short-lived bearer access token.
    pub access_token: String,
    /// Long-lived refresh token (rotated on each refresh).
    pub refresh_token: String,
    /// Token type, always "Bearer".
    #[serde(default)]
    pub token_type: String,
    /// Access-token lifetime in seconds.
    #[serde(default)]
    pub expires_in: i64,
}

/// Request body for `POST /api/v1/devices`.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceRegistration {
    /// "desktop" or "mobile".
    pub kind: String,
    /// OS platform (windows/macos/linux/android/ios).
    pub platform: String,
    /// Human-friendly device name.
    pub name: String,
    /// Base64-encoded 32-byte X25519 public key.
    pub public_key: String,
    /// Optional mobile push token (unused for desktop agents).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fcm_token: Option<String>,
}

/// A registered device as returned by the device service.
#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    /// Server-assigned device id (UUID).
    pub id: String,
    /// "desktop" or "mobile".
    #[serde(default)]
    pub kind: String,
    /// OS platform string.
    #[serde(default)]
    pub platform: String,
    /// Device name.
    #[serde(default)]
    pub name: String,
    /// Presence ("online"/"offline").
    #[serde(default)]
    pub status: String,
}

/// A single ICE server (STUN/TURN) from the session response.
#[derive(Debug, Clone, Deserialize)]
pub struct IceServer {
    /// STUN/TURN URLs.
    #[serde(default)]
    pub urls: Vec<String>,
    /// TURN username (empty for STUN).
    #[serde(default)]
    pub username: String,
    /// TURN credential (empty for STUN).
    #[serde(default)]
    pub credential: String,
}

/// Minimal session identity embedded in the pending-session response.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionRef {
    /// Session id (UUID).
    pub id: String,
    /// Owning pairing id.
    #[serde(default)]
    pub pairing_id: String,
    /// Session status ("connecting", …).
    #[serde(default)]
    pub status: String,
}

/// A pending session the agent should answer, with everything needed to join:
/// the signaling URL + short-lived agent ticket and the ICE configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PendingSession {
    /// The session identity.
    pub session: SessionRef,
    /// WebSocket signaling URL.
    #[serde(default)]
    pub signaling_url: String,
    /// Short-lived signaling ticket (agent role).
    #[serde(default)]
    pub signaling_ticket: String,
    /// ICE servers (STUN + optional TURN relay).
    #[serde(default)]
    pub ice_servers: Vec<IceServer>,
}

/// The `GET /api/v1/sessions/pending` response envelope.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PendingSessions {
    /// Sessions awaiting the agent.
    #[serde(default)]
    pub sessions: Vec<PendingSession>,
}

/// The pairing challenge returned by `POST /api/v1/pairing/initiate`.
#[derive(Debug, Clone, Deserialize)]
pub struct PairingChallenge {
    /// Challenge id the mobile confirms against.
    pub pairing_id: String,
    /// Opaque deep-link string to encode as a QR code.
    #[serde(default)]
    pub qr_payload: String,
    /// Human-enterable 8-digit code.
    #[serde(default)]
    pub manual_code: String,
    /// RFC-3339 expiry timestamp.
    #[serde(default)]
    pub expires_at: String,
}
