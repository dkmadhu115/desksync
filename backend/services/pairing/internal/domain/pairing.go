// Package domain defines the pairing service's core entities and the ports the
// application layer depends on. Pairing establishes a persistent trust
// relationship between a mobile controller and a desktop agent via a QR code or
// a short-lived manual code.
package domain

import (
	"context"
	"errors"
	"time"
)

// Status is the lifecycle state of a persistent pairing.
type Status string

const (
	// StatusPending means initiated but not yet confirmed by the mobile.
	StatusPending Status = "pending"
	// StatusActive means confirmed and usable.
	StatusActive Status = "active"
	// StatusRevoked means the pairing was revoked and can no longer be used.
	StatusRevoked Status = "revoked"
)

// Kind mirrors the device kind used for authorization checks.
type Kind string

const (
	// KindDesktop is a controllable desktop/laptop.
	KindDesktop Kind = "desktop"
	// KindMobile is a mobile controller.
	KindMobile Kind = "mobile"
)

// Pairing is a persistent trust relationship between two devices.
type Pairing struct {
	ID              string
	UserID          string
	MobileDeviceID  string
	DesktopDeviceID string
	Status          Status
	Trusted         bool
	CreatedAt       time.Time
	ConfirmedAt     *time.Time
}

// DeviceRef is the minimal device view needed to authorize a pairing.
type DeviceRef struct {
	ID     string
	UserID string
	Kind   Kind
}

// Challenge is the ephemeral pairing challenge held in Redis between initiate
// and confirm. The plaintext code is never stored; only its hash.
type Challenge struct {
	PairingID       string    `json:"pairing_id"`
	UserID          string    `json:"user_id"`
	DesktopDeviceID string    `json:"desktop_device_id"`
	CodeHash        string    `json:"code_hash"`
	ExpiresAt       time.Time `json:"expires_at"`
}

// Sentinel errors surfaced by the ports/service.
var (
	// ErrChallengeNotFound means no pending challenge matched the pairing id
	// (unknown, already consumed, or expired).
	ErrChallengeNotFound = errors.New("pairing challenge not found")
	// ErrDeviceNotFound means the referenced device does not exist for the user.
	ErrDeviceNotFound = errors.New("device not found")
	// ErrPairingNotFound means no matching pairing exists for the user.
	ErrPairingNotFound = errors.New("pairing not found")
)

// Repository is the persistent (PostgreSQL) port for pairings and the device
// lookups pairing needs for authorization.
type Repository interface {
	// DeviceForUser returns a device owned by the user (nil error), or
	// ErrDeviceNotFound.
	DeviceForUser(ctx context.Context, deviceID, userID string) (DeviceRef, error)
	// UpsertActivePairing creates (or reactivates) an active, trusted pairing
	// between the two devices and returns it.
	UpsertActivePairing(ctx context.Context, userID, mobileDeviceID, desktopDeviceID string) (Pairing, error)
	// ListPairings returns the user's non-revoked pairings, newest first.
	ListPairings(ctx context.Context, userID string) ([]Pairing, error)
	// RevokePairing marks a user-owned pairing revoked; ErrPairingNotFound when
	// none matched.
	RevokePairing(ctx context.Context, id, userID string) error
}

// ChallengeStore is the ephemeral (Redis) port for pending pairing challenges.
type ChallengeStore interface {
	// Save stores a challenge with the given time-to-live and resets its failed
	// attempt counter.
	Save(ctx context.Context, ch Challenge, ttl time.Duration) error
	// Get returns a pending challenge or ErrChallengeNotFound.
	Get(ctx context.Context, pairingID string) (Challenge, error)
	// RecordFailedAttempt increments and returns the failed-attempt count for a
	// challenge.
	RecordFailedAttempt(ctx context.Context, pairingID string) (int, error)
	// Consume deletes a challenge and its attempt counter (one-time use).
	Consume(ctx context.Context, pairingID string) error
}
