-- Outbound notifications (push/email) and the security audit trail.

CREATE TABLE notifications (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id     UUID REFERENCES devices(id) ON DELETE SET NULL,
    channel       TEXT NOT NULL CHECK (channel IN ('push', 'email')),
    kind          TEXT NOT NULL,           -- e.g. connection_request, session_ended, security_alert
    title         TEXT NOT NULL,
    body          TEXT NOT NULL DEFAULT '',
    payload       JSONB NOT NULL DEFAULT '{}'::jsonb,
    status        TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'sent', 'failed')),
    sent_at       TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_notifications_user_id ON notifications(user_id);
CREATE INDEX idx_notifications_status ON notifications(status);

-- Immutable audit log for security-relevant actions. Application code only
-- INSERTs; there is no UPDATE/DELETE path in normal operation.
CREATE TABLE audit_logs (
    id             BIGSERIAL PRIMARY KEY,
    user_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    device_id      UUID REFERENCES devices(id) ON DELETE SET NULL,
    action         TEXT NOT NULL,          -- login, logout, pair, revoke, session_start, ...
    outcome        TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    ip_address     INET,
    user_agent     TEXT NOT NULL DEFAULT '',
    -- Correlation ID propagated from the gateway for request tracing.
    correlation_id TEXT NOT NULL DEFAULT '',
    detail         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_audit_logs_user_id ON audit_logs(user_id);
CREATE INDEX idx_audit_logs_action ON audit_logs(action);
CREATE INDEX idx_audit_logs_created_at ON audit_logs(created_at);
