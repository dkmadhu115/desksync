package transport

import (
	"context"
	"encoding/json"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/crypto"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/services/auth/internal/domain"
	"github.com/desksync/backend/services/auth/internal/oauth"
	"github.com/desksync/backend/services/auth/internal/service"
	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
)

// ---- in-memory fakes ----

type fakeUsers struct {
	byID    map[string]domain.User
	byEmail map[string]string
}

func newFakeUsers() *fakeUsers {
	return &fakeUsers{byID: map[string]domain.User{}, byEmail: map[string]string{}}
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
func (f *fakeUsers) GetByProviderIdentity(_ context.Context, _ domain.Provider, _ string) (domain.User, error) {
	return domain.User{}, domain.ErrUserNotFound
}
func (f *fakeUsers) LinkOAuthIdentity(_ context.Context, _ domain.OAuthIdentity) error { return nil }

type fakeRefresh struct{ byID map[string]domain.RefreshToken }

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
func (f *fakeRefresh) RevokeFamily(_ context.Context, familyID string) error {
	now := time.Now()
	for jti, t := range f.byID {
		if t.FamilyID == familyID && t.RevokedAt == nil {
			t.RevokedAt = &now
			f.byID[jti] = t
		}
	}
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

type fakeStates struct{ m map[string]string }

func newFakeStates() *fakeStates { return &fakeStates{m: map[string]string{}} }

func (f *fakeStates) Save(_ context.Context, state, verifier string, _ time.Duration) error {
	f.m[state] = verifier
	return nil
}
func (f *fakeStates) Consume(_ context.Context, state string) (string, error) {
	v, ok := f.m[state]
	if !ok {
		return "", domain.ErrRefreshNotFound
	}
	delete(f.m, state)
	return v, nil
}

// ---- harness ----

func newTestApp(t *testing.T) *fiber.App {
	t.Helper()
	return newTestEnv(t, nil).app
}

// testEnv is a wired handler plus the fakes behind it, so tests can seed state
// (users, desktop grants) that the HTTP surface then operates on.
type testEnv struct {
	app      *fiber.App
	users    *fakeUsers
	desktops *fakeDesktops
}

// newTestEnv builds the auth HTTP surface over in-memory fakes. Pass a non-nil
// desktops store to enable the desktop sign-in endpoints.
func newTestEnv(t *testing.T, desktops *fakeDesktops) testEnv {
	t.Helper()
	jwtMgr, err := jwtauth.NewManager(config.JWTConfig{
		AccessSecret:  "0123456789abcdef0123456789abcdef",
		RefreshSecret: "abcdef0123456789abcdef0123456789",
		AccessTTL:     time.Hour,
		RefreshTTL:    720 * time.Hour,
		Issuer:        "desksync-test",
	})
	if err != nil {
		t.Fatalf("jwt manager: %v", err)
	}
	// Fast Argon2 parameters keep the HTTP tests quick.
	argon := crypto.DefaultArgon2Params()
	argon.Memory = 8 * 1024
	argon.Iterations = 1

	users := newFakeUsers()
	svc := service.New(service.Config{
		Users:      users,
		Refresh:    newFakeRefresh(),
		JWT:        jwtMgr,
		Argon:      argon,
		RefreshTTL: 720 * time.Hour,
		ReuseGrace: time.Minute,
	})
	cfg := Config{
		Service: svc,
		OAuth:   oauth.NewRegistry(config.OAuthConfig{}), // no providers configured
		States:  newFakeStates(),
	}
	// A typed nil in the interface field would defeat the `desktops == nil`
	// guards, so only set it when a store was supplied.
	if desktops != nil {
		cfg.Desktops = desktops
	}
	h := New(cfg)
	app := fiber.New()
	h.Register(app.Group("/api/v1"))
	return testEnv{app: app, users: users, desktops: desktops}
}

func do(t *testing.T, app *fiber.App, method, path, body string) (int, []byte) {
	t.Helper()
	var r io.Reader
	if body != "" {
		r = strings.NewReader(body)
	}
	req := httptest.NewRequest(method, path, r)
	if body != "" {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	raw, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, raw
}

func tokens(t *testing.T, raw []byte) (access, refresh string) {
	t.Helper()
	var body struct {
		AccessToken  string `json:"access_token"`
		RefreshToken string `json:"refresh_token"`
		TokenType    string `json:"token_type"`
	}
	if err := json.Unmarshal(raw, &body); err != nil {
		t.Fatalf("unmarshal tokens: %v (body=%s)", err, raw)
	}
	if body.TokenType != "Bearer" {
		t.Fatalf("token_type = %q, want Bearer", body.TokenType)
	}
	return body.AccessToken, body.RefreshToken
}

const goodPassword = "correct horse battery" // >= 12 chars

func TestRegisterAndLoginFlow(t *testing.T) {
	app := newTestApp(t)

	// Register.
	code, raw := do(t, app, fiber.MethodPost, "/api/v1/auth/register",
		`{"email":"a@example.com","password":"`+goodPassword+`","display_name":"A"}`)
	if code != fiber.StatusCreated {
		t.Fatalf("register status = %d, want 201 (body=%s)", code, raw)
	}
	access, refresh := tokens(t, raw)
	if access == "" || refresh == "" {
		t.Fatal("register did not return both tokens")
	}

	// Login with the same credentials.
	code, raw = do(t, app, fiber.MethodPost, "/api/v1/auth/login",
		`{"email":"a@example.com","password":"`+goodPassword+`"}`)
	if code != fiber.StatusOK {
		t.Fatalf("login status = %d, want 200 (body=%s)", code, raw)
	}
	_, refresh2 := tokens(t, raw)

	// Refresh rotates the token.
	code, raw = do(t, app, fiber.MethodPost, "/api/v1/auth/refresh",
		`{"refresh_token":"`+refresh2+`"}`)
	if code != fiber.StatusOK {
		t.Fatalf("refresh status = %d, want 200 (body=%s)", code, raw)
	}

	// Logout is idempotent success.
	code, _ = do(t, app, fiber.MethodPost, "/api/v1/auth/logout",
		`{"refresh_token":"`+refresh+`"}`)
	if code != fiber.StatusNoContent {
		t.Fatalf("logout status = %d, want 204", code)
	}
}

func TestRegisterDuplicateEmailConflict(t *testing.T) {
	app := newTestApp(t)
	body := `{"email":"dup@example.com","password":"` + goodPassword + `","display_name":"D"}`
	if code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/register", body); code != fiber.StatusCreated {
		t.Fatalf("first register status = %d, want 201", code)
	}
	code, raw := do(t, app, fiber.MethodPost, "/api/v1/auth/register", body)
	if code != fiber.StatusConflict {
		t.Fatalf("duplicate register status = %d, want 409 (body=%s)", code, raw)
	}
}

func TestRegisterValidation(t *testing.T) {
	app := newTestApp(t)
	cases := map[string]string{
		"short password": `{"email":"b@example.com","password":"short","display_name":"B"}`,
		"bad email":      `{"email":"not-an-email","password":"` + goodPassword + `"}`,
		"invalid body":   `not-json`,
	}
	for name, body := range cases {
		t.Run(name, func(t *testing.T) {
			code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/register", body)
			if code != fiber.StatusBadRequest {
				t.Fatalf("status = %d, want 400", code)
			}
		})
	}
}

func TestLoginWrongPassword(t *testing.T) {
	app := newTestApp(t)
	reg := `{"email":"c@example.com","password":"` + goodPassword + `","display_name":"C"}`
	if code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/register", reg); code != fiber.StatusCreated {
		t.Fatalf("register status = %d, want 201", code)
	}
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/login",
		`{"email":"c@example.com","password":"wrong password value"}`)
	if code != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", code)
	}
}

func TestLoginUnknownUserIsUnauthorized(t *testing.T) {
	app := newTestApp(t)
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/login",
		`{"email":"nobody@example.com","password":"`+goodPassword+`"}`)
	if code != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401 (no user enumeration)", code)
	}
}

func TestRefreshRequiresToken(t *testing.T) {
	app := newTestApp(t)
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/refresh", `{}`)
	if code != fiber.StatusBadRequest {
		t.Fatalf("status = %d, want 400", code)
	}
}

func TestRefreshInvalidTokenUnauthorized(t *testing.T) {
	app := newTestApp(t)
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/refresh",
		`{"refresh_token":"garbage.value.here"}`)
	if code != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", code)
	}
}

func TestLogoutWithInvalidTokenIsSuccess(t *testing.T) {
	app := newTestApp(t)
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/auth/logout",
		`{"refresh_token":"garbage"}`)
	if code != fiber.StatusNoContent {
		t.Fatalf("status = %d, want 204 (logout is best-effort)", code)
	}
}

func TestOAuthStartUnknownProvider(t *testing.T) {
	app := newTestApp(t)
	code, _ := do(t, app, fiber.MethodGet, "/api/v1/auth/oauth/google/start", "")
	if code != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404 for unconfigured provider", code)
	}
}

func TestOAuthCallbackUnknownProvider(t *testing.T) {
	app := newTestApp(t)
	code, _ := do(t, app, fiber.MethodGet, "/api/v1/auth/oauth/github/callback?state=x&code=y", "")
	if code != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404 for unconfigured provider", code)
	}
}
