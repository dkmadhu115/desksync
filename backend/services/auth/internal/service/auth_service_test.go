package service

import (
	"context"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/crypto"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/services/auth/internal/domain"
	"github.com/google/uuid"
)

// ---- in-memory fakes ----

type fakeUsers struct {
	byID       map[string]domain.User
	byEmail    map[string]string // email -> id
	identities map[string]string // provider|puid -> userID
}

func newFakeUsers() *fakeUsers {
	return &fakeUsers{
		byID:       map[string]domain.User{},
		byEmail:    map[string]string{},
		identities: map[string]string{},
	}
}

func (f *fakeUsers) CreateUser(_ context.Context, u domain.User) (domain.User, error) {
	if _, ok := f.byEmail[u.Email]; ok {
		return domain.User{}, domain.ErrEmailTaken
	}
	u.ID = uuid.NewString()
	u.CreatedAt = time.Now()
	u.UpdatedAt = u.CreatedAt
	f.byID[u.ID] = u
	f.byEmail[u.Email] = u.ID
	return u, nil
}
func (f *fakeUsers) GetUserByEmail(_ context.Context, email string) (domain.User, error) {
	id, ok := f.byEmail[email]
	if !ok {
		return domain.User{}, domain.ErrUserNotFound
	}
	return f.byID[id], nil
}
func (f *fakeUsers) GetUserByID(_ context.Context, id string) (domain.User, error) {
	u, ok := f.byID[id]
	if !ok {
		return domain.User{}, domain.ErrUserNotFound
	}
	return u, nil
}
func (f *fakeUsers) GetByProviderIdentity(_ context.Context, p domain.Provider, puid string) (domain.User, error) {
	id, ok := f.identities[string(p)+"|"+puid]
	if !ok {
		return domain.User{}, domain.ErrUserNotFound
	}
	return f.byID[id], nil
}
func (f *fakeUsers) LinkOAuthIdentity(_ context.Context, id domain.OAuthIdentity) error {
	f.identities[string(id.Provider)+"|"+id.ProviderUserID] = id.UserID
	return nil
}

type fakeRefresh struct {
	byID map[string]domain.RefreshToken
}

func newFakeRefresh() *fakeRefresh { return &fakeRefresh{byID: map[string]domain.RefreshToken{}} }

func (f *fakeRefresh) Create(_ context.Context, t domain.RefreshToken) error {
	f.byID[t.ID] = t
	return nil
}
func (f *fakeRefresh) GetByID(_ context.Context, jti string) (domain.RefreshToken, error) {
	t, ok := f.byID[jti]
	if !ok {
		return domain.RefreshToken{}, domain.ErrRefreshNotFound
	}
	return t, nil
}
func (f *fakeRefresh) Revoke(_ context.Context, jti string, replacedBy *string) error {
	t, ok := f.byID[jti]
	if !ok {
		return domain.ErrRefreshNotFound
	}
	now := time.Now()
	t.RevokedAt = &now
	t.ReplacedBy = replacedBy
	f.byID[jti] = t
	return nil
}
func (f *fakeRefresh) RevokeAllForUser(_ context.Context, userID string) error {
	now := time.Now()
	for jti, t := range f.byID {
		if t.UserID == userID && t.RevokedAt == nil {
			t.RevokedAt = &now
			f.byID[jti] = t
		}
	}
	return nil
}

// ---- helpers ----

func newTestService(t *testing.T) (*Service, *fakeRefresh) {
	t.Helper()
	jm, err := jwtauth.NewManager(config.JWTConfig{
		AccessSecret:  "0123456789abcdef0123456789abcdef",
		RefreshSecret: "abcdef0123456789abcdef0123456789",
		AccessTTL:     15 * time.Minute,
		RefreshTTL:    720 * time.Hour,
		Issuer:        "desksync-test",
	})
	if err != nil {
		t.Fatalf("jwt manager: %v", err)
	}
	argon := crypto.DefaultArgon2Params()
	argon.Memory = 8 * 1024
	argon.Iterations = 1

	fr := newFakeRefresh()
	svc := New(Config{
		Users:      newFakeUsers(),
		Refresh:    fr,
		JWT:        jm,
		Argon:      argon,
		RefreshTTL: 720 * time.Hour,
	})
	return svc, fr
}

// ---- tests ----

func TestRegisterAndLogin(t *testing.T) {
	svc, _ := newTestService(t)
	ctx := context.Background()

	reg, err := svc.Register(ctx, "Dev@Example.com", "supersecretpw12", "Dev", Metadata{})
	if err != nil {
		t.Fatalf("Register: %v", err)
	}
	if reg.AccessToken == "" || reg.RefreshToken == "" {
		t.Fatal("expected tokens from Register")
	}
	if reg.User.Email != "dev@example.com" {
		t.Fatalf("email not normalized: %q", reg.User.Email)
	}

	// Duplicate registration is a conflict.
	if _, err := svc.Register(ctx, "dev@example.com", "supersecretpw12", "Dev", Metadata{}); err == nil {
		t.Fatal("expected conflict on duplicate email")
	}

	login, err := svc.Login(ctx, "dev@example.com", "supersecretpw12", Metadata{})
	if err != nil {
		t.Fatalf("Login: %v", err)
	}
	if login.User.ID != reg.User.ID {
		t.Fatal("login returned different user")
	}
}

func TestRegisterRejectsWeakInput(t *testing.T) {
	svc, _ := newTestService(t)
	ctx := context.Background()
	if _, err := svc.Register(ctx, "bad-email", "supersecretpw12", "", Metadata{}); err == nil {
		t.Fatal("expected invalid email error")
	}
	if _, err := svc.Register(ctx, "ok@example.com", "short", "", Metadata{}); err == nil {
		t.Fatal("expected weak password error")
	}
}

func TestLoginWrongPassword(t *testing.T) {
	svc, _ := newTestService(t)
	ctx := context.Background()
	_, _ = svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})
	if _, err := svc.Login(ctx, "dev@example.com", "wrongpassword1", Metadata{}); err == nil {
		t.Fatal("expected unauthorized on wrong password")
	}
}

func TestRefreshRotation(t *testing.T) {
	svc, fr := newTestService(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})

	rot, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{})
	if err != nil {
		t.Fatalf("Refresh: %v", err)
	}
	if rot.RefreshToken == reg.RefreshToken {
		t.Fatal("refresh token was not rotated")
	}
	// New access token must verify.
	if rot.AccessToken == "" {
		t.Fatal("no new access token")
	}
	// Exactly one active token should remain.
	active := 0
	for _, tok := range fr.byID {
		if tok.RevokedAt == nil {
			active++
		}
	}
	if active != 1 {
		t.Fatalf("active tokens = %d, want 1", active)
	}
}

func TestRefreshReuseDetection(t *testing.T) {
	svc, fr := newTestService(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})

	// First rotation succeeds and revokes the original.
	if _, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{}); err != nil {
		t.Fatalf("first refresh: %v", err)
	}
	// Reusing the original (now revoked) token must fail and nuke the family.
	if _, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{}); err == nil {
		t.Fatal("expected reuse detection error")
	}
	for _, tok := range fr.byID {
		if tok.RevokedAt == nil {
			t.Fatal("reuse detection should revoke all tokens for the user")
		}
	}
}

func TestLogoutRevokes(t *testing.T) {
	svc, fr := newTestService(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})

	if err := svc.Logout(ctx, reg.RefreshToken); err != nil {
		t.Fatalf("Logout: %v", err)
	}
	// The revoked token can no longer be refreshed.
	if _, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{}); err == nil {
		t.Fatal("expected refresh to fail after logout")
	}
	_ = fr
}

func TestOAuthUpsertCreatesThenReuses(t *testing.T) {
	svc, _ := newTestService(t)
	ctx := context.Background()

	first, err := svc.UpsertOAuthUser(ctx, domain.ProviderGitHub, "gh-123", "dev@example.com", "Dev", Metadata{})
	if err != nil {
		t.Fatalf("first upsert: %v", err)
	}
	second, err := svc.UpsertOAuthUser(ctx, domain.ProviderGitHub, "gh-123", "dev@example.com", "Dev", Metadata{})
	if err != nil {
		t.Fatalf("second upsert: %v", err)
	}
	if first.User.ID != second.User.ID {
		t.Fatal("OAuth upsert created a duplicate user")
	}
}
