# 6. Stateless, ticket-authorized signaling

- Status: Accepted
- Date: 2026-07-23

## Context

The signaling service brokers WebRTC connection setup between a mobile client
and a desktop agent. It must authenticate both peers, bind them to the correct
session, and scale horizontally without pinning a session to one instance. It
must also never become a confidentiality risk: it only routes SDP/ICE.

Two natural options for authorizing the WebSocket upgrade:

1. Reuse the user's JWT access token and look up the session in the database on
   every connect.
2. Have the session service mint a short-lived, self-contained **ticket** that
   the signaling service verifies with a shared secret — no datastore lookup.

## Decision

Use **short-lived HMAC-signed signaling tickets** (`pkg/signalticket`).

- The session service issues a ticket binding `{session_id, user_id, role}`
  with a small TTL (default 2m) when a session is created.
- The signaling service verifies the ticket with the shared
  `SIGNALING_TICKET_SECRET` and derives the room + role from it. It holds no
  session state beyond in-memory rooms for currently-connected peers.
- The relay hub is **transport-agnostic** (byte messages + an outbound channel)
  so the routing/presence/replay logic is unit-tested without a socket; a thin
  adapter bridges the WebSocket.

## Consequences

- The signaling service is **stateless and horizontally scalable**: any
  instance can verify any ticket. (Cross-instance room affinity — both peers
  landing on the same pod — is handled by routing today and can move to a
  Redis-backed room registry if needed; deferred until load requires it.)
- Blast radius of a leaked signaling URL is bounded by the short TTL, and a
  client cannot assume a role or join a session it was not granted.
- One more shared secret to manage (`SIGNALING_TICKET_SECRET`), documented in
  `.env.example` and injected via compose/Helm.
- Authorization is enforced in two layers: REST pairing-ownership checks plus
  the cryptographic ticket on the socket.
