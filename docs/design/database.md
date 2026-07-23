# Database Design

DeskSync uses **PostgreSQL** as the single source of truth, **Redis** for
ephemeral/hot state, and **SQLite** on the desktop agent for a local cache.

- **PostgreSQL**: users, devices, pairings, sessions, certificates, audit.
- **Redis**: device presence (`presence:{device_id}` with TTL heartbeats),
  signaling pub/sub channels, rate-limit counters, short-lived pairing codes.
- **SQLite** (agent): cached config, workspace shortcuts, offline queue.

Migrations live in [`backend/migrations/`](../../backend/migrations) in
[golang-migrate](https://github.com/golang-migrate/migrate) `NNNNNN_name.up.sql`
/`.down.sql` format and are validated (apply + rollback) in CI. All primary keys
are UUIDs (`gen_random_uuid()` from `pgcrypto`); high-volume append-only tables
(`session_events`, `audit_logs`) use `BIGSERIAL`.

## Entity-Relationship Diagram

```mermaid
erDiagram
    users ||--o{ oauth_identities : has
    users ||--o{ refresh_tokens : has
    users ||--o{ devices : owns
    users ||--o{ pairings : owns
    users ||--o{ sessions : initiates
    users ||--o{ notifications : receives
    users ||--o{ audit_logs : generates

    devices ||--o{ device_certificates : presents
    devices ||--o{ pairings : "mobile side"
    devices ||--o{ pairings : "desktop side"

    pairings ||--o{ sessions : authorizes
    sessions ||--o{ session_events : records

    users {
        uuid id PK
        citext email UK
        text password_hash "nullable (OAuth-only)"
        bool email_verified
        bool is_active
        timestamptz created_at
    }
    oauth_identities {
        uuid id PK
        uuid user_id FK
        text provider "google|github"
        text provider_user_id
    }
    refresh_tokens {
        uuid id PK
        uuid user_id FK
        text token_hash UK "hashed"
        timestamptz expires_at
        timestamptz revoked_at
        uuid replaced_by FK "rotation"
    }
    devices {
        uuid id PK
        uuid user_id FK
        text kind "desktop|mobile"
        text platform
        text public_key UK "X25519 pub only"
        text status "online|offline"
        timestamptz last_seen_at
        timestamptz revoked_at
    }
    device_certificates {
        uuid id PK
        uuid device_id FK
        text fingerprint_sha256 UK
        timestamptz not_after
        timestamptz revoked_at
    }
    pairings {
        uuid id PK
        uuid mobile_device_id FK
        uuid desktop_device_id FK
        text status "pending|active|revoked"
        text pairing_code_hash "hashed, short-lived"
        bool trusted
    }
    sessions {
        uuid id PK
        uuid pairing_id FK
        text status
        text connection_type "p2p|relay"
        int timeout_seconds
        timestamptz started_at
        timestamptz ended_at
    }
    session_events {
        bigserial id PK
        uuid session_id FK
        text event_type
        jsonb detail
    }
    notifications {
        uuid id PK
        uuid user_id FK
        text channel "push|email"
        text status
    }
    audit_logs {
        bigserial id PK
        uuid user_id FK
        text action
        text outcome
        text correlation_id
    }
```

## Table reference

| Table | Purpose | Key notes |
|-------|---------|-----------|
| `users` | Account identity | `email` is `CITEXT` unique; `password_hash` NULL for OAuth-only accounts (Argon2id when set). |
| `oauth_identities` | Federated logins | Unique on `(provider, provider_user_id)`; one user can link Google + GitHub. |
| `refresh_tokens` | Refresh-token rotation | Stored **hashed**; `replaced_by` forms a rotation chain; revocation via `revoked_at`. |
| `devices` | Registered laptops/phones | Stores **public key only**; `status` + `last_seen_at` track presence; `revoked_at` for revocation. |
| `device_certificates` | Device certs (mTLS) | `fingerprint_sha256` unique; supports rotation and CRL-style revocation. |
| `pairings` | Persistent trust | Unique `(mobile_device_id, desktop_device_id)`; `pairing_code_hash` short-lived. |
| `sessions` | Remote-control sessions | `connection_type` records p2p vs relay; `timeout_seconds` drives idle expiry. |
| `session_events` | Append-only session log | JSONB `detail`; indexed by session + time. |
| `notifications` | Push/email outbox | Status machine `pending -> sent/failed`. |
| `audit_logs` | Security audit trail | Insert-only; carries `correlation_id` from the gateway. |

## Design decisions

- **Server never stores private keys.** `devices.public_key` holds only the
  X25519 public key; private keys stay on the device (Keyring on desktop,
  Secure Storage/Keychain on mobile). This is enforced by schema and reviewed in
  the [threat model](threat-model.md).
- **Secrets are hashed at rest.** Refresh tokens and pairing codes are stored as
  hashes so a database compromise does not yield usable credentials.
- **Append-only audit.** `audit_logs`/`session_events` are only ever INSERTed by
  application code, giving a tamper-evident trail for security review.
- **Presence in Redis, not Postgres hot-path.** Heartbeats update a Redis key
  with TTL; `devices.status`/`last_seen_at` are periodically reconciled to avoid
  write amplification on every heartbeat.
- **Cascade deletes** model ownership: removing a user removes their devices,
  pairings, sessions, and tokens.
