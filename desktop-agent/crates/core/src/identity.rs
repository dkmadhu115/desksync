//! Device cryptographic identity.
//!
//! Every desktop agent owns a long-lived X25519 key pair. The **private key
//! never leaves the device** (spec, security requirements): it is generated
//! locally, persisted only on this host (ideally in the OS keychain), and used
//! for the ECDH handshake that derives the end-to-end session key with a paired
//! mobile client. Only the public key is ever transmitted to the backend or a
//! peer.
//!
//! This module is intentionally pure Rust (no platform APIs) so it compiles and
//! is unit-tested on every target, including headless CI.

use crate::error::{AgentError, Result};
use x25519_dalek::{PublicKey, StaticSecret};

/// Length of an X25519 key (public or private) in bytes.
pub const KEY_LEN: usize = 32;

/// A device's long-lived X25519 identity used for the pairing/session ECDH.
///
/// The struct owns the secret; it is deliberately **not** `Clone`, `Debug`, or
/// `Serialize` so the private key cannot be accidentally logged or copied. Use
/// [`DeviceIdentity::secret_bytes`] only at the single persistence boundary.
pub struct DeviceIdentity {
    secret: StaticSecret,
    public: PublicKey,
}

impl DeviceIdentity {
    /// Generate a fresh identity from the operating-system CSPRNG.
    pub fn generate() -> Result<Self> {
        let mut bytes = [0u8; KEY_LEN];
        getrandom::getrandom(&mut bytes).map_err(|e| AgentError::Crypto(format!("failed to gather entropy: {e}")))?;
        Ok(Self::from_secret_bytes(bytes))
    }

    /// Reconstruct an identity from previously persisted secret-key bytes.
    pub fn from_secret_bytes(bytes: [u8; KEY_LEN]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Parse an identity from a hex-encoded secret key (as persisted).
    pub fn from_secret_hex(hex_str: &str) -> Result<Self> {
        let raw =
            hex::decode(hex_str.trim()).map_err(|e| AgentError::Crypto(format!("invalid secret key hex: {e}")))?;
        let bytes: [u8; KEY_LEN] = raw
            .try_into()
            .map_err(|_| AgentError::Crypto("secret key must be 32 bytes".into()))?;
        Ok(Self::from_secret_bytes(bytes))
    }

    /// Raw secret-key bytes. Only call this at the persistence boundary; never
    /// log or transmit the result.
    pub fn secret_bytes(&self) -> [u8; KEY_LEN] {
        self.secret.to_bytes()
    }

    /// Hex-encoded secret key, for writing to the (protected) key store.
    pub fn secret_hex(&self) -> String {
        hex::encode(self.secret_bytes())
    }

    /// Raw public-key bytes, safe to share with the backend and peers.
    pub fn public_bytes(&self) -> [u8; KEY_LEN] {
        *self.public.as_bytes()
    }

    /// Hex-encoded public key, safe to share.
    pub fn public_hex(&self) -> String {
        hex::encode(self.public_bytes())
    }

    /// A short, stable fingerprint of the public key for logs and QR pairing
    /// display (first 8 bytes, hex). Not security-sensitive on its own.
    pub fn fingerprint(&self) -> String {
        hex::encode(&self.public_bytes()[..8])
    }

    /// Perform the ECDH with a peer's public key, yielding the raw shared
    /// secret. The transport layer feeds this through HKDF to derive the
    /// AES-256-GCM session key (see `docs/design/security.md`).
    pub fn shared_secret(&self, peer_public: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
        let peer = PublicKey::from(*peer_public);
        *self.secret.diffie_hellman(&peer).as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_distinct_keys() {
        let a = DeviceIdentity::generate().unwrap();
        let b = DeviceIdentity::generate().unwrap();
        assert_ne!(a.public_hex(), b.public_hex());
        assert_ne!(a.secret_hex(), b.secret_hex());
        assert_eq!(a.public_bytes().len(), KEY_LEN);
    }

    #[test]
    fn secret_roundtrips_through_hex() {
        let id = DeviceIdentity::generate().unwrap();
        let restored = DeviceIdentity::from_secret_hex(&id.secret_hex()).unwrap();
        assert_eq!(id.secret_hex(), restored.secret_hex());
        assert_eq!(id.public_hex(), restored.public_hex());
    }

    #[test]
    fn ecdh_agrees_between_peers() {
        let device = DeviceIdentity::generate().unwrap();
        let mobile = DeviceIdentity::generate().unwrap();

        let on_device = device.shared_secret(&mobile.public_bytes());
        let on_mobile = mobile.shared_secret(&device.public_bytes());

        // Both sides derive the identical shared secret (the core ECDH property).
        assert_eq!(on_device, on_mobile);
        // And it is not simply one of the public keys.
        assert_ne!(on_device, device.public_bytes());
    }

    #[test]
    fn rejects_malformed_hex() {
        assert!(DeviceIdentity::from_secret_hex("nothex").is_err());
        assert!(DeviceIdentity::from_secret_hex("aabb").is_err()); // too short
    }

    #[test]
    fn fingerprint_is_stable_and_short() {
        let id = DeviceIdentity::generate().unwrap();
        assert_eq!(id.fingerprint(), id.fingerprint());
        assert_eq!(id.fingerprint().len(), 16); // 8 bytes hex
    }
}
