# 7. Ephemeral, hashed pairing challenges in Redis

- Status: Accepted
- Date: 2026-07-24

## Context

Pairing links a phone to a laptop via a QR code or a short manual code. The flow
is inherently two-step and cross-device: the desktop **initiates** (and displays
a code) while the mobile **confirms** it later. This raises two questions:

1. Where does the pending challenge live between initiate and confirm?
2. How do we stop an attacker who sees the `pairing_id` from brute-forcing the
   8-digit code (a 10⁸ space)?

The `pairings` table requires both `mobile_device_id` and `desktop_device_id`
(NOT NULL, with a unique constraint on the pair). At initiate time the mobile
device is unknown, so a pending pairing cannot be a well-formed persistent row.

## Decision

Keep pending pairing challenges **ephemeral in Redis**, and only write to
PostgreSQL once a pairing is confirmed.

- **Initiate** stores `{pairing_id, user_id, desktop_device_id, code_hash,
  expires_at}` in Redis with a short TTL (default 5m). The code is stored only as
  a SHA-256 hash (`crypto.HashToken`); the plaintext lives only in the QR payload
  / initiate response.
- **Confirm** verifies the code in constant time, checks ownership + expiry, then
  performs an idempotent `UPSERT` into `pairings` as `active`/`trusted`, and
  consumes the challenge.
- **Rate limiting** uses a per-challenge Redis counter; after `MaxAttempts`
  (default 5) the challenge is burned. All failures share one generic error to
  avoid an oracle.

## Consequences

- The persistent schema stays clean: no half-populated `pairings` rows, no
  nullable device columns, no background job to garbage-collect stale challenges
  (Redis TTL handles expiry).
- Brute force is defeated by the combination of a short TTL, a single-use
  challenge, per-challenge attempt lockout, and the need to know both the random
  `pairing_id` and the code on the correct account.
- The pairing service depends on Redis in addition to PostgreSQL (already part of
  the stack), reflected in its readiness checks and compose/Helm wiring.
- The `pairing_id` returned by initiate is a challenge handle; the confirmed
  pairing has its own database id. Clients treat them as distinct.
