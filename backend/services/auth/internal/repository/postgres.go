// Package repository provides PostgreSQL-backed implementations of the auth
// domain repositories using pgx.
package repository

import (
	"context"
	"errors"
	"fmt"

	"github.com/desksync/backend/services/auth/internal/domain"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

const uniqueViolation = "23505"

// UserRepo implements domain.UserRepository on PostgreSQL.
type UserRepo struct {
	pool *pgxpool.Pool
}

// NewUserRepo builds a UserRepo.
func NewUserRepo(pool *pgxpool.Pool) *UserRepo { return &UserRepo{pool: pool} }

// CreateUser inserts a user, translating unique-email violations.
func (r *UserRepo) CreateUser(ctx context.Context, u domain.User) (domain.User, error) {
	const q = `
		INSERT INTO users (email, password_hash, display_name, email_verified, is_active)
		VALUES ($1, $2, $3, $4, $5)
		RETURNING id, created_at, updated_at`
	row := r.pool.QueryRow(ctx, q, u.Email, u.PasswordHash, u.DisplayName, u.EmailVerified, u.IsActive)
	if err := row.Scan(&u.ID, &u.CreatedAt, &u.UpdatedAt); err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == uniqueViolation {
			return domain.User{}, domain.ErrEmailTaken
		}
		return domain.User{}, fmt.Errorf("create user: %w", err)
	}
	return u, nil
}

func (r *UserRepo) scanUser(row pgx.Row) (domain.User, error) {
	var u domain.User
	err := row.Scan(&u.ID, &u.Email, &u.PasswordHash, &u.DisplayName,
		&u.EmailVerified, &u.IsActive, &u.CreatedAt, &u.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return domain.User{}, domain.ErrUserNotFound
	}
	if err != nil {
		return domain.User{}, fmt.Errorf("scan user: %w", err)
	}
	return u, nil
}

const userColumns = `id, email, password_hash, display_name, email_verified, is_active, created_at, updated_at`

// GetUserByEmail looks up a user by (case-insensitive) email.
func (r *UserRepo) GetUserByEmail(ctx context.Context, email string) (domain.User, error) {
	q := `SELECT ` + userColumns + ` FROM users WHERE email = $1`
	return r.scanUser(r.pool.QueryRow(ctx, q, email))
}

// GetUserByID looks up a user by id.
func (r *UserRepo) GetUserByID(ctx context.Context, id string) (domain.User, error) {
	q := `SELECT ` + userColumns + ` FROM users WHERE id = $1`
	return r.scanUser(r.pool.QueryRow(ctx, q, id))
}

// GetByProviderIdentity finds a user via a linked OAuth identity.
func (r *UserRepo) GetByProviderIdentity(ctx context.Context, p domain.Provider, providerUserID string) (domain.User, error) {
	const q = `SELECT u.id, u.email, u.password_hash, u.display_name, u.email_verified, u.is_active, u.created_at, u.updated_at
		FROM users u
		JOIN oauth_identities oi ON oi.user_id = u.id
		WHERE oi.provider = $1 AND oi.provider_user_id = $2`
	return r.scanUser(r.pool.QueryRow(ctx, q, string(p), providerUserID))
}

// LinkOAuthIdentity attaches a federated identity idempotently.
func (r *UserRepo) LinkOAuthIdentity(ctx context.Context, id domain.OAuthIdentity) error {
	const q = `
		INSERT INTO oauth_identities (user_id, provider, provider_user_id)
		VALUES ($1, $2, $3)
		ON CONFLICT (provider, provider_user_id) DO NOTHING`
	if _, err := r.pool.Exec(ctx, q, id.UserID, string(id.Provider), id.ProviderUserID); err != nil {
		return fmt.Errorf("link oauth identity: %w", err)
	}
	return nil
}

// RefreshRepo implements domain.RefreshTokenRepository on PostgreSQL.
type RefreshRepo struct {
	pool *pgxpool.Pool
}

// NewRefreshRepo builds a RefreshRepo.
func NewRefreshRepo(pool *pgxpool.Pool) *RefreshRepo { return &RefreshRepo{pool: pool} }

// Create stores a new (hashed) refresh token.
func (r *RefreshRepo) Create(ctx context.Context, t domain.RefreshToken) error {
	const q = `
		INSERT INTO refresh_tokens (id, user_id, token_hash, issued_at, expires_at, user_agent, ip_address)
		VALUES ($1, $2, $3, $4, $5, $6, NULLIF($7, '')::inet)`
	if _, err := r.pool.Exec(ctx, q, t.ID, t.UserID, t.TokenHash, t.IssuedAt, t.ExpiresAt, t.UserAgent, t.IPAddress); err != nil {
		return fmt.Errorf("create refresh token: %w", err)
	}
	return nil
}

// GetByID fetches a refresh token by JTI.
func (r *RefreshRepo) GetByID(ctx context.Context, jti string) (domain.RefreshToken, error) {
	const q = `
		SELECT id, user_id, token_hash, issued_at, expires_at, revoked_at, replaced_by,
		       COALESCE(user_agent, ''), COALESCE(host(ip_address), '')
		FROM refresh_tokens WHERE id = $1`
	var t domain.RefreshToken
	err := r.pool.QueryRow(ctx, q, jti).Scan(
		&t.ID, &t.UserID, &t.TokenHash, &t.IssuedAt, &t.ExpiresAt,
		&t.RevokedAt, &t.ReplacedBy, &t.UserAgent, &t.IPAddress,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return domain.RefreshToken{}, domain.ErrRefreshNotFound
	}
	if err != nil {
		return domain.RefreshToken{}, fmt.Errorf("get refresh token: %w", err)
	}
	return t, nil
}

// Revoke marks a token revoked, recording an optional successor JTI.
func (r *RefreshRepo) Revoke(ctx context.Context, jti string, replacedBy *string) error {
	const q = `
		UPDATE refresh_tokens
		SET revoked_at = now(), replaced_by = $2
		WHERE id = $1 AND revoked_at IS NULL`
	tag, err := r.pool.Exec(ctx, q, jti, replacedBy)
	if err != nil {
		return fmt.Errorf("revoke refresh token: %w", err)
	}
	if tag.RowsAffected() == 0 {
		// Either unknown or already revoked; report not-found for the former.
		var exists bool
		if err := r.pool.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM refresh_tokens WHERE id=$1)`, jti).Scan(&exists); err != nil {
			return fmt.Errorf("revoke check: %w", err)
		}
		if !exists {
			return domain.ErrRefreshNotFound
		}
	}
	return nil
}

// RevokeAllForUser revokes every active token for a user.
func (r *RefreshRepo) RevokeAllForUser(ctx context.Context, userID string) error {
	const q = `UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL`
	if _, err := r.pool.Exec(ctx, q, userID); err != nil {
		return fmt.Errorf("revoke all refresh tokens: %w", err)
	}
	return nil
}
