// Package service implements the pairing application logic: minting short-lived
// QR/manual-code challenges for a desktop, and confirming them from a mobile to
// establish a persistent, trusted pairing. Challenge codes are hashed, expiring,
// single-use, and rate-limited.
package service

import (
	"context"
	"errors"
	"log/slog"
	"net/url"
	"strings"
	"time"

	"github.com/desksync/backend/pkg/crypto"
	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/services/pairing/internal/domain"
	"github.com/google/uuid"
)

// qrScheme/qrHost/qrVersion define the deep link encoded into the QR code and
// parsed by the mobile client.
const (
	qrScheme  = "desksync"
	qrHost    = "pair"
	qrVersion = "1"
)

// Config configures the Service.
type Config struct {
	Repo        domain.Repository
	Store       domain.ChallengeStore
	CodeTTL     time.Duration
	CodeDigits  int
	MaxAttempts int
	Logger      *slog.Logger
	// Now is injectable for deterministic tests; defaults to time.Now.
	Now func() time.Time
}

// Service is the pairing application service.
type Service struct {
	repo        domain.Repository
	store       domain.ChallengeStore
	codeTTL     time.Duration
	codeDigits  int
	maxAttempts int
	log         *slog.Logger
	now         func() time.Time
}

// New builds a Service with sensible defaults.
func New(c Config) *Service {
	ttl := c.CodeTTL
	if ttl <= 0 {
		ttl = 5 * time.Minute
	}
	digits := c.CodeDigits
	if digits <= 0 {
		digits = 8
	}
	attempts := c.MaxAttempts
	if attempts <= 0 {
		attempts = 5
	}
	log := c.Logger
	if log == nil {
		log = slog.Default()
	}
	now := c.Now
	if now == nil {
		now = time.Now
	}
	return &Service{
		repo:        c.Repo,
		store:       c.Store,
		codeTTL:     ttl,
		codeDigits:  digits,
		maxAttempts: attempts,
		log:         log,
		now:         now,
	}
}

// Challenge is the result of initiating a pairing: what the desktop displays.
type Challenge struct {
	PairingID  string
	QRPayload  string
	ManualCode string
	ExpiresAt  time.Time
}

// Initiate mints a pairing challenge for one of the user's desktop devices.
func (s *Service) Initiate(ctx context.Context, userID, desktopDeviceID string) (Challenge, error) {
	if strings.TrimSpace(desktopDeviceID) == "" {
		return Challenge{}, apperr.New(apperr.CodeInvalidInput, "desktop_device_id is required")
	}

	dev, err := s.repo.DeviceForUser(ctx, desktopDeviceID, userID)
	if err != nil {
		if errors.Is(err, domain.ErrDeviceNotFound) {
			return Challenge{}, apperr.New(apperr.CodeNotFound, "desktop device not found")
		}
		return Challenge{}, apperr.Wrap(apperr.CodeInternal, "failed to load device", err)
	}
	if dev.Kind != domain.KindDesktop {
		return Challenge{}, apperr.New(apperr.CodeInvalidInput, "device is not a desktop")
	}

	code, err := crypto.GenerateNumericCode(s.codeDigits)
	if err != nil {
		return Challenge{}, apperr.Wrap(apperr.CodeInternal, "failed to generate code", err)
	}
	pairingID := uuid.NewString()
	expiresAt := s.now().Add(s.codeTTL)

	ch := domain.Challenge{
		PairingID:       pairingID,
		UserID:          userID,
		DesktopDeviceID: desktopDeviceID,
		CodeHash:        crypto.HashToken(code),
		ExpiresAt:       expiresAt,
	}
	if err := s.store.Save(ctx, ch, s.codeTTL); err != nil {
		return Challenge{}, apperr.Wrap(apperr.CodeInternal, "failed to store challenge", err)
	}

	s.log.Info("pairing initiated",
		slog.String("pairing_id", pairingID), slog.String("desktop_device_id", desktopDeviceID))

	return Challenge{
		PairingID:  pairingID,
		QRPayload:  buildQRPayload(pairingID, code),
		ManualCode: code,
		ExpiresAt:  expiresAt,
	}, nil
}

// invalidCode is the client-safe error returned for any confirm failure related
// to the code/challenge, deliberately generic to avoid oracles.
func invalidCode() error {
	return apperr.New(apperr.CodeInvalidInput, "invalid or expired pairing code")
}

// Confirm validates a challenge from the mobile device and, on success, creates
// a persistent trusted pairing.
func (s *Service) Confirm(ctx context.Context, userID, pairingID, code, mobileDeviceID string) (domain.Pairing, error) {
	if strings.TrimSpace(pairingID) == "" || strings.TrimSpace(code) == "" || strings.TrimSpace(mobileDeviceID) == "" {
		return domain.Pairing{}, apperr.New(apperr.CodeInvalidInput, "pairing_id, code and mobile_device_id are required")
	}

	ch, err := s.store.Get(ctx, pairingID)
	if err != nil {
		if errors.Is(err, domain.ErrChallengeNotFound) {
			return domain.Pairing{}, invalidCode()
		}
		return domain.Pairing{}, apperr.Wrap(apperr.CodeInternal, "failed to load challenge", err)
	}

	// A challenge belongs to a single account; never let another user probe it.
	if ch.UserID != userID {
		return domain.Pairing{}, invalidCode()
	}

	if s.now().After(ch.ExpiresAt) {
		_ = s.store.Consume(ctx, pairingID)
		return domain.Pairing{}, invalidCode()
	}

	if !crypto.EqualTokenHash(code, ch.CodeHash) {
		attempts, aerr := s.store.RecordFailedAttempt(ctx, pairingID)
		if aerr != nil {
			s.log.Warn("failed to record pairing attempt", slog.String("error", aerr.Error()))
		} else if attempts >= s.maxAttempts {
			// Burn the challenge after too many wrong guesses.
			_ = s.store.Consume(ctx, pairingID)
		}
		return domain.Pairing{}, invalidCode()
	}

	dev, err := s.repo.DeviceForUser(ctx, mobileDeviceID, userID)
	if err != nil {
		if errors.Is(err, domain.ErrDeviceNotFound) {
			return domain.Pairing{}, apperr.New(apperr.CodeNotFound, "mobile device not found")
		}
		return domain.Pairing{}, apperr.Wrap(apperr.CodeInternal, "failed to load device", err)
	}
	if dev.Kind != domain.KindMobile {
		return domain.Pairing{}, apperr.New(apperr.CodeInvalidInput, "device is not a mobile device")
	}

	pairing, err := s.repo.UpsertActivePairing(ctx, userID, mobileDeviceID, ch.DesktopDeviceID)
	if err != nil {
		return domain.Pairing{}, apperr.Wrap(apperr.CodeInternal, "failed to persist pairing", err)
	}

	// One-time use: the challenge is spent whether or not the client retries.
	_ = s.store.Consume(ctx, pairingID)

	s.log.Info("pairing confirmed",
		slog.String("pairing_id", pairing.ID), slog.String("mobile_device_id", mobileDeviceID))
	return pairing, nil
}

// List returns the user's non-revoked pairings.
func (s *Service) List(ctx context.Context, userID string) ([]domain.Pairing, error) {
	pairings, err := s.repo.ListPairings(ctx, userID)
	if err != nil {
		return nil, apperr.Wrap(apperr.CodeInternal, "failed to list pairings", err)
	}
	return pairings, nil
}

// Revoke revokes a user-owned pairing.
func (s *Service) Revoke(ctx context.Context, userID, id string) error {
	if err := s.repo.RevokePairing(ctx, id, userID); err != nil {
		if errors.Is(err, domain.ErrPairingNotFound) {
			return apperr.New(apperr.CodeNotFound, "pairing not found")
		}
		return apperr.Wrap(apperr.CodeInternal, "failed to revoke pairing", err)
	}
	s.log.Info("pairing revoked", slog.String("pairing_id", id))
	return nil
}

// buildQRPayload encodes a pairing deep link for the QR code:
// desksync://pair?v=1&pid=<pairing_id>&code=<code>
func buildQRPayload(pairingID, code string) string {
	q := url.Values{}
	q.Set("v", qrVersion)
	q.Set("pid", pairingID)
	q.Set("code", code)
	u := url.URL{Scheme: qrScheme, Host: qrHost, RawQuery: q.Encode()}
	return u.String()
}
