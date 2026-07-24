package service

import (
	"context"
	"encoding/base64"
	"testing"
	"time"

	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/services/device/internal/domain"
)

// validKey is a base64-encoded 32-byte (all-zero) X25519 public key.
var validKey = base64.StdEncoding.EncodeToString(make([]byte, x25519KeyLen))

// --- Fake repository ---

type fakeRepo struct {
	registered domain.Device
	registerErr error
	devices    []domain.Device
	revoked    []string
	notFound   bool
}

func (f *fakeRepo) Register(_ context.Context, r domain.Registration) (domain.Device, error) {
	if f.registerErr != nil {
		return domain.Device{}, f.registerErr
	}
	d := domain.Device{
		ID:        "dev-1",
		UserID:    r.UserID,
		Kind:      r.Kind,
		Platform:  r.Platform,
		Name:      r.Name,
		PublicKey: r.PublicKey,
		Status:    domain.StatusOffline,
		CreatedAt: time.Now(),
		UpdatedAt: time.Now(),
	}
	f.registered = d
	return d, nil
}

func (f *fakeRepo) Get(_ context.Context, id, _ string) (domain.Device, error) {
	if f.notFound || id != f.registered.ID {
		return domain.Device{}, domain.ErrDeviceNotFound
	}
	return f.registered, nil
}

func (f *fakeRepo) List(_ context.Context, _ string) ([]domain.Device, error) {
	return f.devices, nil
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
	now := time.Now()
	return domain.Device{ID: id, Status: status, LastSeenAt: &now}, nil
}

func newService(repo domain.Repository) *Service {
	return New(Config{Repo: repo})
}

func validRegistration() domain.Registration {
	return domain.Registration{
		Kind:      domain.KindMobile,
		Platform:  domain.PlatformIOS,
		Name:      "My Phone",
		PublicKey: validKey,
	}
}

func TestRegisterSuccess(t *testing.T) {
	repo := &fakeRepo{}
	svc := newService(repo)

	d, err := svc.Register(context.Background(), "user-1", validRegistration())
	if err != nil {
		t.Fatalf("Register: %v", err)
	}
	if d.ID != "dev-1" || d.UserID != "user-1" {
		t.Fatalf("unexpected device %+v", d)
	}
	if repo.registered.Kind != domain.KindMobile {
		t.Fatalf("kind not persisted: %+v", repo.registered)
	}
}

func TestRegisterValidation(t *testing.T) {
	svc := newService(&fakeRepo{})
	cases := map[string]func(r *domain.Registration){
		"bad kind":     func(r *domain.Registration) { r.Kind = "phone" },
		"bad platform": func(r *domain.Registration) { r.Platform = "symbian" },
		"empty name":   func(r *domain.Registration) { r.Name = "   " },
		"short key":    func(r *domain.Registration) { r.PublicKey = "abc" },
		"non base64":   func(r *domain.Registration) { r.PublicKey = "!!!not-base64!!!" },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			reg := validRegistration()
			mutate(&reg)
			_, err := svc.Register(context.Background(), "user-1", reg)
			assertCode(t, err, apperr.CodeInvalidInput)
		})
	}
}

func TestRegisterPublicKeyConflict(t *testing.T) {
	repo := &fakeRepo{registerErr: domain.ErrPublicKeyTaken}
	svc := newService(repo)
	_, err := svc.Register(context.Background(), "user-1", validRegistration())
	assertCode(t, err, apperr.CodeConflict)
}

func TestGetNotFound(t *testing.T) {
	svc := newService(&fakeRepo{notFound: true})
	_, err := svc.Get(context.Background(), "user-1", "missing")
	assertCode(t, err, apperr.CodeNotFound)
}

func TestRevokeSuccess(t *testing.T) {
	repo := &fakeRepo{}
	svc := newService(repo)
	if err := svc.Revoke(context.Background(), "user-1", "dev-1"); err != nil {
		t.Fatalf("Revoke: %v", err)
	}
	if len(repo.revoked) != 1 || repo.revoked[0] != "dev-1" {
		t.Fatalf("expected dev-1 revoked, got %v", repo.revoked)
	}
}

func TestRevokeNotFound(t *testing.T) {
	svc := newService(&fakeRepo{notFound: true})
	err := svc.Revoke(context.Background(), "user-1", "missing")
	assertCode(t, err, apperr.CodeNotFound)
}

func TestHeartbeatDefaultsOnline(t *testing.T) {
	svc := newService(&fakeRepo{})
	d, err := svc.Heartbeat(context.Background(), "user-1", "dev-1", domain.Status("garbage"))
	if err != nil {
		t.Fatalf("Heartbeat: %v", err)
	}
	if d.Status != domain.StatusOnline {
		t.Fatalf("status = %q, want online", d.Status)
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
