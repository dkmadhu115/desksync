// Package service implements the auth use cases (register, login, refresh,
// logout, OAuth upsert) on top of the domain repositories. It is transport- and
// storage-agnostic and fully unit-testable with fake repositories.
package service

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/desksync/backend/pkg/crypto"
	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/services/auth/internal/domain"
	"github.com/google/uuid"
)

// Tokens is the result of a successful authentication.
type Tokens struct {
	AccessToken  string
	RefreshToken string
	ExpiresIn    int
	User         domain.User
}

// Metadata carries request context stored with refresh tokens for auditing.
type Metadata struct {
	UserAgent string
	IPAddress string
}

// Service holds dependencies for the auth use cases.
type Service struct {
	users      domain.UserRepository
	refresh    domain.RefreshTokenRepository
	jwt        *jwtauth.Manager
	argon      crypto.Argon2Params
	now        func() time.Time
	refreshT   time.Duration
	reuseGrace time.Duration
}

// Config configures a Service.
type Config struct {
	Users      domain.UserRepository
	Refresh    domain.RefreshTokenRepository
	JWT        *jwtauth.Manager
	Argon      crypto.Argon2Params
	RefreshTTL time.Duration
	// ReuseGrace is how long after a rotation the spent token may be presented
	// again and still be honoured. See [Service.Refresh].
	ReuseGrace time.Duration
}

// New builds a Service.
func New(c Config) *Service {
	return &Service{
		users:      c.Users,
		refresh:    c.Refresh,
		jwt:        c.JWT,
		argon:      c.Argon,
		now:        time.Now,
		refreshT:   c.RefreshTTL,
		reuseGrace: c.ReuseGrace,
	}
}

// Register creates a password account and returns an initial token pair.
func (s *Service) Register(ctx context.Context, email, password, displayName string, md Metadata) (Tokens, error) {
	email = normalizeEmail(email)
	if !validEmail(email) {
		return Tokens{}, apperr.New(apperr.CodeInvalidInput, "a valid email is required")
	}
	if len(password) < 12 {
		return Tokens{}, apperr.New(apperr.CodeInvalidInput, "password must be at least 12 characters")
	}

	hash, err := crypto.HashPassword(password, s.argon)
	if err != nil {
		return Tokens{}, apperr.Wrap(apperr.CodeInternal, "failed to hash password", err)
	}

	user, err := s.users.CreateUser(ctx, domain.User{
		Email:        email,
		PasswordHash: &hash,
		DisplayName:  displayName,
		IsActive:     true,
	})
	if err != nil {
		if errors.Is(err, domain.ErrEmailTaken) {
			return Tokens{}, apperr.New(apperr.CodeConflict, "email is already registered")
		}
		return Tokens{}, apperr.Wrap(apperr.CodeInternal, "failed to create user", err)
	}

	return s.issueTokens(ctx, user, md)
}

// Login authenticates a password account.
func (s *Service) Login(ctx context.Context, email, password string, md Metadata) (Tokens, error) {
	email = normalizeEmail(email)
	user, err := s.users.GetUserByEmail(ctx, email)
	if err != nil {
		if errors.Is(err, domain.ErrUserNotFound) {
			// Uniform error to avoid user enumeration.
			return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid credentials")
		}
		return Tokens{}, apperr.Wrap(apperr.CodeInternal, "lookup failed", err)
	}
	if !user.IsActive {
		return Tokens{}, apperr.New(apperr.CodeForbidden, "account is disabled")
	}
	if !user.HasPassword() {
		return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid credentials")
	}

	ok, err := crypto.VerifyPassword(password, *user.PasswordHash)
	if err != nil || !ok {
		return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid credentials")
	}
	return s.issueTokens(ctx, user, md)
}

// Refresh rotates a refresh token: it validates the token, revokes it, and
// issues a new pair.
//
// Presenting an already-spent token is normally evidence of theft, and the
// response is to revoke its family — the chain descended from one sign-in, i.e.
// that one device's session. Other devices on the account are unaffected, so a
// phone with a stale token cannot sign a desktop out.
//
// One benign case looks identical to theft and is tolerated: if the response to
// a rotation never reaches the client (a dropped connection, a timeout), the
// client still holds the token the server just spent and will retry with it.
// Within ReuseGrace of the rotation, and only while the successor is still
// unused, that retry is answered with a fresh pair instead of ending the
// session. A successor that has been used proves the client did get the
// response, so a token presented after that is treated as a replay.
func (s *Service) Refresh(ctx context.Context, refreshToken string, md Metadata) (Tokens, error) {
	claims, err := s.jwt.VerifyRefresh(refreshToken)
	if err != nil {
		return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid refresh token")
	}

	stored, err := s.refresh.GetByID(ctx, claims.ID)
	if err != nil {
		return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid refresh token")
	}
	if !crypto.EqualTokenHash(refreshToken, stored.TokenHash) {
		return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid refresh token")
	}

	// The token to spend is normally the one presented; for a retried rotation it
	// is the successor the client never received.
	rotating := stored
	if stored.RevokedAt != nil {
		successor, ok := s.retriedRotation(ctx, stored)
		if !ok {
			_ = s.refresh.RevokeFamily(ctx, stored.FamilyID)
			return Tokens{}, apperr.New(apperr.CodeUnauthorized, "refresh token reuse detected")
		}
		rotating = successor
	}
	if !rotating.Active(s.now()) {
		return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid refresh token")
	}

	user, err := s.users.GetUserByID(ctx, rotating.UserID)
	if err != nil {
		return Tokens{}, apperr.New(apperr.CodeUnauthorized, "invalid refresh token")
	}

	tokens, newJTI, err := s.mint(ctx, user, rotating.FamilyID, md)
	if err != nil {
		return Tokens{}, err
	}
	// Revoke the old token, chaining it to the new one.
	if err := s.refresh.Revoke(ctx, rotating.ID, &newJTI); err != nil {
		return Tokens{}, apperr.Wrap(apperr.CodeInternal, "failed to rotate token", err)
	}
	return tokens, nil
}

// retriedRotation reports the token to spend when an already-revoked token is
// presented, for the one case where that is a client retry rather than theft:
// the rotation happened moments ago and its successor has never been used, so
// the client cannot have received it.
func (s *Service) retriedRotation(ctx context.Context, spent domain.RefreshToken) (domain.RefreshToken, bool) {
	if s.reuseGrace <= 0 || spent.RevokedAt == nil || spent.ReplacedBy == nil {
		return domain.RefreshToken{}, false
	}
	if s.now().Sub(*spent.RevokedAt) > s.reuseGrace {
		return domain.RefreshToken{}, false
	}
	successor, err := s.refresh.GetByID(ctx, *spent.ReplacedBy)
	if err != nil || !successor.Active(s.now()) {
		return domain.RefreshToken{}, false
	}
	return successor, true
}

// Logout revokes the presented refresh token (best-effort on invalid input).
func (s *Service) Logout(ctx context.Context, refreshToken string) error {
	claims, err := s.jwt.VerifyRefresh(refreshToken)
	if err != nil {
		return nil // Already unusable; treat logout as success.
	}
	if err := s.refresh.Revoke(ctx, claims.ID, nil); err != nil && !errors.Is(err, domain.ErrRefreshNotFound) {
		return apperr.Wrap(apperr.CodeInternal, "failed to revoke token", err)
	}
	return nil
}

// UpsertOAuthUser finds or creates a user for a federated identity and issues
// tokens. Used by the browser OAuth callback flow.
func (s *Service) UpsertOAuthUser(ctx context.Context, p domain.Provider, providerUserID, email, displayName string, md Metadata) (Tokens, error) {
	user, err := s.ResolveOAuthUser(ctx, p, providerUserID, email, displayName)
	if err != nil {
		return Tokens{}, err
	}
	return s.issueTokens(ctx, user, md)
}

// ResolveOAuthUser finds or creates the user behind a federated identity and
// links the identity, without issuing tokens.
//
// The desktop sign-in flow needs the user *without* a token pair, because the
// tokens must be minted later for the desktop process rather than the browser
// that completed the consent screen.
func (s *Service) ResolveOAuthUser(ctx context.Context, p domain.Provider, providerUserID, email, displayName string) (domain.User, error) {
	email = normalizeEmail(email)

	if user, err := s.users.GetByProviderIdentity(ctx, p, providerUserID); err == nil {
		return user, nil
	} else if !errors.Is(err, domain.ErrUserNotFound) {
		return domain.User{}, apperr.Wrap(apperr.CodeInternal, "identity lookup failed", err)
	}

	// Link to an existing email account, or create a new OAuth-only account.
	user, err := s.users.GetUserByEmail(ctx, email)
	if errors.Is(err, domain.ErrUserNotFound) {
		user, err = s.users.CreateUser(ctx, domain.User{
			Email:         email,
			DisplayName:   displayName,
			EmailVerified: true,
			IsActive:      true,
		})
		if err != nil {
			return domain.User{}, apperr.Wrap(apperr.CodeInternal, "failed to create user", err)
		}
	} else if err != nil {
		return domain.User{}, apperr.Wrap(apperr.CodeInternal, "lookup failed", err)
	}

	if err := s.users.LinkOAuthIdentity(ctx, domain.OAuthIdentity{
		UserID:         user.ID,
		Provider:       p,
		ProviderUserID: providerUserID,
	}); err != nil {
		return domain.User{}, apperr.Wrap(apperr.CodeInternal, "failed to link identity", err)
	}
	return user, nil
}

// IssueForUserID mints a token pair for an already-authenticated user id.
//
// Used to complete the desktop sign-in flow: the OAuth callback authenticates
// the user in the browser but the tokens must be handed to the desktop process
// instead. Only the user id is held (briefly) between those two steps, so no
// token ever sits at rest.
func (s *Service) IssueForUserID(ctx context.Context, userID string, md Metadata) (Tokens, error) {
	user, err := s.users.GetUserByID(ctx, userID)
	if err != nil {
		if errors.Is(err, domain.ErrUserNotFound) {
			return Tokens{}, apperr.New(apperr.CodeUnauthorized, "unknown user")
		}
		return Tokens{}, apperr.Wrap(apperr.CodeInternal, "lookup failed", err)
	}
	if !user.IsActive {
		return Tokens{}, apperr.New(apperr.CodeForbidden, "account is disabled")
	}
	return s.issueTokens(ctx, user, md)
}

// issueTokens mints and persists a fresh pair for a user, starting a new token
// family: this is a sign-in, so it is a session of its own.
func (s *Service) issueTokens(ctx context.Context, user domain.User, md Metadata) (Tokens, error) {
	tokens, _, err := s.mint(ctx, user, "", md)
	return tokens, err
}

// mint creates a token pair, persists the hashed refresh token, and returns the
// new refresh JTI. An empty familyID starts a new family; a rotation passes the
// family it continues.
func (s *Service) mint(ctx context.Context, user domain.User, familyID string, md Metadata) (Tokens, string, error) {
	jti := uuid.NewString()
	pair, err := s.jwt.Issue(user.ID, jti)
	if err != nil {
		return Tokens{}, "", apperr.Wrap(apperr.CodeInternal, "failed to issue tokens", err)
	}
	if familyID == "" {
		familyID = jti
	}

	now := s.now()
	if err := s.refresh.Create(ctx, domain.RefreshToken{
		ID:        jti,
		UserID:    user.ID,
		FamilyID:  familyID,
		TokenHash: crypto.HashToken(pair.RefreshToken),
		IssuedAt:  now,
		ExpiresAt: now.Add(s.refreshT),
		UserAgent: md.UserAgent,
		IPAddress: md.IPAddress,
	}); err != nil {
		return Tokens{}, "", apperr.Wrap(apperr.CodeInternal, "failed to persist token", err)
	}

	return Tokens{
		AccessToken:  pair.AccessToken,
		RefreshToken: pair.RefreshToken,
		ExpiresIn:    pair.ExpiresIn,
		User:         user,
	}, jti, nil
}

func normalizeEmail(email string) string {
	return strings.ToLower(strings.TrimSpace(email))
}

func validEmail(email string) bool {
	at := strings.IndexByte(email, '@')
	if at <= 0 || at == len(email)-1 {
		return false
	}
	return strings.IndexByte(email[at+1:], '.') > 0
}
