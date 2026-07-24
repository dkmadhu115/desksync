// Package repository provides the PostgreSQL-backed pairing repository.
package repository

import (
	"context"
	"errors"
	"fmt"

	"github.com/desksync/backend/services/pairing/internal/domain"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PairingRepo implements domain.Repository on PostgreSQL.
type PairingRepo struct {
	pool *pgxpool.Pool
}

// New builds a PairingRepo.
func New(pool *pgxpool.Pool) *PairingRepo { return &PairingRepo{pool: pool} }

// DeviceForUser returns a non-revoked device owned by the user.
func (r *PairingRepo) DeviceForUser(ctx context.Context, deviceID, userID string) (domain.DeviceRef, error) {
	const q = `
		SELECT id, user_id, kind
		FROM devices
		WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL`
	var d domain.DeviceRef
	err := r.pool.QueryRow(ctx, q, deviceID, userID).Scan(&d.ID, &d.UserID, &d.Kind)
	if errors.Is(err, pgx.ErrNoRows) {
		return domain.DeviceRef{}, domain.ErrDeviceNotFound
	}
	if err != nil {
		return domain.DeviceRef{}, fmt.Errorf("device lookup: %w", err)
	}
	return d, nil
}

const pairingColumns = `id, user_id, mobile_device_id, desktop_device_id, status,
	trusted, created_at, confirmed_at`

func scanPairing(row pgx.Row) (domain.Pairing, error) {
	var p domain.Pairing
	err := row.Scan(&p.ID, &p.UserID, &p.MobileDeviceID, &p.DesktopDeviceID,
		&p.Status, &p.Trusted, &p.CreatedAt, &p.ConfirmedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return domain.Pairing{}, domain.ErrPairingNotFound
	}
	if err != nil {
		return domain.Pairing{}, fmt.Errorf("scan pairing: %w", err)
	}
	return p, nil
}

// UpsertActivePairing creates or reactivates an active, trusted pairing for the
// unique (mobile, desktop) device pair.
func (r *PairingRepo) UpsertActivePairing(ctx context.Context, userID, mobileDeviceID, desktopDeviceID string) (domain.Pairing, error) {
	const q = `
		INSERT INTO pairings (user_id, mobile_device_id, desktop_device_id, status, trusted, confirmed_at)
		VALUES ($1, $2, $3, 'active', TRUE, now())
		ON CONFLICT (mobile_device_id, desktop_device_id) DO UPDATE
			SET status = 'active',
			    trusted = TRUE,
			    confirmed_at = now(),
			    revoked_at = NULL
		RETURNING ` + pairingColumns
	return scanPairing(r.pool.QueryRow(ctx, q, userID, mobileDeviceID, desktopDeviceID))
}

// ListPairings returns the user's non-revoked pairings, newest first.
func (r *PairingRepo) ListPairings(ctx context.Context, userID string) ([]domain.Pairing, error) {
	q := `SELECT ` + pairingColumns + `
		FROM pairings
		WHERE user_id = $1 AND status <> 'revoked'
		ORDER BY created_at DESC`
	rows, err := r.pool.Query(ctx, q, userID)
	if err != nil {
		return nil, fmt.Errorf("list pairings: %w", err)
	}
	defer rows.Close()

	var out []domain.Pairing
	for rows.Next() {
		p, err := scanPairing(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, p)
	}
	return out, rows.Err()
}

// RevokePairing marks a user-owned pairing revoked.
func (r *PairingRepo) RevokePairing(ctx context.Context, id, userID string) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE pairings
		SET status = 'revoked', revoked_at = now()
		WHERE id = $1 AND user_id = $2 AND status <> 'revoked'`, id, userID)
	if err != nil {
		return fmt.Errorf("revoke pairing: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return domain.ErrPairingNotFound
	}
	return nil
}
