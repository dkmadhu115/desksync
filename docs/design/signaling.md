# Signaling & session plane (Phase 5)

This document describes how a mobile client and a desktop agent discover each
other and negotiate a WebRTC connection. It covers the **session service**
(session lifecycle, signaling tickets, ICE configuration) and the **signaling
service** (the WebSocket relay). Media never flows through either service.

Related: [API design](api.md), [ADR 0004](../adr/0004-webrtc-with-e2e-encryption.md),
[ADR 0006](../adr/0006-stateless-signaling-tickets.md).

## Overview

```
  mobile (controller)                backend                    desktop (agent)
        │                                                              │
        │ 1. POST /api/v1/sessions {pairing_id}   (session service)    │
        │─────────────────────────────────────────▶                   │
        │   201 { session, signaling_url,                              │
        │         signaling_ticket, ice_servers }                      │
        │◀─────────────────────────────────────────                   │
        │                                                              │
        │ 2. WS connect  ?ticket=…&role=controller (signaling service) │
        │═════════════════════════════════════════▶                   │
        │                          ◀═══════════════ 2'. WS connect     │
        │                             ?ticket=…&role=agent             │
        │                                                              │
        │ 3. peer_joined  ◀───── (hub presence) ─────▶  peer_joined     │
        │                                                              │
        │ 4. offer ─▶  relay  ─▶ offer                                  │
        │    answer ◀─ relay ◀─ answer                                  │
        │    ice_candidate ⇄ relay ⇄ ice_candidate                     │
        │                                                              │
        │ 5. WebRTC P2P (or TURN relay); media + input data channel    │
        │◀════════════════ direct / relayed ═══════════════════════════▶
```

## Session service

`POST /api/v1/sessions` (bearer-authenticated) authorizes and creates a session:

1. Verify the `pairing_id` belongs to the caller and is `active`
   (`404` if not owned/found, `412` if not active).
2. Insert a `sessions` row (`status = connecting`) and record a `created`
   event in the append-only `session_events` log.
3. Issue a short-lived **signaling ticket** binding `{session_id, user_id,
   role=controller}`.
4. Build the **ICE server** list (STUN always; TURN with per-session,
   time-limited credentials when configured).

The response is `SessionCreated { session, signaling_url, signaling_ticket,
ice_servers }`. `GET /sessions`, `GET /sessions/{id}`, and
`POST /sessions/{id}/end` (idempotent) complete the lifecycle. All rows are
scoped to the authenticated user.

### ICE configuration

STUN URLs are returned verbatim. TURN uses the coturn **`use-auth-secret`**
(TURN REST API) scheme: the username is `"<expiryUnix>:<sessionID>"` and the
credential is `base64(HMAC-SHA1(TURN_STATIC_AUTH_SECRET, username))`. The static
secret is shared with coturn out of band and **never** sent to a client — only
derived credentials that expire (`TURN_CREDENTIAL_TTL`) are. See
`services/session/internal/ice`.

## Signaling tickets

Tickets (`pkg/signalticket`) are self-contained and HMAC-signed:

```
v1.<base64url(payload)>.<base64url(HMAC-SHA256(secret, payload))>
payload = { sid, uid, role, exp }
```

The session service is the **issuer**; the signaling service is the
**verifier**; both read the same `SIGNALING_TICKET_SECRET`. Because a ticket is
verified with only the shared secret, the signaling service needs no database
and scales horizontally. Tickets expire quickly (`SIGNALING_TICKET_TTL`,
default 2m), so a leaked signaling URL cannot be replayed, and a client cannot
join a session or assume a role it was not granted. Signatures are compared in
constant time.

## Signaling service

A `GET /api/v1/signaling/ws` WebSocket endpoint. Authentication happens
**before** the upgrade: the client passes `?ticket=…` (and optionally
`session`/`role`, which must match the ticket). An invalid or expired ticket is
rejected with `401` and no upgrade occurs.

### Hub (transport-agnostic core)

The relay logic lives in `services/signaling/internal/hub` and deals only in
byte messages plus a per-peer outbound channel, so it is fully unit-tested
without a socket. The thin WebSocket adapter (`internal/ws`) bridges a real
connection to a `Peer`.

- **Rooms** are keyed by `session_id` and hold at most two peers, one per role
  (`controller`, `agent`). A duplicate role is refused (the second connection is
  closed with a policy-violation code).
- **Presence**: on join, the newcomer learns about an already-present peer and
  the present peer is told the newcomer joined (`peer_joined`); on disconnect
  the remaining peer gets `peer_left`. This lets the controller wait for the
  agent before sending its offer.
- **Relay**: `offer`, `answer`, and `ice_candidate` envelopes are forwarded
  verbatim to the other peer; the server never parses SDP/ICE bodies.
- **Validation**: each message must be protocol `v1`, target the peer's own
  `session_id`, and carry a **strictly increasing nonce** (per-connection replay
  guard). `heartbeat` is accepted and dropped; `bye` is relayed then closes the
  connection. Anything else is rejected and the connection closed.
- **Backpressure**: a peer whose outbound buffer fills (slow consumer) is
  disconnected so it cannot stall the relay.

### Message envelope

Mirrors the Rust agent's `SignalEnvelope` and the Flutter client:

```json
{ "v": 1, "nonce": 42, "ts_ms": 1700000000000, "session_id": "…",
  "payload": { "kind": "offer|answer|ice_candidate|heartbeat|bye", … } }
```

Server-originated control messages use `kind = peer_joined | peer_left` and
carry the affected `role`; clients that don't recognize a kind ignore it.

## Security properties

- Backend is **untrusted for confidentiality**: it brokers connection setup
  only. Application-level E2E encryption (X25519 → HKDF → AES-256-GCM, Phase 9)
  protects the media/data payloads on top of DTLS-SRTP.
- **Authorization** is enforced twice: the REST layer checks pairing ownership;
  the ticket cryptographically binds the WebSocket to a session and role.
- **Replay protection** via monotonic nonces; **short ticket TTLs**; **constant
  -time** signature checks; **role uniqueness** per session.

## Configuration

| Variable | Used by | Purpose |
|---|---|---|
| `SIGNALING_TICKET_SECRET` | session, signaling | Shared HMAC secret for tickets |
| `SIGNALING_TICKET_TTL` | session | Ticket lifetime (default 2m) |
| `SIGNALING_PUBLIC_URL` | session | WS URL returned to clients |
| `STUN_URLS` | session | Comma-separated STUN URLs |
| `TURN_URLS` | session | Comma-separated TURN URLs |
| `TURN_STATIC_AUTH_SECRET` | session | Shared secret with coturn |
| `TURN_CREDENTIAL_TTL` | session | TURN credential lifetime |

## Testing

- `pkg/signalticket`: issue/verify round-trip, tamper/expiry/wrong-secret/role.
- `session/internal/ice`: STUN-only, derived TURN credentials, TURN omitted
  without a secret.
- `session/internal/service`: create/authorize/end (fakes) + integration test
  against Postgres (seeds a user, devices, active pairing).
- `signaling/internal/protocol` + `hub`: envelope parsing, nonce guard,
  presence, relay, replay/foreign-session rejection, bye, slow-peer drop.
- `signaling/internal/ws`: in-process WebSocket end-to-end — auth accept/reject,
  presence, offer relay, duplicate-role rejection.
