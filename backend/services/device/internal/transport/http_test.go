package transport

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/services/device/internal/domain"
	"github.com/desksync/backend/services/device/internal/service"
	"github.com/gofiber/fiber/v2"
)

// validKey is a base64-encoded 32-byte (all-zero) X25519 public key.
var validKey = base64.StdEncoding.EncodeToString(make([]byte, 32))

// --- fake repository (implements domain.Repository) ---

type fakeRepo struct {
	device      domain.Device
	registerErr error
	list        []domain.Device
	notFound    bool
	revoked     []string
	lastStatus  domain.Status
}

func (f *fakeRepo) Register(_ context.Context, r domain.Registration) (domain.Device, error) {
	if f.registerErr != nil {
		return domain.Device{}, f.registerErr
	}
	f.device = domain.Device{
		ID: "dev-1", UserID: r.UserID, Kind: r.Kind, Platform: r.Platform,
		Name: r.Name, PublicKey: r.PublicKey, Status: domain.StatusOffline,
		CreatedAt: time.Now(), UpdatedAt: time.Now(),
	}
	return f.device, nil
}

func (f *fakeRepo) Get(_ context.Context, id, _ string) (domain.Device, error) {
	if f.notFound || id != "dev-1" {
		return domain.Device{}, domain.ErrDeviceNotFound
	}
	return domain.Device{ID: id, Kind: domain.KindMobile, Platform: domain.PlatformIOS, Name: "Phone"}, nil
}

func (f *fakeRepo) List(_ context.Context, _ string) ([]domain.Device, error) {
	return f.list, nil
}

func (f *fakeRepo) Revoke(_ context.Context, id, _ string) error {
	if f.notFound {
		return domain.ErrDeviceNotFound
	}
	f.revoked = append(f.revoked, id)
	return nil
}

func (f *fakeRepo) Heartbeat(_ context.Context, id, _ string, status domain.Status) (domain.Device, error) {
	if f.notFound {
		return domain.Device{}, domain.ErrDeviceNotFound
	}
	f.lastStatus = status
	now := time.Now()
	return domain.Device{ID: id, Status: status, LastSeenAt: &now}, nil
}

// --- test harness ---

const testUserID = "user-1"

func newTestApp(t *testing.T, repo domain.Repository) (*fiber.App, string) {
	t.Helper()
	jwtMgr, err := jwtauth.NewManager(config.JWTConfig{
		AccessSecret:  "test-access-secret-0123456789",
		RefreshSecret: "test-refresh-secret-0123456789",
		AccessTTL:     time.Minute,
		RefreshTTL:    time.Hour,
		Issuer:        "desksync",
	})
	if err != nil {
		t.Fatalf("jwt manager: %v", err)
	}
	pair, err := jwtMgr.Issue(testUserID, "jti-1")
	if err != nil {
		t.Fatalf("issue token: %v", err)
	}

	svc := service.New(service.Config{Repo: repo})
	app := fiber.New()
	New(svc, jwtMgr).Register(app.Group("/api/v1"))
	return app, pair.AccessToken
}

func do(t *testing.T, app *fiber.App, method, path, token, body string) (int, map[string]any) {
	t.Helper()
	var r io.Reader
	if body != "" {
		r = strings.NewReader(body)
	}
	req := httptest.NewRequest(method, path, r)
	if body != "" {
		req.Header.Set("Content-Type", "application/json")
	}
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	raw, _ := io.ReadAll(resp.Body)
	var m map[string]any
	if len(raw) > 0 && raw[0] == '{' {
		_ = json.Unmarshal(raw, &m)
	}
	return resp.StatusCode, m
}

func TestRegisterRequiresAuth(t *testing.T) {
	app, _ := newTestApp(t, &fakeRepo{})
	code, body := do(t, app, fiber.MethodPost, "/api/v1/devices/", "", `{}`)
	if code != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", code)
	}
	if body["error"] != "unauthorized" {
		t.Fatalf("error = %v, want unauthorized", body["error"])
	}
}

func TestRegisterCreated(t *testing.T) {
	repo := &fakeRepo{}
	app, token := newTestApp(t, repo)
	reqBody := `{"kind":"mobile","platform":"ios","name":"My Phone","public_key":"` + validKey + `"}`
	code, body := do(t, app, fiber.MethodPost, "/api/v1/devices/", token, reqBody)
	if code != fiber.StatusCreated {
		t.Fatalf("status = %d, want 201 (body=%v)", code, body)
	}
	if body["id"] != "dev-1" {
		t.Fatalf("id = %v, want dev-1", body["id"])
	}
	if repo.device.UserID != testUserID {
		t.Fatalf("device userID = %q, want %q (auth identity must be used)", repo.device.UserID, testUserID)
	}
}

func TestRegisterInvalidBody(t *testing.T) {
	app, token := newTestApp(t, &fakeRepo{})
	code, body := do(t, app, fiber.MethodPost, "/api/v1/devices/", token, `not-json`)
	if code != fiber.StatusBadRequest {
		t.Fatalf("status = %d, want 400", code)
	}
	if body["error"] != "invalid_input" {
		t.Fatalf("error = %v, want invalid_input", body["error"])
	}
}

func TestRegisterValidationRejected(t *testing.T) {
	app, token := newTestApp(t, &fakeRepo{})
	reqBody := `{"kind":"phone","platform":"ios","name":"X","public_key":"` + validKey + `"}`
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/devices/", token, reqBody)
	if code != fiber.StatusBadRequest {
		t.Fatalf("status = %d, want 400 for bad kind", code)
	}
}

func TestRegisterConflict(t *testing.T) {
	app, token := newTestApp(t, &fakeRepo{registerErr: domain.ErrPublicKeyTaken})
	reqBody := `{"kind":"mobile","platform":"ios","name":"My Phone","public_key":"` + validKey + `"}`
	code, body := do(t, app, fiber.MethodPost, "/api/v1/devices/", token, reqBody)
	if code != fiber.StatusConflict {
		t.Fatalf("status = %d, want 409 (body=%v)", code, body)
	}
}

func TestListReturnsArray(t *testing.T) {
	repo := &fakeRepo{list: []domain.Device{{ID: "a"}, {ID: "b"}}}
	app, token := newTestApp(t, repo)
	req := httptest.NewRequest(fiber.MethodGet, "/api/v1/devices/", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != fiber.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	var arr []map[string]any
	raw, _ := io.ReadAll(resp.Body)
	if err := json.Unmarshal(raw, &arr); err != nil {
		t.Fatalf("expected JSON array, got %s", raw)
	}
	if len(arr) != 2 {
		t.Fatalf("len = %d, want 2", len(arr))
	}
}

func TestGetNotFound(t *testing.T) {
	app, token := newTestApp(t, &fakeRepo{notFound: true})
	code, body := do(t, app, fiber.MethodGet, "/api/v1/devices/missing", token, "")
	if code != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404 (body=%v)", code, body)
	}
}

func TestRevokeNoContent(t *testing.T) {
	repo := &fakeRepo{}
	app, token := newTestApp(t, repo)
	code, _ := do(t, app, fiber.MethodDelete, "/api/v1/devices/dev-1", token, "")
	if code != fiber.StatusNoContent {
		t.Fatalf("status = %d, want 204", code)
	}
	if len(repo.revoked) != 1 || repo.revoked[0] != "dev-1" {
		t.Fatalf("revoked = %v, want [dev-1]", repo.revoked)
	}
}

func TestHeartbeatDefaultsOnlineOnEmptyBody(t *testing.T) {
	repo := &fakeRepo{}
	app, token := newTestApp(t, repo)
	code, body := do(t, app, fiber.MethodPost, "/api/v1/devices/dev-1/heartbeat", token, "")
	if code != fiber.StatusOK {
		t.Fatalf("status = %d, want 200 (body=%v)", code, body)
	}
	if repo.lastStatus != domain.StatusOnline {
		t.Fatalf("status = %q, want online", repo.lastStatus)
	}
}

func TestHeartbeatHonoursOfflineStatus(t *testing.T) {
	repo := &fakeRepo{}
	app, token := newTestApp(t, repo)
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/devices/dev-1/heartbeat", token, `{"status":"offline"}`)
	if code != fiber.StatusOK {
		t.Fatalf("status = %d, want 200", code)
	}
	if repo.lastStatus != domain.StatusOffline {
		t.Fatalf("status = %q, want offline", repo.lastStatus)
	}
}

func TestInvalidTokenRejected(t *testing.T) {
	app, _ := newTestApp(t, &fakeRepo{})
	code, _ := do(t, app, fiber.MethodGet, "/api/v1/devices/", "garbage.token.value", "")
	if code != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", code)
	}
}
