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
