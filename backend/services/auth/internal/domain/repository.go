package domain

import "context"

// UserRepository persists users and their OAuth identities.
type UserRepository interface {
	// CreateUser inserts a new user and returns it with generated fields.
	// Returns ErrEmailTaken if the email already exists.
	CreateUser(ctx context.Context, u User) (User, error)
	// GetUserByEmail looks up a user by email. Returns ErrUserNotFound.
	GetUserByEmail(ctx context.Context, email string) (User, error)
	// GetUserByID looks up a user by id. Returns ErrUserNotFound.
	GetUserByID(ctx context.Context, id string) (User, error)

	// GetByProviderIdentity finds a user via a linked OAuth identity.
	// Returns ErrUserNotFound when no identity matches.
	GetByProviderIdentity(ctx context.Context, p Provider, providerUserID string) (User, error)
	// LinkOAuthIdentity attaches a federated identity to a user (idempotent).
	LinkOAuthIdentity(ctx context.Context, id OAuthIdentity) error
}

// RefreshTokenRepository persists the refresh-token rotation chain.
type RefreshTokenRepository interface {
	// Create stores a new refresh token (hashed).
	Create(ctx context.Context, t RefreshToken) error
	// GetByID fetches a refresh token by JTI. Returns ErrRefreshNotFound.
	GetByID(ctx context.Context, jti string) (RefreshToken, error)
	// Revoke marks a token revoked and records the successor JTI.
	Revoke(ctx context.Context, jti string, replacedBy *string) error
	// RevokeAllForUser revokes every active token for a user (theft response,
	// logout-all, revocation).
	RevokeAllForUser(ctx context.Context, userID string) error
}
