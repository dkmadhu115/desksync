//! Backend REST client for the DeskSync desktop agent.
//!
//! This crate lets a headless agent enroll itself against the backend:
//! authenticate with the auth service, register this desktop as a device
//! (uploading only its public key), and initiate a pairing that the user
//! confirms from their phone. It also renders the pairing QR code for the
//! terminal.
//!
//! It is intentionally independent of the capture/input/transport crates and
//! uses rustls (pure Rust TLS) so it builds and is unit-tested on headless CI.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod client;
pub mod enrollment;
pub mod error;
pub mod models;
pub mod oauth;
pub mod qr;

pub use client::{BackendApi, BackendClient};
pub use enrollment::{Credentials, DeviceProfile, Enrollment, EnrollmentOutcome};
pub use error::{BackendError, Result};
pub use oauth::{google_login, login_with_provider, PkcePair, DEFAULT_LOGIN_TIMEOUT};
pub use models::{
    Device, DeviceRegistration, IceServer, PairingChallenge, PendingSession, PendingSessions, SessionRef, TokenPair,
};
pub use qr::render_qr;

/// Map the host OS to the backend's device `platform` enum
/// (windows/macos/linux/android/ios), defaulting to `linux` for unknown hosts.
pub fn detect_platform() -> String {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "ios" => "ios",
        "android" => "android",
        _ => "linux",
    }
    .to_string()
}

/// Best-effort human-friendly device name: `DESKSYNC_DEVICE_NAME`, else the
/// host name, else a stable default.
pub fn detect_device_name() -> String {
    std::env::var("DESKSYNC_DEVICE_NAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "DeskSync Desktop".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_is_a_known_enum_value() {
        let p = detect_platform();
        assert!(["windows", "macos", "linux", "android", "ios"].contains(&p.as_str()));
    }

    #[test]
    fn device_name_is_never_empty() {
        assert!(!detect_device_name().is_empty());
    }
}
