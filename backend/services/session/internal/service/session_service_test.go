package service

import (
	"context"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/session/internal/domain"
	"github.com/desksync/backend/services/session/internal/ice"
)

// --- Fakes ---

type fakeRepo struct {
	pairing    domain.Pairing
	pairingErr error
	created    domain.Session
	events     []string
}

func (f *fakeRepo) PairingForUser(_ context.Context, _, _ string) (domain.Pairing, error) {
	return f.pairing, f.pairingErr
}

func (f *fakeRepo) CreateSession(_ context.Context, s domain.Session) (domain.Session, error) {
	s.ID = "sess-1"
	s.StartedAt = time.Now()
	s.CreatedAt = s.StartedAt
	f.created = s
	return s, nil
}

func (f *fakeRepo) GetSession(_ context.Context, id, _ string) (domain.Session, error) {
	if id == f.created.ID {
		return f.created, nil
	}
	return domain.Session{}, domain.ErrSessionNotFound
}

func (f *fakeRepo) ListSessions(_ context.Context, _ string, _ int) ([]domain.Session, error) {
	return []domain.Session{f.created}, nil
}

func (f *fakeRepo) PendingSessionsForDevice(_ context.Context, _, _ string, _ int) ([]domain.Session, error) {
	if f.created.ID == "" {
		return nil, nil
	}
	return []domain.Session{f.created}, nil
}

func (f *fakeRepo) EndSession(_ context.Context, id, _, _ string) (domain.Session, error) {
	if id != f.created.ID {
		return domain.Session{}, domain.ErrSessionNotFound
	}
	s := f.created
	s.Status = domain.StatusEnded
	return s, nil
}

func (f *fakeRepo) AppendEvent(_ context.Context, _, eventType string, _ map[string]any) error {
	f.events = append(f.events, eventType)
	return nil
}

func newService(t *testing.T, repo domain.Repository) *Service {
	t.Helper()
	issuer, err := signalticket.NewIssuer("test-signaling-secret-0123456789", time.Minute)
	if err != nil {
		t.Fatalf("issuer: %v", err)
	}
	return New(Config{
		Repo:         repo,
		Tickets:      issuer,
		ICE:          ice.NewBuilder(config.ICEConfig{STUNURLs: []string{"stun:stun.example.com:3478"}}),
		SignalingURL: "ws://localhost:8085/api/v1/signaling/ws",
	})
}

func TestCreateSessionSuccess(t *testing.T) {
	repo := &fakeRepo{pairing: domain.Pairing{ID: "p1", Status: "active"}}
	svc := newService(t, repo)

	created, err := svc.CreateSession(context.Background(), "user-1", "p1")
	if err != nil {
		t.Fatalf("CreateSession: %v", err)
	}
	if created.Session.ID != "sess-1" {
		t.Fatalf("unexpected session id %q", created.Session.ID)
	}
	if created.Session.Status != domain.StatusConnecting {
		t.Fatalf("status = %q, want connecting", created.Session.Status)
	}
	if created.SignalingTicket == "" {
		t.Fatal("expected a signaling ticket")
	}
	// The issued ticket must verify and bind the session + controller role.
	ver, _ := signalticket.NewVerifier("test-signaling-secret-0123456789")
	tk, err := ver.Verify(created.SignalingTicket)
	if err != nil {
		t.Fatalf("verify ticket: %v", err)
	}
	if tk.SessionID != "sess-1" || tk.Role != signalticket.RoleController || tk.UserID != "user-1" {
		t.Fatalf("unexpected ticket payload: %+v", tk)
	}
	if len(repo.events) == 0 || repo.events[0] != "created" {
		t.Fatalf("expected a 'created' event, got %v", repo.events)
	}
}

func TestCreateSessionRequiresPairingID(t *testing.T) {
	svc := newService(t, &fakeRepo{})
	_, err := svc.CreateSession(context.Background(), "user-1", "")
	assertCode(t, err, apperr.CodeInvalidInput)
}

func TestCreateSessionPairingNotFound(t *testing.T) {
	repo := &fakeRepo{pairingErr: domain.ErrPairingNotFound}
	svc := newService(t, repo)
	_, err := svc.CreateSession(context.Background(), "user-1", "missing")
	assertCode(t, err, apperr.CodeNotFound)
}

func TestCreateSessionPairingNotActive(t *testing.T) {
	repo := &fakeRepo{pairing: domain.Pairing{ID: "p1", Status: "pending"}}
	svc := newService(t, repo)
	_, err := svc.CreateSession(context.Background(), "user-1", "p1")
	assertCode(t, err, apperr.CodePreconditionF)
}

func TestGetSessionNotFound(t *testing.T) {
	svc := newService(t, &fakeRepo{})
	_, err := svc.GetSession(context.Background(), "user-1", "nope")
	assertCode(t, err, apperr.CodeNotFound)
}

func TestEndSessionIdempotentSuccess(t *testing.T) {
	repo := &fakeRepo{pairing: domain.Pairing{ID: "p1", Status: "active"}}
	svc := newService(t, repo)
	created, _ := svc.CreateSession(context.Background(), "user-1", "p1")

	ended, err := svc.EndSession(context.Background(), "user-1", created.Session.ID, "")
	if err != nil {
		t.Fatalf("EndSession: %v", err)
	}
	if ended.Status != domain.StatusEnded {
		t.Fatalf("status = %q, want ended", ended.Status)
	}
}

func TestPendingForDeviceRequiresDeviceID(t *testing.T) {
	svc := newService(t, &fakeRepo{})
	_, err := svc.PendingForDevice(context.Background(), "user-1", "")
	assertCode(t, err, apperr.CodeInvalidInput)
}

func TestPendingForDeviceIssuesAgentTickets(t *testing.T) {
	repo := &fakeRepo{created: domain.Session{ID: "sess-1", Status: domain.StatusConnecting}}
	svc := newService(t, repo)

	pending, err := svc.PendingForDevice(context.Background(), "user-1", "desktop-1")
	if err != nil {
		t.Fatalf("PendingForDevice: %v", err)
	}
	if len(pending) != 1 {
		t.Fatalf("expected 1 pending session, got %d", len(pending))
	}
	if pending[0].SignalingTicket == "" {
		t.Fatal("expected an agent signaling ticket")
	}
	ver, _ := signalticket.NewVerifier("test-signaling-secret-0123456789")
	tk, err := ver.Verify(pending[0].SignalingTicket)
	if err != nil {
		t.Fatalf("verify ticket: %v", err)
	}
	if tk.SessionID != "sess-1" || tk.Role != signalticket.RoleAgent || tk.UserID != "user-1" {
		t.Fatalf("unexpected ticket payload: %+v", tk)
	}
	if len(pending[0].ICEServers) == 0 {
		t.Fatal("expected ICE servers in the pending response")
	}
}

func TestPendingForDeviceEmpty(t *testing.T) {
	svc := newService(t, &fakeRepo{})
	pending, err := svc.PendingForDevice(context.Background(), "user-1", "desktop-1")
	if err != nil {
		t.Fatalf("PendingForDevice: %v", err)
	}
	if len(pending) != 0 {
		t.Fatalf("expected no pending sessions, got %d", len(pending))
	}
}

func assertCode(t *testing.T, err error, want apperr.Code) {
	t.Helper()
	de, ok := apperr.As(err)
	if !ok {
		t.Fatalf("expected apperr, got %v", err)
	}
	if de.Code != want {
		t.Fatalf("code = %q, want %q", de.Code, want)
	}
}
