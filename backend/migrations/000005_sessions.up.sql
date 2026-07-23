-- Remote-control sessions and their append-only event log.

CREATE TABLE sessions (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    pairing_id         UUID NOT NULL REFERENCES pairings(id) ON DELETE CASCADE,
    user_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status             TEXT NOT NULL DEFAULT 'initiating'
                          CHECK (status IN ('initiating', 'connecting', 'active', 'ended', 'failed')),
    -- How media flowed: p2p (direct) or relayed (TURN).
    connection_type    TEXT CHECK (connection_type IN ('p2p', 'relay')),
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at           TIMESTAMPTZ,
    end_reason         TEXT,
    -- Idle timeout in seconds after which the session auto-terminates.
    timeout_seconds    INTEGER NOT NULL DEFAULT 900,
    client_ip          INET,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_sessions_pairing_id ON sessions(pairing_id);
CREATE INDEX idx_sessions_user_id ON sessions(user_id);
CREATE INDEX idx_sessions_status ON sessions(status);
CREATE INDEX idx_sessions_started_at ON sessions(started_at);

-- Fine-grained, append-only session telemetry (connect, reconnect, input
-- enabled, file transfer, disconnect, etc.). Used for the session log UI and
-- for security auditing.
CREATE TABLE session_events (
    id            BIGSERIAL PRIMARY KEY,
    session_id    UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    event_type    TEXT NOT NULL,
    detail        JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_session_events_session_id ON session_events(session_id);
CREATE INDEX idx_session_events_created_at ON session_events(created_at);
