//! End-to-end session encryption for DeskSync.
//!
//! The signaling/relay servers broker connectivity but must never be able to
//! read the remote-control stream (input, clipboard, control actions). This
//! crate implements the application-layer AEAD that guarantees that:
//!
//! ```text
//! X25519 ECDH(shared)  ->  HKDF-SHA256  ->  two AES-256-GCM keys (one/direction)
//! ```
//!
//! ## Construction
//!
//! - **Key agreement**: X25519 ECDH between the two paired device keys (a helper
//!   is provided; callers may also pass an ephemeral shared secret for forward
//!   secrecy once the handshake exchanges ephemeral keys).
//! - **Key derivation**: HKDF-SHA256 with a fixed domain salt, expanded into a
//!   distinct 32-byte key **per direction** (`c2a` = controller→agent,
//!   `a2c` = agent→controller). The HKDF `info` binds the `session_id` and
//!   **both** device public keys, so a server that swaps a key produces
//!   different keys on each side and decryption fails — authenticating the
//!   handshake against a man-in-the-middle.
//! - **AEAD**: AES-256-GCM. Each frame is `counter(8, big-endian) || ciphertext||tag`.
//!   The 96-bit nonce is `0x00000000 || counter`; because each direction has its
//!   own key, counters never collide. The additional authenticated data binds the
//!   `session_id` and direction, so a frame cannot be replayed into the other
//!   direction or another session.
//! - **Replay protection**: the receiver tracks the highest counter it has
//!   *successfully authenticated* and rejects any counter `<=` it. State only
//!   advances on a valid tag, so forged frames can't skip legitimate ones.
//!
//! The wire format is mirrored byte-for-byte by the Flutter client
//! (`mobile/lib/features/security/secure_channel.dart`); see the interop vector
//! test for the shared constants.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod pinning;

pub use pinning::CertPinner;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

/// Length of an X25519/AES-256 key in bytes.
pub const KEY_LEN: usize = 32;
/// AES-GCM nonce length in bytes (96 bits).
pub const NONCE_LEN: usize = 12;
/// AES-GCM authentication tag length in bytes.
pub const TAG_LEN: usize = 16;
/// Length of the per-frame counter prefix in bytes.
pub const COUNTER_LEN: usize = 8;

/// Fixed HKDF salt (protocol/version domain separation).
const HKDF_SALT: &[u8] = b"desksync-e2e-v1";
/// HKDF info prefix for the controller→agent key.
const INFO_C2A: &[u8] = b"c2a";
/// HKDF info prefix for the agent→controller key.
const INFO_A2C: &[u8] = b"a2c";
/// AAD direction byte for controller→agent frames.
const DIR_C2A: u8 = 1;
/// AAD direction byte for agent→controller frames.
const DIR_A2C: u8 = 2;

/// Errors from the secure channel.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    /// The frame is too short to contain a counter + tag.
    #[error("malformed frame")]
    Malformed,
    /// The frame's counter is not strictly greater than the last accepted one.
    #[error("replayed or out-of-order frame")]
    Replay,
    /// AEAD authentication/decryption failed (tampering, wrong key, or wrong
    /// session/direction binding).
    #[error("authentication failed")]
    Open,
    /// Encryption failed.
    #[error("encryption failed")]
    Seal,
    /// The per-direction counter space is exhausted (rekey required).
    #[error("counter exhausted; rekey required")]
    CounterExhausted,
}

/// Result alias for crypto operations.
pub type Result<T> = std::result::Result<T, CryptoError>;

/// Which end of the channel this instance represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The controlling peer (the mobile client).
    Controller,
    /// The controlled peer (the desktop agent).
    Agent,
}

/// Perform X25519 ECDH, returning the raw 32-byte shared secret.
pub fn x25519_ecdh(local_secret: &[u8; KEY_LEN], peer_public: &[u8; KEY_LEN]) -> Zeroizing<[u8; KEY_LEN]> {
    let secret = StaticSecret::from(*local_secret);
    let peer = PublicKey::from(*peer_public);
    Zeroizing::new(*secret.diffie_hellman(&peer).as_bytes())
}

/// Derive the two directional AES-256-GCM keys from a shared secret, binding the
/// session id and both public keys. Returns `(c2a, a2c)`.
pub(crate) fn derive_keys(
    shared: &[u8; KEY_LEN],
    session_id: &[u8],
    controller_pub: &[u8; KEY_LEN],
    agent_pub: &[u8; KEY_LEN],
) -> (Zeroizing<[u8; KEY_LEN]>, Zeroizing<[u8; KEY_LEN]>) {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), shared);
    let c2a = expand(&hk, INFO_C2A, session_id, controller_pub, agent_pub);
    let a2c = expand(&hk, INFO_A2C, session_id, controller_pub, agent_pub);
    (c2a, a2c)
}

fn expand(
    hk: &Hkdf<Sha256>,
    prefix: &[u8],
    session_id: &[u8],
    controller_pub: &[u8; KEY_LEN],
    agent_pub: &[u8; KEY_LEN],
) -> Zeroizing<[u8; KEY_LEN]> {
    let mut info = Vec::with_capacity(prefix.len() + session_id.len() + 2 * KEY_LEN);
    info.extend_from_slice(prefix);
    info.extend_from_slice(session_id);
    info.extend_from_slice(controller_pub);
    info.extend_from_slice(agent_pub);
    let mut okm = Zeroizing::new([0u8; KEY_LEN]);
    hk.expand(&info, &mut *okm)
        .expect("HKDF expand of 32 bytes never fails");
    okm
}

/// An authenticated, encrypted, replay-protected channel between the two paired
/// peers. Construct one on each side with the same handshake inputs.
pub struct SecureChannel {
    send_key: Zeroizing<[u8; KEY_LEN]>,
    recv_key: Zeroizing<[u8; KEY_LEN]>,
    send_dir: u8,
    recv_dir: u8,
    session_id: Vec<u8>,
    send_counter: u64,
    recv_last: Option<u64>,
}

impl SecureChannel {
    /// Build a channel from an already-agreed shared secret.
    pub fn new(
        shared: &[u8; KEY_LEN],
        session_id: &[u8],
        controller_pub: &[u8; KEY_LEN],
        agent_pub: &[u8; KEY_LEN],
        role: Role,
    ) -> Self {
        let (c2a, a2c) = derive_keys(shared, session_id, controller_pub, agent_pub);
        let (send_key, send_dir, recv_key, recv_dir) = match role {
            Role::Controller => (c2a, DIR_C2A, a2c, DIR_A2C),
            Role::Agent => (a2c, DIR_A2C, c2a, DIR_C2A),
        };
        Self {
            send_key,
            recv_key,
            send_dir,
            recv_dir,
            session_id: session_id.to_vec(),
            send_counter: 0,
            recv_last: None,
        }
    }

    /// Build a channel by performing X25519 ECDH with the peer's public key.
    pub fn establish(
        local_secret: &[u8; KEY_LEN],
        peer_public: &[u8; KEY_LEN],
        session_id: &str,
        controller_pub: &[u8; KEY_LEN],
        agent_pub: &[u8; KEY_LEN],
        role: Role,
    ) -> Self {
        let shared = x25519_ecdh(local_secret, peer_public);
        Self::new(&shared, session_id.as_bytes(), controller_pub, agent_pub, role)
    }

    /// Encrypt `plaintext` into a self-framed sealed message.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let counter = self.send_counter;
        let cipher = Aes256Gcm::new_from_slice(&*self.send_key).map_err(|_| CryptoError::Seal)?;
        let nonce = nonce_for(counter);
        let aad = aad_for(&self.session_id, self.send_dir);
        let ct = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Seal)?;

        self.send_counter = self.send_counter.checked_add(1).ok_or(CryptoError::CounterExhausted)?;

        let mut frame = Vec::with_capacity(COUNTER_LEN + ct.len());
        frame.extend_from_slice(&counter.to_be_bytes());
        frame.extend_from_slice(&ct);
        Ok(frame)
    }

    /// Authenticate and decrypt a sealed message. Rejects replays/out-of-order
    /// frames; receive state only advances on a valid tag.
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>> {
        if frame.len() < COUNTER_LEN + TAG_LEN {
            return Err(CryptoError::Malformed);
        }
        let mut cbytes = [0u8; COUNTER_LEN];
        cbytes.copy_from_slice(&frame[..COUNTER_LEN]);
        let counter = u64::from_be_bytes(cbytes);

        if let Some(last) = self.recv_last {
            if counter <= last {
                return Err(CryptoError::Replay);
            }
        }

        let cipher = Aes256Gcm::new_from_slice(&*self.recv_key).map_err(|_| CryptoError::Open)?;
        let nonce = nonce_for(counter);
        let aad = aad_for(&self.session_id, self.recv_dir);
        let pt = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &frame[COUNTER_LEN..],
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::Open)?;

        self.recv_last = Some(counter);
        Ok(pt)
    }
}

fn nonce_for(counter: u64) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[NONCE_LEN - COUNTER_LEN..].copy_from_slice(&counter.to_be_bytes());
    n
}

fn aad_for(session_id: &[u8], dir: u8) -> Vec<u8> {
    let mut aad = Vec::with_capacity(session_id.len() + 1);
    aad.extend_from_slice(session_id);
    aad.push(dir);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels() -> (SecureChannel, SecureChannel) {
        let shared = [9u8; KEY_LEN];
        let cpub = [1u8; KEY_LEN];
        let apub = [2u8; KEY_LEN];
        let controller = SecureChannel::new(&shared, b"sess", &cpub, &apub, Role::Controller);
        let agent = SecureChannel::new(&shared, b"sess", &cpub, &apub, Role::Agent);
        (controller, agent)
    }

    #[test]
    fn round_trips_both_directions() {
        let (mut controller, mut agent) = channels();

        let f = controller.seal(b"move 10 20").unwrap();
        assert_eq!(agent.open(&f).unwrap(), b"move 10 20");

        let g = agent.seal(b"clipboard: hi").unwrap();
        assert_eq!(controller.open(&g).unwrap(), b"clipboard: hi");
    }

    #[test]
    fn establish_via_ecdh_interoperates() {
        // Two device secrets; each side does ECDH with the other's public key.
        let controller_secret = [7u8; KEY_LEN];
        let agent_secret = [8u8; KEY_LEN];
        let controller_pub = *PublicKey::from(&StaticSecret::from(controller_secret)).as_bytes();
        let agent_pub = *PublicKey::from(&StaticSecret::from(agent_secret)).as_bytes();

        let mut controller = SecureChannel::establish(
            &controller_secret,
            &agent_pub,
            "sess-1",
            &controller_pub,
            &agent_pub,
            Role::Controller,
        );
        let mut agent = SecureChannel::establish(
            &agent_secret,
            &controller_pub,
            "sess-1",
            &controller_pub,
            &agent_pub,
            Role::Agent,
        );

        let f = controller.seal(b"hello").unwrap();
        assert_eq!(agent.open(&f).unwrap(), b"hello");
    }

    #[test]
    fn replay_and_reorder_are_rejected() {
        let (mut controller, mut agent) = channels();
        let f1 = controller.seal(b"one").unwrap();
        let f2 = controller.seal(b"two").unwrap();

        assert_eq!(agent.open(&f2).unwrap(), b"two");
        // Replaying f2 is rejected, and f1 (lower counter) is now out of order.
        assert_eq!(agent.open(&f2), Err(CryptoError::Replay));
        assert_eq!(agent.open(&f1), Err(CryptoError::Replay));
    }

    #[test]
    fn tampered_ciphertext_fails_authentication() {
        let (mut controller, mut agent) = channels();
        let mut f = controller.seal(b"secret").unwrap();
        let last = f.len() - 1;
        f[last] ^= 0x01; // flip a tag bit
        assert_eq!(agent.open(&f), Err(CryptoError::Open));
        // A subsequent genuine frame still opens (state didn't advance on the
        // forged frame).
        let g = controller.seal(b"secret2").unwrap();
        assert_eq!(agent.open(&g).unwrap(), b"secret2");
    }

    #[test]
    fn wrong_session_binding_fails() {
        let shared = [9u8; KEY_LEN];
        let cpub = [1u8; KEY_LEN];
        let apub = [2u8; KEY_LEN];
        let mut controller = SecureChannel::new(&shared, b"sessA", &cpub, &apub, Role::Controller);
        let mut agent_other = SecureChannel::new(&shared, b"sessB", &cpub, &apub, Role::Agent);
        let f = controller.seal(b"x").unwrap();
        assert_eq!(agent_other.open(&f), Err(CryptoError::Open));
    }

    #[test]
    fn short_frame_is_malformed() {
        let (_c, mut agent) = channels();
        assert_eq!(agent.open(&[0u8; 4]), Err(CryptoError::Malformed));
    }

    #[test]
    fn interop_vector_is_stable() {
        // Shared constants mirrored by the Flutter client's interop test. A
        // change here is a wire-format break and must be made on both sides.
        let shared = [3u8; KEY_LEN];
        let session_id = b"sess-vector";
        let controller_pub = [1u8; KEY_LEN];
        let agent_pub = [2u8; KEY_LEN];

        let (c2a, a2c) = derive_keys(&shared, session_id, &controller_pub, &agent_pub);
        assert_eq!(
            hex::encode(&c2a[..]),
            "e975bd790b80759673c92d4e4de5710be3427041cb81613489a99a279f806ec8"
        );
        assert_eq!(
            hex::encode(&a2c[..]),
            "18f166f40d28a1dd6cc0448066d5d13aa5c9504300fe07ee6f551b3f5f7ea96f"
        );

        let mut controller = SecureChannel::new(&shared, session_id, &controller_pub, &agent_pub, Role::Controller);
        let frame = controller.seal(b"hello").unwrap();
        assert_eq!(
            hex::encode(&frame),
            "00000000000000001332232a849ee705233318f10f25aa7f7c39c00726"
        );
    }
}
