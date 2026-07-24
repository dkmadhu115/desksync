package service

import (
	"context"
	"net/url"
	"strings"
	"testing"
	"time"

	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/services/pairing/internal/domain"
)

// --- Fakes ---

type fakeStore struct {
	challenges map[string]domain.Challenge
	attempts   map[string]int
}

func newFakeStore() *fakeStore {
	return &fakeStore{challenges: map[string]domain.Challenge{}, attempts: map[string]int{}}
}

func (s *fakeStore) Save(_ context.Context, ch domain.Challenge, _ time.Duration) error {
	s.challenges[ch.PairingID] = ch
	delete(s.attempts, ch.PairingID)
	return nil
}

func (s *fakeStore) Get(_ context.Context, pairingID string) (domain.Challenge, error) {
	ch, ok := s.challenges[pairingID]
	if !ok {
		return domain.Challenge{}, domain.ErrChallengeNotFound
	}
	return ch, nil
}

func (s *fakeStore) RecordFailedAttempt(_ context.Context, pairingID string) (int, error) {
	s.attempts[pairingID]++
	return s.attempts[pairingID], nil
}

func (s *fakeStore) Consume(_ context.Context, pairingID string) error {
	delete(s.challenges, pairingID)
	delete(s.attempts, pairingID)
	return nil
}

type fakeRepo struct {
	devices  map[string]domain.DeviceRef
	upserted domain.Pairing
	pairings []domain.Pairing
	revoked  []string
	notFound bool
}

func (r *fakeRepo) DeviceForUser(_ context.Context, deviceID, userID string) (domain.DeviceRef, error) {
	d, ok := r.devices[deviceID]
	if !ok || d.UserID != userID {
		return domain.DeviceRef{}, domain.ErrDeviceNotFound
	}
	return d, nil
}

func (r *fakeRepo) UpsertActivePairing(_ context.Context, userID, mobileDeviceID, desktopDeviceID string) (domain.Pairing, error) {
	now := time.Now()
	r.upserted = domain.Pairing{
		ID:              "pair-1",
		UserID:          userID,
		MobileDeviceID:  mobileDeviceID,
		DesktopDeviceID: desktopDeviceID,
		Status:          domain.StatusActive,
		Trusted:         true,
		CreatedAt:       now,
		ConfirmedAt:     &now,
	}
	return r.upserted, nil
}

func (r *fakeRepo) ListPairings(_ context.Context, _ string) ([]domain.Pairing, error) {
	return r.pairings, nil
}

func (r *fakeRepo) RevokePairing(_ context.Context, id, _ string) error {
	if r.notFound {
		return domain.ErrPairingNotFound
	}
	r.revoked = append(r.revoked, id)
	return nil
}

func newService(repo domain.Repository, st domain.ChallengeStore) *Service {
	return New(Config{
		Repo:    repo,
		Store:   st,
		CodeTTL: 5 * time.Minute,
		Now:     func() time.Time { return time.Unix(1_700_000_000, 0) },
	})
}

func seededRepo() *fakeRepo {
	return &fakeRepo{devices: map[string]domain.DeviceRef{
		"desk-1":   {ID: "desk-1", UserID: "user-1", Kind: domain.KindDesktop},
		"mobile-1": {ID: "mobile-1", UserID: "user-1", Kind: domain.KindMobile},
	}}
}

func TestInitiateSuccess(t *testing.T) {
	st := newFakeStore()
	svc := newService(seededRepo(), st)

	ch, err := svc.Initiate(context.Background(), "user-1", "desk-1")
	if err != nil {
		t.Fatalf("Initiate: %v", err)
	}
	if len(ch.ManualCode) != 8 {
		t.Fatalf("manual code = %q, want 8 digits", ch.ManualCode)
	}
	if !strings.HasPrefix(ch.QRPayload, "desksync://pair?") {
		t.Fatalf("unexpected qr payload %q", ch.QRPayload)
	}
	// The QR payload carries the pairing id and code so the mobile can confirm.
	pid, code := parseQR(t, ch.QRPayload)
	if pid != ch.PairingID || code != ch.ManualCode {
		t.Fatalf("qr payload mismatch: pid=%q code=%q", pid, code)
	}
	// The stored challenge holds a hash, never the plaintext code.
	stored := st.challenges[ch.PairingID]
	if stored.CodeHash == "" || stored.CodeHash == ch.ManualCode {
		t.Fatalf("challenge must store a hashed code, got %q", stored.CodeHash)
	}
}

func TestInitiateDeviceNotFound(t *testing.T) {
	svc := newService(seededRepo(), newFakeStore())
	_, err := svc.Initiate(context.Background(), "user-1", "missing")
	assertCode(t, err, apperr.CodeNotFound)
}

func TestInitiateRejectsNonDesktop(t *testing.T) {
	svc := newService(seededRepo(), newFakeStore())
	_, err := svc.Initiate(context.Background(), "user-1", "mobile-1")
	assertCode(t, err, apperr.CodeInvalidInput)
}

func TestConfirmSuccess(t *testing.T) {
	st := newFakeStore()
	repo := seededRepo()
	svc := newService(repo, st)

	ch, err := svc.Initiate(context.Background(), "user-1", "desk-1")
	if err != nil {
		t.Fatalf("Initiate: %v", err)
	}

	pairing, err := svc.Confirm(context.Background(), "user-1", ch.PairingID, ch.ManualCode, "mobile-1")
	if err != nil {
		t.Fatalf("Confirm: %v", err)
	}
	if pairing.Status != domain.StatusActive || !pairing.Trusted {
		t.Fatalf("expected active+trusted pairing, got %+v", pairing)
	}
	if pairing.DesktopDeviceID != "desk-1" || pairing.MobileDeviceID != "mobile-1" {
		t.Fatalf("unexpected devices in pairing %+v", pairing)
	}
	// The challenge is single-use: consumed after success.
	if _, ok := st.challenges[ch.PairingID]; ok {
		t.Fatal("challenge should be consumed after confirm")
	}
}

func TestConfirmWrongCodeThenLockout(t *testing.T) {
	st := newFakeStore()
	svc := New(Config{
		Repo:        seededRepo(),
		Store:       st,
		CodeTTL:     5 * time.Minute,
		MaxAttempts: 3,
		Now:         func() time.Time { return time.Unix(1_700_000_000, 0) },
	})

	ch, _ := svc.Initiate(context.Background(), "user-1", "desk-1")

	for i := 0; i < 3; i++ {
		_, err := svc.Confirm(context.Background(), "user-1", ch.PairingID, "00000000", "mobile-1")
		assertCode(t, err, apperr.CodeInvalidInput)
	}
	// After maxAttempts the challenge is burned, so even the right code fails.
	if _, ok := st.challenges[ch.PairingID]; ok {
		t.Fatal("challenge should be burned after too many attempts")
	}
	_, err := svc.Confirm(context.Background(), "user-1", ch.PairingID, ch.ManualCode, "mobile-1")
	assertCode(t, err, apperr.CodeInvalidInput)
}

func TestConfirmExpired(t *testing.T) {
	st := newFakeStore()
	repo := seededRepo()
	// Initiate at t0, confirm well after the TTL.
	base := time.Unix(1_700_000_000, 0)
	svc := New(Config{Repo: repo, Store: st, CodeTTL: time.Minute, Now: func() time.Time { return base }})
	ch, _ := svc.Initiate(context.Background(), "user-1", "desk-1")

	later := New(Config{Repo: repo, Store: st, CodeTTL: time.Minute, Now: func() time.Time { return base.Add(2 * time.Minute) }})
	_, err := later.Confirm(context.Background(), "user-1", ch.PairingID, ch.ManualCode, "mobile-1")
	assertCode(t, err, apperr.CodeInvalidInput)
}

func TestConfirmForeignUserDenied(t *testing.T) {
	st := newFakeStore()
	svc := newService(seededRepo(), st)
	ch, _ := svc.Initiate(context.Background(), "user-1", "desk-1")

	_, err := svc.Confirm(context.Background(), "attacker", ch.PairingID, ch.ManualCode, "mobile-1")
	assertCode(t, err, apperr.CodeInvalidInput)
	// A foreign probe must not consume the legitimate challenge.
	if _, ok := st.challenges[ch.PairingID]; !ok {
		t.Fatal("foreign probe must not consume the challenge")
	}
}

func TestConfirmMobileDeviceNotFound(t *testing.T) {
	st := newFakeStore()
	svc := newService(seededRepo(), st)
	ch, _ := svc.Initiate(context.Background(), "user-1", "desk-1")

	_, err := svc.Confirm(context.Background(), "user-1", ch.PairingID, ch.ManualCode, "missing")
	assertCode(t, err, apperr.CodeNotFound)
}

func TestRevokeNotFound(t *testing.T) {
	svc := newService(&fakeRepo{notFound: true}, newFakeStore())
	err := svc.Revoke(context.Background(), "user-1", "missing")
	assertCode(t, err, apperr.CodeNotFound)
}

// parseQR extracts the pairing id and code from a desksync://pair deep link.
func parseQR(t *testing.T, payload string) (pid, code string) {
	t.Helper()
	u, err := url.Parse(payload)
	if err != nil {
		t.Fatalf("parse qr: %v", err)
	}
	return u.Query().Get("pid"), u.Query().Get("code")
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
