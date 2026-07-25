// Package repository provides the PostgreSQL-backed session repository.
package repository

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/desksync/backend/services/session/internal/domain"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// SessionRepo implements domain.Repository on PostgreSQL.
type SessionRepo struct {
	pool *pgxpool.Pool
}

// New builds a SessionRepo.
func New(pool *pgxpool.Pool) *SessionRepo { return &SessionRepo{pool: pool} }

// PairingForUser returns the pairing when it belongs to the given user.
func (r *SessionRepo) PairingForUser(ctx context.Context, pairingID, userID string) (domain.Pairing, error) {
	const q = `
		SELECT id, mobile_device_id, desktop_device_id, status
		FROM pairings
		WHERE id = $1 AND user_id = $2`
	var p domain.Pairing
	err := r.pool.QueryRow(ctx, q, pairingID, userID).
		Scan(&p.ID, &p.MobileDeviceID, &p.DesktopDeviceID, &p.Status)
	if errors.Is(err, pgx.ErrNoRows) {
		return domain.Pairing{}, domain.ErrPairingNotFound
	}
	if err != nil {
		return domain.Pairing{}, fmt.Errorf("pairing lookup: %w", err)
	}
	return p, nil
}

const sessionColumns = `id, pairing_id, user_id, status, connection_type,
	started_at, ended_at, end_reason, timeout_seconds, created_at`

// prefixedSessionColumns returns the session columns qualified with a table
// alias, for queries that JOIN other tables (e.g. pairings).
func prefixedSessionColumns(alias string) string {
	return alias + `.id, ` + alias + `.pairing_id, ` + alias + `.user_id, ` + alias + `.status, ` +
		alias + `.connection_type, ` + alias + `.started_at, ` + alias + `.ended_at, ` +
		alias + `.end_reason, ` + alias + `.timeout_seconds, ` + alias + `.created_at`
}

func scanSession(row pgx.Row) (domain.Session, error) {
	var s domain.Session
	var connType *string
	err := row.Scan(&s.ID, &s.PairingID, &s.UserID, &s.Status, &connType,
		&s.StartedAt, &s.EndedAt, &s.EndReason, &s.TimeoutSeconds, &s.CreatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return domain.Session{}, domain.ErrSessionNotFound
	}
	if err != nil {
		return domain.Session{}, fmt.Errorf("scan session: %w", err)
	}
	if connType != nil {
		ct := domain.ConnectionType(*connType)
		s.ConnectionType = &ct
	}
	return s, nil
}

// CreateSession inserts a new session.
func (r *SessionRepo) CreateSession(ctx context.Context, s domain.Session) (domain.Session, error) {
	const q = `
		INSERT INTO sessions (pairing_id, user_id, status, timeout_seconds)
		VALUES ($1, $2, $3, $4)
		RETURNING ` + sessionColumns
	row := r.pool.QueryRow(ctx, q, s.PairingID, s.UserID, s.Status, s.TimeoutSeconds)
	return scanSession(row)
}

// GetSession returns a user-owned session.
func (r *SessionRepo) GetSession(ctx context.Context, id, userID string) (domain.Session, error) {
	q := `SELECT ` + sessionColumns + ` FROM sessions WHERE id = $1 AND user_id = $2`
	return scanSession(r.pool.QueryRow(ctx, q, id, userID))
}

// ListSessions returns the user's most recent sessions.
func (r *SessionRepo) ListSessions(ctx context.Context, userID string, limit int) ([]domain.Session, error) {
	if limit <= 0 || limit > 200 {
		limit = 50
	}
	q := `SELECT ` + sessionColumns + `
		FROM sessions WHERE user_id = $1
		ORDER BY started_at DESC LIMIT $2`
	rows, err := r.pool.Query(ctx, q, userID, limit)
	if err != nil {
		return nil, fmt.Errorf("list sessions: %w", err)
	}
	defer rows.Close()

	var out []domain.Session
	for rows.Next() {
		s, err := scanSession(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

// PendingSessionsForDevice returns connecting sessions for the user whose
// pairing targets the given desktop device. The agent polls this to discover
// sessions it should answer.
func (r *SessionRepo) PendingSessionsForDevice(ctx context.Context, userID, desktopDeviceID string, limit int) ([]domain.Session, error) {
	if limit <= 0 || limit > 50 {
		limit = 10
	}
	q := `SELECT ` + prefixedSessionColumns("s") + `
		FROM sessions s
		JOIN pairings p ON p.id = s.pairing_id
		WHERE s.user_id = $1
		  AND p.desktop_device_id = $2
		  AND s.status = 'connecting'
		ORDER BY s.started_at DESC
		LIMIT $3`
	rows, err := r.pool.Query(ctx, q, userID, desktopDeviceID, limit)
	if err != nil {
		return nil, fmt.Errorf("pending sessions: %w", err)
	}
	defer rows.Close()

	var out []domain.Session
	for rows.Next() {
		s, err := scanSession(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

// EndSession marks a session ended (idempotent) and returns the current row.
func (r *SessionRepo) EndSession(ctx context.Context, id, userID, reason string) (domain.Session, error) {
	const q = `
		UPDATE sessions
		SET status = 'ended', ended_at = now(), end_reason = $3
		WHERE id = $1 AND user_id = $2 AND status NOT IN ('ended', 'failed')
		RETURNING ` + sessionColumns
	row := r.pool.QueryRow(ctx, q, id, userID, reason)
	s, err := scanSession(row)
	if errors.Is(err, domain.ErrSessionNotFound) {
		// Either the session doesn't exist for this user, or it is already
		// terminal; return the current row so end is idempotent.
		return r.GetSession(ctx, id, userID)
	}
	return s, err
}

// AppendEvent records a session event.
func (r *SessionRepo) AppendEvent(ctx context.Context, sessionID, eventType string, detail map[string]any) error {
	if detail == nil {
		detail = map[string]any{}
	}
	payload, err := json.Marshal(detail)
	if err != nil {
		return fmt.Errorf("marshal event detail: %w", err)
	}
	const q = `INSERT INTO session_events (session_id, event_type, detail) VALUES ($1, $2, $3)`
	if _, err := r.pool.Exec(ctx, q, sessionID, eventType, payload); err != nil {
		return fmt.Errorf("append session event: %w", err)
	}
	return nil
}
