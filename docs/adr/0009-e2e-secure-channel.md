# 9. Application-layer end-to-end secure channel

- Status: Accepted
- Date: 2026-07-24

## Context

WebRTC already gives us DTLS-SRTP for the media path and DTLS for data channels,
so the *transport* between the two peers is encrypted. But DeskSync's security
objective is stronger: the signaling/relay/TURN servers must never be able to
read the remote-control stream, and we want confidentiality + integrity +
replay protection that is independent of the WebRTC stack and identical on both
clients. Relying solely on DTLS also ties our security to correct TURN/relay
configuration and to trusting the negotiated DTLS fingerprints, which the
signaling server relays and could substitute.

We need an application-layer AEAD, keyed by the paired devices' own keys, that
wraps input/clipboard/control frames regardless of how the bytes are carried.

## Decision

Add a dedicated `SecureChannel`, implemented identically in the Rust agent
(`desksync-crypto`) and the Flutter client
(`mobile/lib/features/security/secure_channel.dart`):

- **Key agreement**: X25519 ECDH between the paired device keys (or an ephemeral
  shared secret once the handshake exchanges ephemeral keys, for forward
  secrecy).
- **Key derivation**: HKDF-SHA256 with a fixed domain salt (`desksync-e2e-v1`),
  expanded into a **distinct key per direction** (`c2a`, `a2c`). The HKDF `info`
  binds the `session_id` and **both** device public keys, so a server that
  substitutes a key derives mismatched keys and every frame fails to
  authenticate — this authenticates the handshake against a MITM.
- **AEAD**: AES-256-GCM. Frame = `counter(8, big-endian) || ciphertext||tag`;
  the 96-bit nonce is `0…0 || counter`. Per-direction keys mean counters never
  collide. The AAD binds `session_id` + a direction byte, so a frame cannot be
  replayed into the other direction or another session.
- **Replay protection**: the receiver tracks the highest counter it has
  *successfully authenticated* and rejects counters `<=` it. State advances only
  on a valid tag, so a forged high-counter frame cannot suppress legitimate
  ones. This complements the signaling-plane `ReplayGuard` (monotonic nonces on
  SDP/ICE envelopes) and the server-side Redis seen-nonce set.

The wire format is frozen by a shared interop vector (fixed shared secret,
session id, and public keys → known derived keys and a known sealed frame). Both
the Rust and Dart test suites assert the exact same bytes, so the two
implementations cannot silently diverge.

## Certificates and pinning

Two supporting mechanisms ship alongside the channel:

- **Device certificates** (`backend/pkg/devicecert`): the backend runs an
  Ed25519 CA that issues a compact, signed certificate binding a device's id,
  owner, X25519 public key, kind, and validity window. Peers verify the CA
  signature and window offline; revocation is layered via
  `device_certificates.revoked_at`. The CA private key never leaves the backend.
- **TLS certificate pinning** (`CertPinner` in `desksync-crypto`;
  `CertificatePinner` on mobile): a fail-closed, additional check that the API
  gateway's leaf certificate SHA-256 is one we expect, defending against a
  mis-issued/rogue-CA certificate. Both implementations use the identical
  base64(SHA-256(DER)) pin format (asserted by a shared test constant).

## Consequences

- Media/data confidentiality no longer depends on trusting the relay or the
  DTLS fingerprints the signaling server forwards.
- The two client implementations share a byte-exact contract enforced by tests;
  changing the KDF/AEAD/framing is a deliberate, dual-sided wire break.
- Wiring the `SecureChannel` onto the live data channels (and exchanging
  ephemeral handshake keys for forward secrecy) lands with the native WebRTC
  peer, alongside the media encoder; the crypto core and its vectors are already
  in place and tested.
