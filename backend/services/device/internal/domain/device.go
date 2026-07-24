// Package domain defines the device service's core entities and the repository
// interface the application layer depends on. A device is a laptop running the
// agent or a mobile client; the server stores only its public key (private keys
// never leave the device) plus presence and lifecycle metadata.
package domain

import (
	"context"
	"errors"
	"time"
)

// Kind distinguishes controllable desktops from mobile controllers.
type Kind string

const (
	// KindDesktop is a laptop/desktop running the agent.
	KindDesktop Kind = "desktop"
	// KindMobile is a phone/tablet controller.
	KindMobile Kind = "mobile"
)

// Platform is the operating-system family a device runs.
type Platform string

const (
	// PlatformWindows is Microsoft Windows.
	PlatformWindows Platform = "windows"
	// PlatformMacOS is Apple macOS.
	PlatformMacOS Platform = "macos"
	// PlatformLinux is Linux.
	PlatformLinux Platform = "linux"
	// PlatformAndroid is Google Android.
	PlatformAndroid Platform = "android"
	// PlatformIOS is Apple iOS.
	PlatformIOS Platform = "ios"
)

// Status is a device's presence.
type Status string

const (
	// StatusOnline means the device is currently connected.
	StatusOnline Status = "online"
	// StatusOffline means the device is not currently connected.
	StatusOffline Status = "offline"
)

// Device is a registered laptop or phone belonging to a user.
type Device struct {
	ID         string
	UserID     string
	Kind       Kind
	Platform   Platform
	Name       string
	PublicKey  string
	Status     Status
	LastSeenAt *time.Time
	FCMToken   *string
	CreatedAt  time.Time
	UpdatedAt  time.Time
}

// Registration carries the fields a client supplies when registering (or
// re-registering) a device.
type Registration struct {
	UserID    string
	Kind      Kind
	Platform  Platform
	Name      string
	PublicKey string
	FCMToken  *string
}

// Sentinel errors surfaced by the repository/service.
var (
	// ErrDeviceNotFound means no active device matched the id for the user.
	ErrDeviceNotFound = errors.New("device not found")
	// ErrPublicKeyTaken means the public key is already registered to a
	// different user (device public keys are unique across the fleet).
	ErrPublicKeyTaken = errors.New("public key already registered")
)

// Repository is the persistence port for devices.
type Repository interface {
	// Register inserts a device or, when the same user re-registers an existing
	// public key, updates and un-revokes it (idempotent registration).
	Register(ctx context.Context, r Registration) (Device, error)
	// Get returns an active device owned by the user.
	Get(ctx context.Context, id, userID string) (Device, error)
	// List returns the user's active devices, newest first.
	List(ctx context.Context, userID string) ([]Device, error)
	// Revoke soft-deletes the device and revokes any pairings referencing it.
	Revoke(ctx context.Context, id, userID string) error
	// Heartbeat updates a device's presence and last-seen timestamp.
	Heartbeat(ctx context.Context, id, userID string, status Status) (Device, error)
}
