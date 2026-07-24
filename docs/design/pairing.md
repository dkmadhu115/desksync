# Device pairing & trust

Phase 6 delivers the two services that turn a phone and a laptop into a trusted
pair: the **device** service (registration, presence, revocation) and the
**pairing** service (QR/manual-code challenge → confirmed, persistent pairing).

Both are stateless HTTP services behind the gateway; all routes require a valid
access token, and every row is scoped to the authenticated user.

## Devices

A device is a laptop running the agent (`desktop`) or a phone (`mobile`). The
server stores only the device's public key (private keys never leave the
device), its presence, and lifecycle metadata.

| Operation | Route | Notes |
|-----------|-------|-------|
| Register  | `POST /api/v1/devices` | Idempotent upsert keyed by public key. |
| List      | `GET /api/v1/devices` | Active (non-revoked) devices, newest first. |
| Get       | `GET /api/v1/devices/{id}` | Single active device. |
| Heartbeat | `POST /api/v1/devices/{id}/heartbeat` | Refreshes `last_seen_at` + presence. |
| Revoke    | `DELETE /api/v1/devices/{id}` | Soft-delete; cascades to pairings. |

**Registration** validates the `kind`/`platform` enums, a bounded display name,
and a base64-encoded 32-byte X25519 public key. The insert uses
`ON CONFLICT (public_key) DO UPDATE ... WHERE devices.user_id = EXCLUDED.user_id`
so a user can re-register the same key idempotently, while a key already owned by
a **different** user is rejected (`409 conflict`) — the guard makes the upsert
atomic and prevents key hijacking across accounts.

**Revocation** is a soft delete (`revoked_at`) run in a transaction that also
flips any pairing referencing the device to `revoked`. Revoked devices disappear
from listings and can no longer pair or start sessions.

The mobile client registers itself as a device on first pairing and caches the
server-assigned id; that id is the `mobile_device_id` used to confirm pairings.

## Pairing challenge

Pairing is a two-step, cross-device handshake:

1. **Initiate** (desktop, authenticated): `POST /api/v1/pairing/initiate` with a
   `desktop_device_id`. The service verifies the device belongs to the caller
   and is a desktop, then mints:
   - a random `pairing_id` (UUID),
   - an 8-digit numeric `manual_code`,
   - a `qr_payload` deep link: `desksync://pair?v=1&pid=<pairing_id>&code=<code>`.

   The desktop agent drives this via `desksync-agent pair`, which logs in,
   registers itself, calls initiate, and renders the QR + manual code in the
   terminal (see [desktop-agent.md](desktop-agent.md)).

   Only the **hash** of the code is stored (`crypto.HashToken`, SHA-256), in
   **Redis** with a short TTL (default 5 minutes). Nothing is written to
   PostgreSQL yet — a pending challenge is ephemeral and single-use.

2. **Confirm** (mobile, authenticated): `POST /api/v1/pairing/confirm` with
   `pairing_id`, `code`, and the mobile's `mobile_device_id`. The service:
   - loads the challenge from Redis (unknown/expired → generic failure),
   - checks the challenge belongs to the **same account** (no cross-user
     probing),
   - compares the code in constant time against the stored hash,
   - verifies the mobile device belongs to the caller and is a `mobile`,
   - upserts an **active, trusted** pairing for the unique `(mobile, desktop)`
     pair, and
   - consumes the challenge (one-time use).

## Anti-abuse properties

- **Hashed at rest** — codes are never stored or logged in plaintext.
- **Short-lived** — challenges expire via Redis TTL (and an explicit
  `expires_at` check) so a leaked code is useless within minutes.
- **Single-use** — a successful confirm (or too many failures) consumes the
  challenge.
- **Rate-limited** — each wrong code increments a per-challenge attempt counter;
  after `MaxAttempts` (default 5) the challenge is burned, defeating brute force
  of the 10⁸ code space.
- **Non-oracle errors** — all code/challenge failures return one generic
  `invalid_input` message so an attacker cannot distinguish "wrong code" from
  "expired" or "unknown pairing".
- **Two secrets** — an attacker needs both the random `pairing_id` (only in the
  QR / initiate response) *and* the code, within the TTL, on the right account.

## Persistent pairings

Confirmed pairings live in PostgreSQL (`pairings`) with `status = active` and
`trusted = true`, so the two devices reconnect automatically without re-pairing.
Management endpoints:

| Operation | Route | Notes |
|-----------|-------|-------|
| List   | `GET /api/v1/pairings` | Non-revoked pairings for the user. |
| Revoke | `DELETE /api/v1/pairings/{id}` | Marks the pairing revoked. |

A pairing id from this list is what the session service consumes to create a
remote-control session (see [signaling.md](signaling.md)).

## Configuration

| Env | Service | Default | Purpose |
|-----|---------|---------|---------|
| `DATABASE_URL` | device, pairing | — | PostgreSQL connection. |
| `REDIS_ADDR` | pairing | `localhost:6379` | Ephemeral challenge store. |
| `JWT_ACCESS_SECRET` | device, pairing | — | Verifies access tokens. |

## Testing

- **Unit** — device and pairing application logic run against in-memory fakes
  (validation, idempotent registration, key-conflict, code hashing/expiry/lockout,
  one-time use, cross-user denial).
- **Integration** (`DESKSYNC_INTEGRATION=1`, Postgres + Redis) — the full device
  lifecycle (register/heartbeat/list/revoke + pairing cascade) and the full
  pairing lifecycle (initiate → confirm → list → revoke, plus wrong-code
  handling) against the real schema and Redis.
