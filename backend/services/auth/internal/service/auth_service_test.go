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
	// now is shared with the service under test so revocation timestamps move
	// with the clock a test controls.
	now func() time.Time
}

func newFakeRefresh() *fakeRefresh {
	return &fakeRefresh{byID: map[string]domain.RefreshToken{}, now: time.Now}
}

// activeCount reports how many tokens are still usable, which is how the tests
// check the blast radius of a revocation.
func (f *fakeRefresh) activeCount() int {
	n := 0
	for _, t := range f.byID {
		if t.RevokedAt == nil {
			n++
		}
	}
	return n
}

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
	if t.RevokedAt == nil {
		now := f.now()
		t.RevokedAt = &now
		t.ReplacedBy = replacedBy
		f.byID[jti] = t
	}
	return nil
}
func (f *fakeRefresh) RevokeFamily(_ context.Context, familyID string) error {
	now := f.now()
	for jti, t := range f.byID {
		if t.FamilyID == familyID && t.RevokedAt == nil {
			t.RevokedAt = &now
			f.byID[jti] = t
		}
	}
	return nil
}
func (f *fakeRefresh) RevokeAllForUser(_ context.Context, userID string) error {
	now := f.now()
	for jti, t := range f.byID {
		if t.UserID == userID && t.RevokedAt == nil {
			t.RevokedAt = &now
			f.byID[jti] = t
		}
	}
	return nil
}

// ---- helpers ----

// testReuseGrace is the window a spent refresh token may still be retried in.
const testReuseGrace = time.Minute

// testClock is a clock the test moves by hand, shared by the service and its
// repository so a test can step past the reuse grace window without sleeping.
type testClock struct{ t time.Time }

func (c *testClock) now() time.Time          { return c.t }
func (c *testClock) advance(d time.Duration) { c.t = c.t.Add(d) }

func newTestService(t *testing.T) (*Service, *fakeRefresh) {
	svc, fr, _ := newTestServiceWithClock(t)
	return svc, fr
}

func newTestServiceWithClock(t *testing.T) (*Service, *fakeRefresh, *testClock) {
	t.Helper()
	jm, err := jwtauth.NewManager(config.JWTConfig{
		AccessSecret:  "0123456789abcdef0123456789abcdef",
		RefreshSecret: "abcdef0123456789abcdef0123456789",
		AccessTTL:     time.Hour,
		RefreshTTL:    720 * time.Hour,
		Issuer:        "desksync-test",
	})
	if err != nil {
		t.Fatalf("jwt manager: %v", err)
	}
	argon := crypto.DefaultArgon2Params()
	argon.Memory = 8 * 1024
	argon.Iterations = 1

	clock := &testClock{t: time.Now()}
	fr := newFakeRefresh()
	fr.now = clock.now
	svc := New(Config{
		Users:      newFakeUsers(),
		Refresh:    fr,
		JWT:        jm,
		Argon:      argon,
		RefreshTTL: 720 * time.Hour,
		ReuseGrace: testReuseGrace,
	})
	svc.now = clock.now
	return svc, fr, clock
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
	if active := fr.activeCount(); active != 1 {
		t.Fatalf("active tokens = %d, want 1", active)
	}
}

// A rotation stays inside the family it continues, which is what keeps a
// revocation from reaching the account's other sessions.
func TestRefreshKeepsTheFamily(t *testing.T) {
	svc, fr := newTestService(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})

	first := familyOf(t, svc, fr, reg.RefreshToken)
	rot, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{})
	if err != nil {
		t.Fatalf("Refresh: %v", err)
	}
	if got := familyOf(t, svc, fr, rot.RefreshToken); got != first {
		t.Fatalf("family after rotation = %q, want %q", got, first)
	}
}

// Two sign-ins are two sessions: signing in twice must not put both devices in
// one family, or a theft response would take out both.
func TestSignInStartsANewFamily(t *testing.T) {
	svc, fr := newTestService(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})
	second, err := svc.Login(ctx, "dev@example.com", "supersecretpw12", Metadata{})
	if err != nil {
		t.Fatalf("Login: %v", err)
	}
	if familyOf(t, svc, fr, reg.RefreshToken) == familyOf(t, svc, fr, second.RefreshToken) {
		t.Fatal("a second sign-in joined the first session's family")
	}
}

func TestRefreshReuseDetection(t *testing.T) {
	svc, _, clock := newTestServiceWithClock(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})

	// First rotation succeeds and revokes the original.
	rot, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{})
	if err != nil {
		t.Fatalf("first refresh: %v", err)
	}
	// Past the grace window the client cannot still be retrying, so reusing the
	// original token is theft and ends this session.
	clock.advance(testReuseGrace + time.Second)
	if _, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{}); err == nil {
		t.Fatal("expected reuse detection error")
	}
	if _, err := svc.Refresh(ctx, rot.RefreshToken, Metadata{}); err == nil {
		t.Fatal("the live token in a compromised family should have been revoked")
	}
}

// The theft response must end one session, not the account. This is the failure
// that logged a working desktop out whenever a phone presented a stale token.
func TestReuseDetectionSparesOtherDevices(t *testing.T) {
	svc, _, clock := newTestServiceWithClock(t)
	ctx := context.Background()
	phone, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})
	desktop, err := svc.Login(ctx, "dev@example.com", "supersecretpw12", Metadata{})
	if err != nil {
		t.Fatalf("second sign-in: %v", err)
	}

	// The phone rotates, then replays its spent token long after the fact.
	if _, err := svc.Refresh(ctx, phone.RefreshToken, Metadata{}); err != nil {
		t.Fatalf("phone refresh: %v", err)
	}
	clock.advance(testReuseGrace + time.Second)
	if _, err := svc.Refresh(ctx, phone.RefreshToken, Metadata{}); err == nil {
		t.Fatal("expected reuse detection for the replayed token")
	}

	// The desktop never misbehaved and must still be signed in.
	if _, err := svc.Refresh(ctx, desktop.RefreshToken, Metadata{}); err != nil {
		t.Fatalf("the desktop session was collateral damage: %v", err)
	}
}

// A client that never receives the response to its rotation has no choice but to
// retry with the token it still holds. That is indistinguishable from theft on
// the wire, so it is allowed briefly — otherwise every dropped response costs the
// user a session.
func TestRefreshHonoursARetryAfterALostResponse(t *testing.T) {
	svc, fr, clock := newTestServiceWithClock(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})

	// The rotation succeeds server-side but the response is lost in transit.
	lost, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{})
	if err != nil {
		t.Fatalf("first refresh: %v", err)
	}

	clock.advance(5 * time.Second)
	retry, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{})
	if err != nil {
		t.Fatalf("a retried rotation should be honoured: %v", err)
	}
	if retry.RefreshToken == lost.RefreshToken {
		t.Fatal("the retry handed back the pair the client never received")
	}
	// The pair from the retry is the live one, and the unseen one is spent.
	if _, err := svc.Refresh(ctx, retry.RefreshToken, Metadata{}); err != nil {
		t.Fatalf("the pair from the retry should be usable: %v", err)
	}
	if fr.activeCount() != 1 {
		t.Fatalf("active tokens = %d, want 1", fr.activeCount())
	}
}

// Once the successor has been used, the client demonstrably received it, so an
// older token turning up is a replay rather than a retry.
func TestRefreshRejectsAReplayOnceTheSuccessorWasUsed(t *testing.T) {
	svc, _, _ := newTestServiceWithClock(t)
	ctx := context.Background()
	reg, _ := svc.Register(ctx, "dev@example.com", "supersecretpw12", "", Metadata{})

	first, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{})
	if err != nil {
		t.Fatalf("first refresh: %v", err)
	}
	if _, err := svc.Refresh(ctx, first.RefreshToken, Metadata{}); err != nil {
		t.Fatalf("second refresh: %v", err)
	}
	// Still inside the grace window, but the chain has moved on.
	if _, err := svc.Refresh(ctx, reg.RefreshToken, Metadata{}); err == nil {
		t.Fatal("expected reuse detection for a replayed token")
	}
}

// familyOf resolves the family a refresh token belongs to.
func familyOf(t *testing.T, svc *Service, fr *fakeRefresh, refreshToken string) string {
	t.Helper()
	claims, err := svc.jwt.VerifyRefresh(refreshToken)
	if err != nil {
		t.Fatalf("verify refresh token: %v", err)
	}
	stored, ok := fr.byID[claims.ID]
	if !ok {
		t.Fatalf("refresh token %s is not stored", claims.ID)
	}
	return stored.FamilyID
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
