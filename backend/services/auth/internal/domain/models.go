// Package domain holds the auth service's core entities and repository
// contracts. It is free of transport/storage concerns (Clean Architecture):
// the application layer depends on these interfaces, and the repository layer
// implements them.
package domain

import "time"

// Provider identifies a federated identity provider.
type Provider string

const (
	// ProviderGoogle is Google OAuth.
	ProviderGoogle Provider = "google"
	// ProviderGitHub is GitHub OAuth.
	ProviderGitHub Provider = "github"
)

// User is a DeskSync account.
type User struct {
	ID            string
	Email         string
	PasswordHash  *string // nil for OAuth-only accounts
	DisplayName   string
	EmailVerified bool
	IsActive      bool
	CreatedAt     time.Time
	UpdatedAt     time.Time
}

// HasPassword reports whether the user can log in with a password.
func (u User) HasPassword() bool { return u.PasswordHash != nil && *u.PasswordHash != "" }

// OAuthIdentity links a user to a federated identity.
type OAuthIdentity struct {
	ID             string
	UserID         string
	Provider       Provider
	ProviderUserID string
	CreatedAt      time.Time
}

// DesktopFlow is the pending context of a native-client (desktop agent) sign-in:
// where to hand the result back — a loopback port on the user's own machine —
// and the PKCE challenge that must be satisfied to redeem it.
type DesktopFlow struct {
	RedirectPort  int    `json:"redirect_port"`
	CodeChallenge string `json:"code_challenge"`
}

// DesktopGrant is a completed browser sign-in awaiting redemption by the desktop
// process that started it. It holds only the user id — never tokens — so nothing
// usable sits at rest between the two legs of the flow.
type DesktopGrant struct {
	UserID        string `json:"user_id"`
	CodeChallenge string `json:"code_challenge"`
}

// RefreshToken is a persisted (hashed) refresh token in a rotation chain.
type RefreshToken struct {
	ID         string // JTI
	UserID     string
	TokenHash  string
	IssuedAt   time.Time
	ExpiresAt  time.Time
	RevokedAt  *time.Time
	ReplacedBy *string
	UserAgent  string
	IPAddress  string
}

// Active reports whether the token is currently usable at time now.
func (t RefreshToken) Active(now time.Time) bool {
	return t.RevokedAt == nil && now.Before(t.ExpiresAt)
}
