// Package devicecert issues and verifies DeskSync device certificates.
//
// A device certificate is a short, self-describing token that binds a device's
// identity (device id, owner, X25519 public key, kind) to a validity window,
// signed by the backend's Ed25519 certificate authority. It is used to
// mutually authenticate the agent<->backend and controller<->agent channels:
// a peer presents its certificate, and the other side verifies the CA
// signature and validity window offline (no per-request DB lookup needed for
// the cryptographic check; revocation is layered on top via the database).
//
// The token format is a compact JWS-like structure:
//
//	base64url(payloadJSON) "." base64url(ed25519-signature)
//
// This keeps certificates human-inspectable, dependency-free (standard library
// crypto only), and cheap to verify, while the Ed25519 signature makes them
// unforgeable without the CA private key (which never leaves the backend).
package devicecert

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
)

// Errors returned by verification.
var (
	// ErrMalformed indicates the token is not a well-formed certificate.
	ErrMalformed = errors.New("devicecert: malformed certificate")
	// ErrBadSignature indicates the CA signature did not verify.
	ErrBadSignature = errors.New("devicecert: signature verification failed")
	// ErrExpired indicates the certificate's validity window has passed.
	ErrExpired = errors.New("devicecert: certificate expired")
	// ErrNotYetValid indicates the certificate is not valid yet.
	ErrNotYetValid = errors.New("devicecert: certificate not yet valid")
)

// tokenVersion is the certificate payload schema version.
const tokenVersion = 1

// Claims is the signed payload of a device certificate.
type Claims struct {
	// Version of the certificate schema.
	Version int `json:"v"`
	// Serial uniquely identifies this certificate.
	Serial string `json:"serial"`
	// DeviceID is the device this certificate is bound to.
	DeviceID string `json:"device_id"`
	// UserID is the owning user.
	UserID string `json:"user_id"`
	// Kind is "desktop" or "mobile".
	Kind string `json:"kind"`
	// PublicKey is the device's base64 X25519 public key.
	PublicKey string `json:"public_key"`
	// NotBefore is the Unix seconds from which the certificate is valid.
	NotBefore int64 `json:"nbf"`
	// NotAfter is the Unix seconds after which the certificate is invalid.
	NotAfter int64 `json:"exp"`
}

// Certificate is a freshly issued device certificate plus derived metadata that
// the caller persists (see the device_certificates table).
type Certificate struct {
	// Claims is the signed payload.
	Claims Claims
	// Token is the encoded certificate string (stored as certificate_pem).
	Token string
	// Fingerprint is the hex SHA-256 of the token (stored as fingerprint_sha256).
	Fingerprint string
	// NotBefore / NotAfter as time.Time for convenience.
	NotBefore time.Time
	NotAfter  time.Time
}

// Issuer mints device certificates with the CA private key.
type Issuer struct {
	priv ed25519.PrivateKey
	ttl  time.Duration
	now  func() time.Time
}

// IssuerConfig configures an Issuer.
type IssuerConfig struct {
	// PrivateKey is the CA Ed25519 private key.
	PrivateKey ed25519.PrivateKey
	// TTL is the certificate lifetime (defaults to 90 days).
	TTL time.Duration
	// Now overrides the clock (tests).
	Now func() time.Time
}

// NewIssuer builds an Issuer, validating the key.
func NewIssuer(c IssuerConfig) (*Issuer, error) {
	if len(c.PrivateKey) != ed25519.PrivateKeySize {
		return nil, fmt.Errorf("devicecert: invalid CA private key size %d", len(c.PrivateKey))
	}
	ttl := c.TTL
	if ttl <= 0 {
		ttl = 90 * 24 * time.Hour
	}
	now := c.Now
	if now == nil {
		now = time.Now
	}
	return &Issuer{priv: c.PrivateKey, ttl: ttl, now: now}, nil
}

// Subject identifies the device a certificate is issued for.
type Subject struct {
	DeviceID  string
	UserID    string
	Kind      string
	PublicKey string
}

// Issue signs a certificate for the subject.
func (i *Issuer) Issue(s Subject) (Certificate, error) {
	if s.DeviceID == "" || s.UserID == "" || s.PublicKey == "" {
		return Certificate{}, errors.New("devicecert: device_id, user_id and public_key are required")
	}
	serial, err := randomSerial()
	if err != nil {
		return Certificate{}, err
	}
	nbf := i.now().Add(-30 * time.Second) // small backdate for clock skew
	exp := i.now().Add(i.ttl)
	claims := Claims{
		Version:   tokenVersion,
		Serial:    serial,
		DeviceID:  s.DeviceID,
		UserID:    s.UserID,
		Kind:      s.Kind,
		PublicKey: s.PublicKey,
		NotBefore: nbf.Unix(),
		NotAfter:  exp.Unix(),
	}
	token, err := encodeAndSign(claims, i.priv)
	if err != nil {
		return Certificate{}, err
	}
	return Certificate{
		Claims:      claims,
		Token:       token,
		Fingerprint: Fingerprint(token),
		NotBefore:   time.Unix(claims.NotBefore, 0).UTC(),
		NotAfter:    time.Unix(claims.NotAfter, 0).UTC(),
	}, nil
}

// Verifier checks certificates against the CA public key.
type Verifier struct {
	pub ed25519.PublicKey
}

// NewVerifier builds a Verifier.
func NewVerifier(pub ed25519.PublicKey) (*Verifier, error) {
	if len(pub) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("devicecert: invalid CA public key size %d", len(pub))
	}
	return &Verifier{pub: pub}, nil
}

// Verify checks the signature and validity window at time `at`, returning the
// claims on success. Revocation (device_certificates.revoked_at) is enforced by
// the caller against the database; this performs the cryptographic checks.
func (v *Verifier) Verify(token string, at time.Time) (Claims, error) {
	claims, err := v.verifySignature(token)
	if err != nil {
		return Claims{}, err
	}
	at = at.UTC()
	if at.Unix() < claims.NotBefore {
		return Claims{}, ErrNotYetValid
	}
	if at.Unix() > claims.NotAfter {
		return Claims{}, ErrExpired
	}
	return claims, nil
}

func (v *Verifier) verifySignature(token string) (Claims, error) {
	payloadPart, sigPart, ok := strings.Cut(token, ".")
	if !ok {
		return Claims{}, ErrMalformed
	}
	payload, err := base64.RawURLEncoding.DecodeString(payloadPart)
	if err != nil {
		return Claims{}, ErrMalformed
	}
	sig, err := base64.RawURLEncoding.DecodeString(sigPart)
	if err != nil {
		return Claims{}, ErrMalformed
	}
	if !ed25519.Verify(v.pub, payload, sig) {
		return Claims{}, ErrBadSignature
	}
	var claims Claims
	if err := json.Unmarshal(payload, &claims); err != nil {
		return Claims{}, ErrMalformed
	}
	if claims.Version != tokenVersion {
		return Claims{}, ErrMalformed
	}
	return claims, nil
}

// Fingerprint returns the hex-encoded SHA-256 of a certificate token.
func Fingerprint(token string) string {
	sum := sha256.Sum256([]byte(token))
	return hex.EncodeToString(sum[:])
}

// EqualFingerprint compares two fingerprints in constant time.
func EqualFingerprint(a, b string) bool {
	return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1
}

// GenerateCA creates a new Ed25519 CA keypair (for bootstrapping/tests).
func GenerateCA() (ed25519.PublicKey, ed25519.PrivateKey, error) {
	return ed25519.GenerateKey(rand.Reader)
}

func encodeAndSign(claims Claims, priv ed25519.PrivateKey) (string, error) {
	payload, err := json.Marshal(claims)
	if err != nil {
		return "", fmt.Errorf("devicecert: marshal claims: %w", err)
	}
	sig := ed25519.Sign(priv, payload)
	return base64.RawURLEncoding.EncodeToString(payload) + "." + base64.RawURLEncoding.EncodeToString(sig), nil
}

func randomSerial() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("devicecert: read serial: %w", err)
	}
	return hex.EncodeToString(b), nil
}
