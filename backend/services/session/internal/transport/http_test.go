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
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/session/internal/domain"
	"github.com/desksync/backend/services/session/internal/ice"
	"github.com/desksync/backend/services/session/internal/service"
	"github.com/gofiber/fiber/v2"
)

const testUserID = "user-1"

// --- fakes ---

type fakeRepo struct {
	pairing     domain.Pairing
	pairingErr  error
	created     domain.Session
	sessions    []domain.Session
	pending     []domain.Session
	getErr      error
	endErr      error
	ended       []string
}

func (f *fakeRepo) PairingForUser(_ context.Context, id, _ string) (domain.Pairing, error) {
	if f.pairingErr != nil {
		return domain.Pairing{}, f.pairingErr
	}
	p := f.pairing
	p.ID = id
	return p, nil
}

func (f *fakeRepo) CreateSession(_ context.Context, s domain.Session) (domain.Session, error) {
	f.created = domain.Session{
		ID: "sess-1", PairingID: s.PairingID, UserID: s.UserID,
		Status: domain.StatusConnecting, StartedAt: time.Now(),
	}
	return f.created, nil
}

func (f *fakeRepo) GetSession(_ context.Context, id, _ string) (domain.Session, error) {
	if f.getErr != nil {
		return domain.Session{}, f.getErr
	}
	return domain.Session{ID: id, Status: domain.StatusActive}, nil
}

func (f *fakeRepo) ListSessions(_ context.Context, _ string, _ int) ([]domain.Session, error) {
	return f.sessions, nil
}

func (f *fakeRepo) PendingSessionsForDevice(_ context.Context, _, _ string, _ int) ([]domain.Session, error) {
	return f.pending, nil
}

func (f *fakeRepo) EndSession(_ context.Context, id, _, _ string) (domain.Session, error) {
	if f.endErr != nil {
		return domain.Session{}, f.endErr
	}
	f.ended = append(f.ended, id)
	now := time.Now()
	return domain.Session{ID: id, Status: domain.StatusEnded, EndedAt: &now}, nil
}

func (f *fakeRepo) AppendEvent(_ context.Context, _, _ string, _ map[string]any) error {
	return nil
}

type fakeTickets struct{ err error }

func (f fakeTickets) Issue(_, _ string, _ signalticket.Role) (string, error) {
	if f.err != nil {
		return "", f.err
	}
	return "ticket-abc", nil
}

type fakeICE struct{}

func (fakeICE) Build(_ string) []ice.Server {
	return []ice.Server{{URLs: []string{"stun:stun.example.com:3478"}}}
}

// --- harness ---

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
	svc := service.New(service.Config{
		Repo: repo, Tickets: fakeTickets{}, ICE: fakeICE{},
		SignalingURL: "wss://sig.example.com/ws",
	})
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

func TestCreateRequiresAuth(t *testing.T) {
	app, _ := newTestApp(t, &fakeRepo{})
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/sessions/", "", `{"pairing_id":"p1"}`)
	if code != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", code)
	}
}

func TestCreateSessionCreated(t *testing.T) {
	repo := &fakeRepo{pairing: domain.Pairing{Status: "active"}}
	app, token := newTestApp(t, repo)
	code, raw := do(t, app, fiber.MethodPost, "/api/v1/sessions/", token, `{"pairing_id":"p1"}`)
	if code != fiber.StatusCreated {
		t.Fatalf("status = %d, want 201 (body=%s)", code, raw)
	}
	var body struct {
		Session         map[string]any `json:"session"`
		SignalingURL    string         `json:"signaling_url"`
		SignalingTicket string         `json:"signaling_ticket"`
		ICEServers      []any          `json:"ice_servers"`
	}
	if err := json.Unmarshal(raw, &body); err != nil {
		t.Fatalf("unmarshal: %v (body=%s)", err, raw)
	}
	if body.SignalingTicket != "ticket-abc" || body.SignalingURL != "wss://sig.example.com/ws" {
		t.Fatalf("unexpected signaling info: %s", raw)
	}
	if len(body.ICEServers) != 1 {
		t.Fatalf("ice servers = %d, want 1", len(body.ICEServers))
	}
	if body.Session["id"] != "sess-1" {
		t.Fatalf("session id = %v, want sess-1", body.Session["id"])
	}
}

func TestCreateMissingPairingID(t *testing.T) {
	app, token := newTestApp(t, &fakeRepo{})
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/sessions/", token, `{}`)
	if code != fiber.StatusBadRequest {
		t.Fatalf("status = %d, want 400", code)
	}
}

func TestCreatePairingNotFound(t *testing.T) {
	repo := &fakeRepo{pairingErr: domain.ErrPairingNotFound}
	app, token := newTestApp(t, repo)
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/sessions/", token, `{"pairing_id":"missing"}`)
	if code != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404", code)
	}
}

func TestCreateInactivePairingRejected(t *testing.T) {
	repo := &fakeRepo{pairing: domain.Pairing{Status: "revoked"}}
	app, token := newTestApp(t, repo)
	code, _ := do(t, app, fiber.MethodPost, "/api/v1/sessions/", token, `{"pairing_id":"p1"}`)
	if code != fiber.StatusPreconditionFailed {
		t.Fatalf("status = %d, want 412 for inactive pairing", code)
	}
}

func TestListSessionsReturnsArray(t *testing.T) {
	repo := &fakeRepo{sessions: []domain.Session{{ID: "a"}, {ID: "b"}, {ID: "c"}}}
	app, token := newTestApp(t, repo)
	code, raw := do(t, app, fiber.MethodGet, "/api/v1/sessions/", token, "")
	if code != fiber.StatusOK {
		t.Fatalf("status = %d, want 200", code)
	}
	var arr []map[string]any
	if err := json.Unmarshal(raw, &arr); err != nil || len(arr) != 3 {
		t.Fatalf("expected 3-element array, got %s (err=%v)", raw, err)
	}
}

func TestGetSession(t *testing.T) {
	app, token := newTestApp(t, &fakeRepo{})
	code, raw := do(t, app, fiber.MethodGet, "/api/v1/sessions/sess-9", token, "")
	if code != fiber.StatusOK {
		t.Fatalf("status = %d, want 200", code)
	}
	var m map[string]any
	_ = json.Unmarshal(raw, &m)
	if m["id"] != "sess-9" {
		t.Fatalf("id = %v, want sess-9", m["id"])
	}
}

func TestEndSession(t *testing.T) {
	repo := &fakeRepo{}
	app, token := newTestApp(t, repo)
	code, raw := do(t, app, fiber.MethodPost, "/api/v1/sessions/sess-1/end", token, "")
	if code != fiber.StatusOK {
		t.Fatalf("status = %d, want 200 (body=%s)", code, raw)
	}
	if len(repo.ended) != 1 || repo.ended[0] != "sess-1" {
		t.Fatalf("ended = %v, want [sess-1]", repo.ended)
	}
}

func TestGetSessionNotFound(t *testing.T) {
	app, token := newTestApp(t, &fakeRepo{getErr: domain.ErrSessionNotFound})
	code, _ := do(t, app, fiber.MethodGet, "/api/v1/sessions/missing", token, "")
	if code != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404", code)
	}
}
