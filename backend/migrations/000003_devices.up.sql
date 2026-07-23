-- Registered devices (laptops running the agent, and mobile clients) and their
-- cryptographic material. Private keys NEVER leave the device; the server only
-- stores public keys and issued certificates.

CREATE TABLE devices (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL CHECK (kind IN ('desktop', 'mobile')),
    platform          TEXT NOT NULL CHECK (platform IN ('windows', 'macos', 'linux', 'android', 'ios')),
    name              TEXT NOT NULL,
    -- X25519 public key (base64), used for the E2E key agreement.
    public_key        TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'offline' CHECK (status IN ('online', 'offline')),
    last_seen_at      TIMESTAMPTZ,
    fcm_token         TEXT,             -- mobile push token, when kind = 'mobile'
    revoked_at        TIMESTAMPTZ,      -- set on device revocation
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_devices_user_id ON devices(user_id);
CREATE INDEX idx_devices_status ON devices(status);
-- A given public key must be unique across the fleet.
CREATE UNIQUE INDEX idx_devices_public_key ON devices(public_key);

-- Per-device certificates used for mutual authentication (mTLS / device certs).
-- Rotating a certificate inserts a new active row and expires the old one.
CREATE TABLE device_certificates (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id          UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    serial             TEXT NOT NULL UNIQUE,
    certificate_pem    TEXT NOT NULL,
    fingerprint_sha256 TEXT NOT NULL UNIQUE,
    not_before         TIMESTAMPTZ NOT NULL,
    not_after          TIMESTAMPTZ NOT NULL,
    revoked_at         TIMESTAMPTZ,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_device_certificates_device_id ON device_certificates(device_id);
