// Package service implements the device application logic: validating and
// registering devices, listing/fetching them, updating presence via heartbeats,
// and revoking them (which cascades to their pairings).
package service

import (
	"context"
	"encoding/base64"
	"errors"
	"log/slog"
	"strings"

	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/services/device/internal/domain"
)

// x25519KeyLen is the byte length of an X25519 public key.
const x25519KeyLen = 32

// maxNameLen bounds a device's display name.
const maxNameLen = 120

// Config configures the Service.
type Config struct {
	Repo   domain.Repository
	Logger *slog.Logger
}

// Service is the device application service.
type Service struct {
	repo domain.Repository
	log  *slog.Logger
}

// New builds a Service.
func New(c Config) *Service {
	log := c.Logger
	if log == nil {
		log = slog.Default()
	}
	return &Service{repo: c.Repo, log: log}
}

// Register validates and persists a device registration for the user.
func (s *Service) Register(ctx context.Context, userID string, reg domain.Registration) (domain.Device, error) {
	reg.UserID = userID
	reg.Name = strings.TrimSpace(reg.Name)

	if !validKind(reg.Kind) {
		return domain.Device{}, apperr.New(apperr.CodeInvalidInput, "kind must be desktop or mobile")
	}
	if !validPlatform(reg.Platform) {
		return domain.Device{}, apperr.New(apperr.CodeInvalidInput, "platform is invalid")
	}
	if reg.Name == "" || len(reg.Name) > maxNameLen {
		return domain.Device{}, apperr.New(apperr.CodeInvalidInput, "name is required and must be at most 120 characters")
	}
	if !validPublicKey(reg.PublicKey) {
		return domain.Device{}, apperr.New(apperr.CodeInvalidInput, "public_key must be a base64-encoded 32-byte X25519 key")
	}

	device, err := s.repo.Register(ctx, reg)
	if err != nil {
		if errors.Is(err, domain.ErrPublicKeyTaken) {
			return domain.Device{}, apperr.New(apperr.CodeConflict, "device public key is already registered")
		}
		return domain.Device{}, apperr.Wrap(apperr.CodeInternal, "failed to register device", err)
	}
	s.log.Info("device registered",
		slog.String("device_id", device.ID), slog.String("kind", string(device.Kind)))
	return device, nil
}

// List returns the user's active devices.
func (s *Service) List(ctx context.Context, userID string) ([]domain.Device, error) {
	devices, err := s.repo.List(ctx, userID)
	if err != nil {
		return nil, apperr.Wrap(apperr.CodeInternal, "failed to list devices", err)
	}
	return devices, nil
}

// Get returns a single active device owned by the user.
func (s *Service) Get(ctx context.Context, userID, id string) (domain.Device, error) {
	device, err := s.repo.Get(ctx, id, userID)
	if err != nil {
		if errors.Is(err, domain.ErrDeviceNotFound) {
			return domain.Device{}, apperr.New(apperr.CodeNotFound, "device not found")
		}
		return domain.Device{}, apperr.Wrap(apperr.CodeInternal, "failed to load device", err)
	}
	return device, nil
}

// Revoke removes a device and cascades revocation to its pairings.
func (s *Service) Revoke(ctx context.Context, userID, id string) error {
	if err := s.repo.Revoke(ctx, id, userID); err != nil {
		if errors.Is(err, domain.ErrDeviceNotFound) {
			return apperr.New(apperr.CodeNotFound, "device not found")
		}
		return apperr.Wrap(apperr.CodeInternal, "failed to revoke device", err)
	}
	s.log.Info("device revoked", slog.String("device_id", id))
	return nil
}

// Heartbeat updates a device's presence.
func (s *Service) Heartbeat(ctx context.Context, userID, id string, status domain.Status) (domain.Device, error) {
	if status != domain.StatusOnline && status != domain.StatusOffline {
		status = domain.StatusOnline
	}
	device, err := s.repo.Heartbeat(ctx, id, userID, status)
	if err != nil {
		if errors.Is(err, domain.ErrDeviceNotFound) {
			return domain.Device{}, apperr.New(apperr.CodeNotFound, "device not found")
		}
		return domain.Device{}, apperr.Wrap(apperr.CodeInternal, "failed to update device presence", err)
	}
	return device, nil
}

func validKind(k domain.Kind) bool {
	return k == domain.KindDesktop || k == domain.KindMobile
}

func validPlatform(p domain.Platform) bool {
	switch p {
	case domain.PlatformWindows, domain.PlatformMacOS, domain.PlatformLinux,
		domain.PlatformAndroid, domain.PlatformIOS:
		return true
	default:
		return false
	}
}

// validPublicKey accepts a standard or raw base64 encoding of a 32-byte key.
func validPublicKey(key string) bool {
	key = strings.TrimSpace(key)
	if key == "" {
		return false
	}
	for _, enc := range []*base64.Encoding{base64.StdEncoding, base64.RawStdEncoding} {
		if b, err := enc.DecodeString(key); err == nil && len(b) == x25519KeyLen {
			return true
		}
	}
	return false
}
