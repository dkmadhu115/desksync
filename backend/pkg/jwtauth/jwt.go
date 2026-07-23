// Package jwtauth issues and verifies DeskSync JWTs. It uses separate signing
// secrets for access and refresh tokens so that a leaked access secret cannot
// be used to mint refresh tokens (and vice versa). Refresh tokens carry a JTI
// used for rotation and theft detection.
package jwtauth

import (
	"errors"
	"fmt"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/golang-jwt/jwt/v5"
)

// TokenType distinguishes access from refresh tokens.
type TokenType string

const (
	// AccessToken is a short-lived bearer token for API calls.
	AccessToken TokenType = "access"
	// RefreshToken is a long-lived token used only to mint new access tokens.
	RefreshToken TokenType = "refresh"
)

// Errors returned by verification.
var (
	ErrInvalidToken   = errors.New("jwtauth: invalid token")
	ErrWrongTokenType = errors.New("jwtauth: wrong token type")
)

// Claims is the DeskSync JWT claim set.
type Claims struct {
	jwt.RegisteredClaims
	UserID string    `json:"uid"`
	Type   TokenType `json:"typ"`
}

// Manager issues and verifies tokens using the configured secrets/TTLs.
type Manager struct {
	cfg config.JWTConfig
	now func() time.Time
}

// NewManager builds a Manager. It returns an error if either secret is empty,
// preventing services from booting with insecure defaults.
func NewManager(cfg config.JWTConfig) (*Manager, error) {
	if len(cfg.AccessSecret) < 16 || len(cfg.RefreshSecret) < 16 {
		return nil, errors.New("jwtauth: access and refresh secrets must each be >= 16 bytes")
	}
	return &Manager{cfg: cfg, now: time.Now}, nil
}

// TokenPair is an issued access+refresh pair with the access TTL in seconds.
type TokenPair struct {
	AccessToken  string
	RefreshToken string
	ExpiresIn    int
	RefreshJTI   string
}

// Issue mints a new access+refresh pair for the given user. refreshJTI, when
// non-empty, is used as the refresh token's ID (for rotation chains); otherwise
// a new one derived from the subject/time is expected to be supplied by the
// caller's store. The caller persists the refresh token hash keyed by JTI.
func (m *Manager) Issue(userID, refreshJTI string) (TokenPair, error) {
	now := m.now()

	access, err := m.sign(m.cfg.AccessSecret, Claims{
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer:    m.cfg.Issuer,
			Subject:   userID,
			IssuedAt:  jwt.NewNumericDate(now),
			ExpiresAt: jwt.NewNumericDate(now.Add(m.cfg.AccessTTL)),
		},
		UserID: userID,
		Type:   AccessToken,
	})
	if err != nil {
		return TokenPair{}, err
	}

	refresh, err := m.sign(m.cfg.RefreshSecret, Claims{
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer:    m.cfg.Issuer,
			Subject:   userID,
			ID:        refreshJTI,
			IssuedAt:  jwt.NewNumericDate(now),
			ExpiresAt: jwt.NewNumericDate(now.Add(m.cfg.RefreshTTL)),
		},
		UserID: userID,
		Type:   RefreshToken,
	})
	if err != nil {
		return TokenPair{}, err
	}

	return TokenPair{
		AccessToken:  access,
		RefreshToken: refresh,
		ExpiresIn:    int(m.cfg.AccessTTL.Seconds()),
		RefreshJTI:   refreshJTI,
	}, nil
}

// VerifyAccess validates an access token and returns its claims.
func (m *Manager) VerifyAccess(token string) (*Claims, error) {
	return m.verify(token, m.cfg.AccessSecret, AccessToken)
}

// VerifyRefresh validates a refresh token and returns its claims.
func (m *Manager) VerifyRefresh(token string) (*Claims, error) {
	return m.verify(token, m.cfg.RefreshSecret, RefreshToken)
}

func (m *Manager) sign(secret string, claims Claims) (string, error) {
	tok := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
	s, err := tok.SignedString([]byte(secret))
	if err != nil {
		return "", fmt.Errorf("jwtauth: sign: %w", err)
	}
	return s, nil
}

func (m *Manager) verify(token, secret string, want TokenType) (*Claims, error) {
	claims := &Claims{}
	parsed, err := jwt.ParseWithClaims(token, claims, func(t *jwt.Token) (interface{}, error) {
		if _, ok := t.Method.(*jwt.SigningMethodHMAC); !ok {
			return nil, fmt.Errorf("jwtauth: unexpected signing method %q", t.Header["alg"])
		}
		return []byte(secret), nil
	}, jwt.WithValidMethods([]string{"HS256"}), jwt.WithIssuer(m.cfg.Issuer))
	if err != nil || !parsed.Valid {
		return nil, ErrInvalidToken
	}
	if claims.Type != want {
		return nil, ErrWrongTokenType
	}
	return claims, nil
}
