// Package repository provides the PostgreSQL-backed device repository.
package repository

import (
	"context"
	"errors"
	"fmt"

	"github.com/desksync/backend/services/device/internal/domain"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// DeviceRepo implements domain.Repository on PostgreSQL.
type DeviceRepo struct {
	pool *pgxpool.Pool
}

// New builds a DeviceRepo.
func New(pool *pgxpool.Pool) *DeviceRepo { return &DeviceRepo{pool: pool} }

const deviceColumns = `id, user_id, kind, platform, name, public_key, status,
	last_seen_at, fcm_token, created_at, updated_at`

func scanDevice(row pgx.Row) (domain.Device, error) {
	var d domain.Device
	err := row.Scan(&d.ID, &d.UserID, &d.Kind, &d.Platform, &d.Name, &d.PublicKey,
		&d.Status, &d.LastSeenAt, &d.FCMToken, &d.CreatedAt, &d.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return domain.Device{}, domain.ErrDeviceNotFound
	}
	if err != nil {
		return domain.Device{}, fmt.Errorf("scan device: %w", err)
	}
	return d, nil
}

// Register inserts a device, or updates it in place when the same user
// re-registers an existing public key. The ON CONFLICT ... WHERE guard makes the
// upsert atomic and prevents one user from hijacking another user's key: if the
// key belongs to a different user the update matches no row and we report the
// conflict.
func (r *DeviceRepo) Register(ctx context.Context, reg domain.Registration) (domain.Device, error) {
	const q = `
		INSERT INTO devices (user_id, kind, platform, name, public_key, fcm_token)
		VALUES ($1, $2, $3, $4, $5, $6)
		ON CONFLICT (public_key) DO UPDATE
			SET kind = EXCLUDED.kind,
			    platform = EXCLUDED.platform,
			    name = EXCLUDED.name,
			    fcm_token = EXCLUDED.fcm_token,
			    revoked_at = NULL,
			    updated_at = now()
			WHERE devices.user_id = EXCLUDED.user_id
		RETURNING ` + deviceColumns
	row := r.pool.QueryRow(ctx, q, reg.UserID, reg.Kind, reg.Platform, reg.Name, reg.PublicKey, reg.FCMToken)
	d, err := scanDevice(row)
	if errors.Is(err, domain.ErrDeviceNotFound) {
		// No row returned means the public key exists but is owned by another
		// user (the conflicting update was filtered out by the WHERE guard).
		return domain.Device{}, domain.ErrPublicKeyTaken
	}
	return d, err
}

// Get returns an active device owned by the user.
func (r *DeviceRepo) Get(ctx context.Context, id, userID string) (domain.Device, error) {
	q := `SELECT ` + deviceColumns + `
		FROM devices WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL`
	return scanDevice(r.pool.QueryRow(ctx, q, id, userID))
}

// List returns the user's active devices, newest first.
func (r *DeviceRepo) List(ctx context.Context, userID string) ([]domain.Device, error) {
	q := `SELECT ` + deviceColumns + `
		FROM devices WHERE user_id = $1 AND revoked_at IS NULL
		ORDER BY created_at DESC`
	rows, err := r.pool.Query(ctx, q, userID)
	if err != nil {
		return nil, fmt.Errorf("list devices: %w", err)
	}
	defer rows.Close()

	var out []domain.Device
	for rows.Next() {
		d, err := scanDevice(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, d)
	}
	return out, rows.Err()
}

// Revoke soft-deletes the device and cascades revocation to any pairings that
// reference it, in a single transaction.
func (r *DeviceRepo) Revoke(ctx context.Context, id, userID string) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin revoke tx: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	tag, err := tx.Exec(ctx, `
		UPDATE devices
		SET revoked_at = now(), status = 'offline', updated_at = now()
		WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL`, id, userID)
	if err != nil {
		return fmt.Errorf("revoke device: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return domain.ErrDeviceNotFound
	}

	if _, err := tx.Exec(ctx, `
		UPDATE pairings
		SET status = 'revoked', revoked_at = now()
		WHERE user_id = $2
		  AND (mobile_device_id = $1 OR desktop_device_id = $1)
		  AND status <> 'revoked'`, id, userID); err != nil {
		return fmt.Errorf("revoke device pairings: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit revoke tx: %w", err)
	}
	return nil
}

// Heartbeat updates a device's presence and last-seen timestamp.
func (r *DeviceRepo) Heartbeat(ctx context.Context, id, userID string, status domain.Status) (domain.Device, error) {
	q := `
		UPDATE devices
		SET status = $3, last_seen_at = now(), updated_at = now()
		WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
		RETURNING ` + deviceColumns
	return scanDevice(r.pool.QueryRow(ctx, q, id, userID, status))
}
