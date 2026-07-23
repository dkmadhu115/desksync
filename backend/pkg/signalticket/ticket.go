// Package signalticket issues and verifies short-lived, HMAC-signed tickets
// that authorize a WebSocket upgrade to the signaling service.
//
// The session service issues a ticket when a client creates a session; the
// signaling service verifies it on connect. A ticket binds a specific session,
// user, and role (controller vs agent) and expires quickly, so a leaked
// signaling URL cannot be replayed and a client cannot join a session it does
// not own. Tickets are self-contained (no shared datastore lookup needed),
// which keeps the signaling service stateless and horizontally scalable.
//
// Format: "v1.<base64url(payload)>.<base64url(hmac-sha256(secret, payload))>".
// The signature is compared in constant time.
package signalticket

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
)

// Role identifies which side of a session a ticket authorizes.
type Role string

const (
	// RoleController is the mobile client that drives the desktop.
	RoleController Role = "controller"
	// RoleAgent is the desktop agent being controlled.
	RoleAgent Role = "agent"
)

// Valid reports whether r is a known role.
func (r Role) Valid() bool { return r == RoleController || r == RoleAgent }

// Errors returned by Verify.
var (
	ErrMalformed = errors.New("signalticket: malformed ticket")
	ErrSignature = errors.New("signalticket: signature mismatch")
	ErrExpired   = errors.New("signalticket: ticket expired")
	ErrRole      = errors.New("signalticket: invalid role")
)

// Ticket is the decoded ticket payload.
type Ticket struct {
	SessionID string `json:"sid"`
	UserID    string `json:"uid"`
	Role      Role   `json:"role"`
	// ExpiresAt is Unix seconds.
	ExpiresAt int64 `json:"exp"`
}

var b64 = base64.RawURLEncoding

// Issuer mints tickets with a fixed secret and TTL.
type Issuer struct {
	secret []byte
	ttl    time.Duration
	now    func() time.Time
}

// NewIssuer builds an Issuer. It errors if the secret is too short or the TTL
// is non-positive, so services cannot boot with an insecure configuration.
func NewIssuer(secret string, ttl time.Duration) (*Issuer, error) {
	if len(secret) < 16 {
		return nil, errors.New("signalticket: secret must be >= 16 bytes")
	}
	if ttl <= 0 {
		return nil, errors.New("signalticket: ttl must be positive")
	}
	return &Issuer{secret: []byte(secret), ttl: ttl, now: time.Now}, nil
}

// TTL exposes the configured ticket lifetime.
func (i *Issuer) TTL() time.Duration { return i.ttl }

// Issue mints a ticket binding the session, user, and role.
func (i *Issuer) Issue(sessionID, userID string, role Role) (string, error) {
	if !role.Valid() {
		return "", ErrRole
	}
	t := Ticket{
		SessionID: sessionID,
		UserID:    userID,
		Role:      role,
		ExpiresAt: i.now().Add(i.ttl).Unix(),
	}
	payload, err := json.Marshal(t)
	if err != nil {
		return "", fmt.Errorf("signalticket: marshal: %w", err)
	}
	sig := sign(i.secret, payload)
	return "v1." + b64.EncodeToString(payload) + "." + b64.EncodeToString(sig), nil
}

// Verifier validates tickets minted with the same secret.
type Verifier struct {
	secret []byte
	now    func() time.Time
}

// NewVerifier builds a Verifier.
func NewVerifier(secret string) (*Verifier, error) {
	if len(secret) < 16 {
		return nil, errors.New("signalticket: secret must be >= 16 bytes")
	}
	return &Verifier{secret: []byte(secret), now: time.Now}, nil
}

// Verify checks the ticket's signature and expiry and returns the payload.
func (v *Verifier) Verify(token string) (*Ticket, error) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 || parts[0] != "v1" {
		return nil, ErrMalformed
	}
	payload, err := b64.DecodeString(parts[1])
	if err != nil {
		return nil, ErrMalformed
	}
	sig, err := b64.DecodeString(parts[2])
	if err != nil {
		return nil, ErrMalformed
	}
	expected := sign(v.secret, payload)
	if !hmac.Equal(sig, expected) {
		return nil, ErrSignature
	}
	var t Ticket
	if err := json.Unmarshal(payload, &t); err != nil {
		return nil, ErrMalformed
	}
	if !t.Role.Valid() {
		return nil, ErrRole
	}
	if v.now().Unix() > t.ExpiresAt {
		return nil, ErrExpired
	}
	return &t, nil
}

func sign(secret, payload []byte) []byte {
	mac := hmac.New(sha256.New, secret)
	mac.Write(payload)
	return mac.Sum(nil)
}
