-- Persistent trust relationships between a mobile device and a desktop device.
-- A pairing is created via QR or manual code and, once confirmed, persists so
-- the two devices reconnect automatically without re-pairing.

CREATE TABLE pairings (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    mobile_device_id   UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    desktop_device_id  UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    status             TEXT NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending', 'active', 'revoked')),
    -- Short-lived, hashed pairing code (never stored in plaintext).
    pairing_code_hash  TEXT,
    code_expires_at    TIMESTAMPTZ,
    trusted            BOOLEAN NOT NULL DEFAULT FALSE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    confirmed_at       TIMESTAMPTZ,
    revoked_at         TIMESTAMPTZ,
    -- A mobile/desktop pair is unique.
    UNIQUE (mobile_device_id, desktop_device_id)
);
CREATE INDEX idx_pairings_user_id ON pairings(user_id);
CREATE INDEX idx_pairings_desktop_device_id ON pairings(desktop_device_id);
CREATE INDEX idx_pairings_status ON pairings(status);
