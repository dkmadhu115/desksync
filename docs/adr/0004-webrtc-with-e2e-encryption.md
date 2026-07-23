# 4. WebRTC transport with end-to-end encryption and TURN fallback

- Status: Accepted
- Date: 2026-07-23

## Context

The core product is a low-latency remote desktop: screen/audio out, input back,
across arbitrary networks (NAT, mobile carriers). We must maximize direct P2P
connectivity, degrade gracefully when it fails, and never expose plaintext media
to the backend.

## Decision

- Use **WebRTC** for media (SRTP) and a data channel for input/clipboard/files.
- Use **STUN** for NAT discovery and **ICE** for candidate negotiation; fall
  back to a **TURN relay (Coturn)** when direct P2P fails, using time-limited
  HMAC credentials issued by the `relay` service.
- Signaling runs over **secure WebSockets** through the `signaling` service,
  which only routes SDP/ICE and never holds media keys.
- Layer **application-level E2E encryption** (X25519 ECDH -> HKDF ->
  AES-256-GCM) authenticated by long-term device keys exchanged at pairing, so
  even a compromised backend or relay cannot decrypt content.

## Consequences

- Best-case direct P2P latency; guaranteed connectivity via relay.
- The backend is untrusted for confidentiality (defense in depth beyond DTLS).
- More cryptographic complexity on the clients (implemented in Phase 5/9), with
  the contract already modeled in `desksync-transport` and the signaling spec.
