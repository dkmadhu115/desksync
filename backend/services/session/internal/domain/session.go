// Package domain defines the session service's core entities and the
// repository interface the application layer depends on.
package domain

import (
	"context"
	"errors"
	"time"
)

// Status is the lifecycle state of a session.
type Status string

const (
	// StatusInitiating is the initial state before signaling begins.
	StatusInitiating Status = "initiating"
	// StatusConnecting means peers are negotiating the WebRTC connection.
	StatusConnecting Status = "connecting"
	// StatusActive means media/data are flowing.
	StatusActive Status = "active"
	// StatusEnded means the session terminated normally.
	StatusEnded Status = "ended"
	// StatusFailed means the session terminated abnormally.
	StatusFailed Status = "failed"
)

// ConnectionType records how media flowed once known.
type ConnectionType string

const (
	// ConnP2P is a direct peer-to-peer connection.
	ConnP2P ConnectionType = "p2p"
	// ConnRelay is a TURN-relayed connection.
	ConnRelay ConnectionType = "relay"
)

// Session is a remote-control session between a paired mobile and desktop.
type Session struct {
	ID             string
	PairingID      string
	UserID         string
	Status         Status
	ConnectionType *ConnectionType
	StartedAt      time.Time
	EndedAt        *time.Time
	EndReason      *string
	TimeoutSeconds int
	CreatedAt      time.Time
}

// Pairing is the minimal pairing view the session service needs to authorize a
// session (ownership + active status + the two device ids).
type Pairing struct {
	ID              string
	MobileDeviceID  string
	DesktopDeviceID string
	Status          string
}

// Sentinel errors surfaced by the repository/service.
var (
	ErrSessionNotFound = errors.New("session not found")
	ErrPairingNotFound = errors.New("pairing not found")
)

// Repository is the persistence port for sessions.
type Repository interface {
	// PairingForUser returns the pairing if it belongs to the user.
	PairingForUser(ctx context.Context, pairingID, userID string) (Pairing, error)
	// CreateSession inserts a new session and returns it with server defaults.
	CreateSession(ctx context.Context, s Session) (Session, error)
	// GetSession returns a session owned by the user.
	GetSession(ctx context.Context, id, userID string) (Session, error)
	// ListSessions returns the user's most recent sessions (bounded by limit).
	ListSessions(ctx context.Context, userID string, limit int) ([]Session, error)
	// PendingSessionsForDevice returns the user's sessions that are still
	// connecting and belong to a pairing whose desktop device is the given one.
	// It is how a desktop agent discovers incoming sessions to answer.
	//
	// Only sessions started within maxAge are returned. A connect handshake takes
	// seconds, so anything older is a controller that went away mid-connect;
	// without this bound those rows stay 'connecting' forever and every agent
	// poll re-answers them, spawning a WebRTC peer per zombie.
	PendingSessionsForDevice(ctx context.Context, userID, desktopDeviceID string, maxAge time.Duration, limit int) ([]Session, error)
	// EndSession transitions a session to ended (idempotent) and returns it.
	EndSession(ctx context.Context, id, userID, reason string) (Session, error)
	// AppendEvent records an append-only session event.
	AppendEvent(ctx context.Context, sessionID, eventType string, detail map[string]any) error
}
