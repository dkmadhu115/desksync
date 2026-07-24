//! Agent configuration types.

use serde::{Deserialize, Serialize};

/// Video codec preference, negotiated with the peer at connect time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    /// VP9 — good quality/CPU tradeoff, wide software support.
    #[default]
    Vp9,
    /// H.264 — broad hardware-encoder availability.
    H264,
    /// H.265/HEVC — best compression where hardware supports it.
    H265,
}

/// Top-level, serializable agent configuration. Loaded from the config file
/// managed by the Tauri config UI and overridable via environment variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Stable device identifier issued at registration.
    pub device_id: String,
    /// Secure WebSocket URL of the signaling backend.
    pub backend_url: String,
    /// Base URL of the backend REST API gateway (used for enrollment, device
    /// registration, pairing, and heartbeats), e.g. `https://api.desksync.dev`.
    #[serde(default = "default_api_url")]
    pub api_url: String,
    /// Preferred video codec.
    #[serde(default)]
    pub codec: Codec,
    /// Target frames per second (adaptive at runtime).
    #[serde(default = "default_fps")]
    pub target_fps: u32,
    /// Maximum vertical resolution in pixels (e.g. 1080, 1440, 2160).
    #[serde(default = "default_max_height")]
    pub max_height: u32,
    /// Whether the agent starts automatically on login.
    #[serde(default)]
    pub autostart: bool,
    /// Heartbeat interval in seconds sent to the backend.
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
}

fn default_api_url() -> String {
    "http://localhost:8080".into()
}
fn default_fps() -> u32 {
    30
}
fn default_max_height() -> u32 {
    1080
}
fn default_heartbeat_secs() -> u64 {
    15
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            backend_url: String::new(),
            api_url: default_api_url(),
            codec: Codec::default(),
            target_fps: default_fps(),
            max_height: default_max_height(),
            autostart: false,
            heartbeat_secs: default_heartbeat_secs(),
        }
    }
}

impl AgentConfig {
    /// Validate invariants that must hold before the agent can start.
    pub fn validate(&self) -> Result<(), String> {
        if self.device_id.trim().is_empty() {
            return Err("device_id must not be empty".into());
        }
        if !self.backend_url.starts_with("wss://") && !self.backend_url.starts_with("ws://") {
            return Err("backend_url must be a ws:// or wss:// URL".into());
        }
        if self.target_fps == 0 || self.target_fps > 120 {
            return Err("target_fps must be in 1..=120".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = AgentConfig::default();
        assert_eq!(c.target_fps, 30);
        assert_eq!(c.max_height, 1080);
        assert_eq!(c.codec, Codec::Vp9);
    }

    #[test]
    fn validate_rejects_bad_config() {
        let mut c = AgentConfig::default();
        assert!(c.validate().is_err()); // empty device_id
        c.device_id = "d1".into();
        c.backend_url = "https://nope".into();
        assert!(c.validate().is_err()); // wrong scheme
        c.backend_url = "wss://ok".into();
        assert!(c.validate().is_ok());
        c.target_fps = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn roundtrips_through_json() {
        let c = AgentConfig {
            device_id: "d1".into(),
            backend_url: "wss://ok".into(),
            codec: Codec::H265,
            ..Default::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.codec, Codec::H265);
        assert_eq!(back.device_id, "d1");
    }
}
