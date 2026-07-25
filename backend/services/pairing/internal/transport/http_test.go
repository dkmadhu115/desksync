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
	"github.com/desksync/backend/services/pairing/internal/domain"
	"github.com/desksync/backend/services/pairing/internal/service"
	"github.com/gofiber/fiber/v2"
)

const testUserID = "user-1"

// --- fakes ---

type fakeRepo struct {
	device        domain.DeviceRef
	deviceErr     error
	pairings      []domain.Pairing
	revokeErr     error
	revoked       []string
	upsertPairing domain.Pairing
}

func (f *fakeRepo) DeviceForUser(_ context.Context, id, userID string) (domain.DeviceRef, error) {
	if f.deviceErr != nil {
		return domain.DeviceRef{}, f.deviceErr
	}
	d := f.device
	d.ID = id
	d.UserID = userID
	return d, nil
}

func (f *fakeRepo) UpsertActivePairing(_ context.Context, userID, mobileID, desktopID string) (domain.Pairing, error) {
	p := f.upsertPairing
	p.UserID = userID
	p.MobileDeviceID = mobileID
	p.DesktopDeviceID = desktopID
	return p, nil
}

func (f *fakeRepo) ListPairings(_ context.Context, _ string) ([]domain.Pairing, error) {
	return f.pairings, nil
}

func (f *fakeRepo) RevokePairing(_ context.Context, id, _ string) error {
	if f.revokeErr != nil {
		return f.revokeErr
	}
	f.revoked = append(f.revoked, id)
	return nil
}

type fakeStore struct {
	saved    *domain.Challenge
	saveErr  error
	getCh    *domain.Challenge
	getErr   error
	attempts int
	consumed []string
}

func (f *fakeStore) Save(_ context.Context, ch domain.Challenge, _ time.Duration) error {
	if f.saveErr != nil {
		return f.saveErr
	}
	f.saved = &ch
	return nil
}

func (f *fakeStore) Get(_ context.Context, _ string) (domain.Challenge, error) {
	if f.getErr != nil {
		return domain.Challenge{}, f.getErr
	}
	return *f.getCh, nil
}

func (f *fakeStore) RecordFailedAttempt(_ context.Context, _ string) (int, error) {
	f.attempts++
	return f.attempts, nil
}

func (f *fakeStore) Consume(_ context.Context, pairingID string) error {
	f.consumed = append(f.consumed, pairingID)
	return nil
}

// --- harness ---

func newTestApp(t *testing.T, repo domain.Repository, store domain.ChallengeStore) (*fiber.App, string) {
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
	svc := service.New(service.Config{Repo: repo, Store: store})
	app := fiber.New()
	New(svc, jwtMgr).Register(app.Group("/api/v1"))
	return app, pair.AccessToken
}

func do(t *testing.T, app *fiber.App, method, path, token, body string) (int, []byte) {
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
	return resp.StatusCode, raw
}

func obj(t *testing.T, raw []byte) map[string]any {
	t.Helper()
	var m map[string]any
	if err := json.Unmarshal(raw, &m); err != nil {
		t.Fatalf("expected JSON object, got %s", raw)
	}
	return m
}

func TestInitiateRequiresAuth(t *testing.T) {
	app, _ := newTestApp(t, &fakeRepo{}, &fakeStore{})
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/pairing/initiate", "", `{"desktop_device_id":"d1"}`)
	if code != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", code)
	}
}

func TestInitiateCreatesChallenge(t *testing.T) {
	repo := &fakeRepo{device: domain.DeviceRef{Kind: domain.KindDesktop}}
	store := &fakeStore{}
	app, token := newTestApp(t, repo, store)
	code, raw := do(t, app, fiber.MethodPost, "/api/v1/pairing/initiate", token, `{"desktop_device_id":"desk-1"}`)
	if code != fiber.StatusCreated {
		t.Fatalf("status = %d, want 201 (body=%s)", code, raw)
	}
	body := obj(t, raw)
	if body["pairing_id"] == "" || body["manual_code"] == "" || body["qr_payload"] == "" {
		t.Fatalf("incomplete challenge response: %s", raw)
	}
	if store.saved == nil {
		t.Fatal("challenge was not stored")
	}
	if store.saved.CodeHash == body["manual_code"] {
		t.Fatal("code must be stored hashed, not in plaintext")
	}
}

func TestInitiateRejectsNonDesktop(t *testing.T) {
	repo := &fakeRepo{device: domain.DeviceRef{Kind: domain.KindMobile}}
	app, token := newTestApp(t, repo, &fakeStore{})
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/pairing/initiate", token, `{"desktop_device_id":"m1"}`)
	if code != fiber.StatusBadRequest {
		t.Fatalf("status = %d, want 400 for non-desktop", code)
	}
}

func TestInitiateDeviceNotFound(t *testing.T) {
	repo := &fakeRepo{deviceErr: domain.ErrDeviceNotFound}
	app, token := newTestApp(t, repo, &fakeStore{})
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/pairing/initiate", token, `{"desktop_device_id":"missing"}`)
	if code != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404", code)
	}
}

func TestConfirmWrongCodeIsGeneric(t *testing.T) {
	ch := &domain.Challenge{
		PairingID: "p1", UserID: testUserID, DesktopDeviceID: "desk-1",
		CodeHash:  "deadbeef", ExpiresAt: time.Now().Add(time.Minute),
	}
	store := &fakeStore{getCh: ch}
	app, token := newTestApp(t, &fakeRepo{}, store)
	code, raw := do(t, app, fiber.MethodPost, "/api/v1/pairing/confirm", token,
		`{"pairing_id":"p1","code":"00000000","mobile_device_id":"mob-1"}`)
	if code != fiber.StatusBadRequest {
		t.Fatalf("status = %d, want 400 (body=%s)", code, raw)
	}
	if store.attempts != 1 {
		t.Fatalf("attempts = %d, want 1 recorded", store.attempts)
	}
}

func TestConfirmSucceedsWithMatchingCode(t *testing.T) {
	// Hash of "12345678" per crypto.HashToken.
	const plain = "12345678"
	ch := &domain.Challenge{
		PairingID: "p1", UserID: testUserID, DesktopDeviceID: "desk-1",
		CodeHash:  crypto.HashToken(plain), ExpiresAt: time.Now().Add(time.Minute),
	}
	repo := &fakeRepo{
		device:        domain.DeviceRef{Kind: domain.KindMobile},
		upsertPairing: domain.Pairing{ID: "pair-1", Status: domain.StatusActive, Trusted: true},
	}
	store := &fakeStore{getCh: ch}
	app, token := newTestApp(t, repo, store)
	code, raw := do(t, app, fiber.MethodPost, "/api/v1/pairing/confirm", token,
		`{"pairing_id":"p1","code":"`+plain+`","mobile_device_id":"mob-1"}`)
	if code != fiber.StatusOK {
		t.Fatalf("status = %d, want 200 (body=%s)", code, raw)
	}
	body := obj(t, raw)
	if body["id"] != "pair-1" || body["trusted"] != true {
		t.Fatalf("unexpected pairing response: %s", raw)
	}
	if len(store.consumed) != 1 {
		t.Fatalf("challenge must be consumed once, got %d", len(store.consumed))
	}
}

func TestListPairingsReturnsArray(t *testing.T) {
	repo := &fakeRepo{pairings: []domain.Pairing{{ID: "a"}, {ID: "b"}}}
	app, token := newTestApp(t, repo, &fakeStore{})
	code, raw := do(t, app, fiber.MethodGet, "/api/v1/pairings/", token, "")
	if code != fiber.StatusOK {
		t.Fatalf("status = %d, want 200", code)
	}
	var arr []map[string]any
	if err := json.Unmarshal(raw, &arr); err != nil || len(arr) != 2 {
		t.Fatalf("expected 2-element array, got %s (err=%v)", raw, err)
	}
}

func TestRevokePairingNoContent(t *testing.T) {
	repo := &fakeRepo{}
	app, token := newTestApp(t, repo, &fakeStore{})
	code, _ := do(t, app, fiber.MethodDelete, "/api/v1/pairings/pair-1", token, "")
	if code != fiber.StatusNoContent {
		t.Fatalf("status = %d, want 204", code)
	}
	if len(repo.revoked) != 1 || repo.revoked[0] != "pair-1" {
		t.Fatalf("revoked = %v, want [pair-1]", repo.revoked)
	}
}

func TestRevokePairingNotFound(t *testing.T) {
	repo := &fakeRepo{revokeErr: domain.ErrPairingNotFound}
	app, token := newTestApp(t, repo, &fakeStore{})
	code, _ := do(t, app, fiber.MethodDelete, "/api/v1/pairings/missing", token, "")
	if code != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404", code)
	}
}
