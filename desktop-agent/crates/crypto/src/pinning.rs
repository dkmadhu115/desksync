//! Certificate pinning for the agent↔backend TLS connection.
//!
//! TLS still does normal chain validation via the platform/webpki roots; pinning
//! is an **additional** check that the presented leaf certificate's SHA-256
//! (a "SPKI/cert pin") is one we expect. This defends against a mis-issued or
//! rogue-CA certificate that would otherwise validate: a corporate MITM proxy,
//! a compromised CA, etc. The check is **fail-closed** — if pins are configured
//! and none match, the connection is refused.
//!
//! This module provides the pure pin computation and matching so it is fully
//! unit-tested; the rustls `ServerCertVerifier` wiring (which calls
//! [`CertPinner::verify_der`] after webpki validation) lives with the reqwest
//! client construction.

use base64::Engine;
use sha2::{Digest, Sha256};

/// A set of allowed leaf-certificate pins (base64 SHA-256 of the DER).
#[derive(Debug, Clone, Default)]
pub struct CertPinner {
    pins: Vec<String>,
}

impl CertPinner {
    /// Build a pinner from base64 SHA-256 pins. An empty set means pinning is
    /// not configured (see [`CertPinner::is_configured`]).
    pub fn new(pins: Vec<String>) -> Self {
        Self { pins }
    }

    /// Whether any pins are configured.
    pub fn is_configured(&self) -> bool {
        !self.pins.is_empty()
    }

    /// Compute the pin (base64 SHA-256) for a DER-encoded certificate.
    pub fn pin_for_der(der: &[u8]) -> String {
        let digest = Sha256::digest(der);
        base64::engine::general_purpose::STANDARD.encode(digest)
    }

    /// Verify that a presented DER certificate matches one of the pins.
    /// Returns `false` (fail-closed) when no pins are configured.
    pub fn verify_der(&self, der: &[u8]) -> bool {
        if !self.is_configured() {
            return false;
        }
        let pin = Self::pin_for_der(der);
        self.pins.iter().any(|p| p == &pin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_is_stable_base64_sha256() {
        // SHA-256("") = e3b0c442... ; base64 of that digest is a known constant.
        assert_eq!(
            CertPinner::pin_for_der(b""),
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
    }

    #[test]
    fn verify_matches_configured_pin() {
        let der = b"a fake certificate";
        let pin = CertPinner::pin_for_der(der);
        let pinner = CertPinner::new(vec!["someotherpin".into(), pin]);
        assert!(pinner.verify_der(der));
    }

    #[test]
    fn verify_rejects_unpinned_cert() {
        let pinner = CertPinner::new(vec![CertPinner::pin_for_der(b"trusted")]);
        assert!(!pinner.verify_der(b"rogue certificate"));
    }

    #[test]
    fn empty_pinner_fails_closed() {
        let pinner = CertPinner::new(vec![]);
        assert!(!pinner.is_configured());
        assert!(!pinner.verify_der(b"anything"));
    }
}
