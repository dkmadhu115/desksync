package jwtauth

import (
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
)

func testManager(t *testing.T) *Manager {
	t.Helper()
	m, err := NewManager(config.JWTConfig{
		AccessSecret:  "0123456789abcdef0123456789abcdef",
		RefreshSecret: "abcdef0123456789abcdef0123456789",
		AccessTTL:     15 * time.Minute,
		RefreshTTL:    720 * time.Hour,
		Issuer:        "desksync-test",
	})
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	return m
}

func TestNewManagerRejectsShortSecrets(t *testing.T) {
	_, err := NewManager(config.JWTConfig{AccessSecret: "short", RefreshSecret: "short"})
	if err == nil {
		t.Fatal("expected error for short secrets")
	}
}

func TestIssueAndVerify(t *testing.T) {
	m := testManager(t)
	pair, err := m.Issue("user-1", "jti-1")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	if pair.AccessToken == "" || pair.RefreshToken == "" {
		t.Fatal("empty tokens")
	}
	if pair.ExpiresIn != 900 {
		t.Fatalf("ExpiresIn = %d, want 900", pair.ExpiresIn)
	}

	ac, err := m.VerifyAccess(pair.AccessToken)
	if err != nil {
		t.Fatalf("VerifyAccess: %v", err)
	}
	if ac.UserID != "user-1" || ac.Type != AccessToken {
		t.Fatalf("access claims = %+v", ac)
	}

	rc, err := m.VerifyRefresh(pair.RefreshToken)
	if err != nil {
		t.Fatalf("VerifyRefresh: %v", err)
	}
	if rc.ID != "jti-1" || rc.Type != RefreshToken {
		t.Fatalf("refresh claims = %+v", rc)
	}
}

func TestTokenTypeIsolation(t *testing.T) {
	m := testManager(t)
	pair, _ := m.Issue("user-1", "jti-1")

	// An access token must not verify as a refresh token and vice versa.
	if _, err := m.VerifyRefresh(pair.AccessToken); err == nil {
		t.Fatal("access token verified as refresh")
	}
	if _, err := m.VerifyAccess(pair.RefreshToken); err == nil {
		t.Fatal("refresh token verified as access")
	}
}

func TestExpiredTokenRejected(t *testing.T) {
	m := testManager(t)
	// Force issuance in the past so the access token is already expired.
	m.now = func() time.Time { return time.Now().Add(-time.Hour) }
	pair, _ := m.Issue("user-1", "jti-1")

	m.now = time.Now
	if _, err := m.VerifyAccess(pair.AccessToken); err == nil {
		t.Fatal("expired access token was accepted")
	}
}

func TestTamperedTokenRejected(t *testing.T) {
	m := testManager(t)
	pair, _ := m.Issue("user-1", "jti-1")
	tampered := pair.AccessToken[:len(pair.AccessToken)-2] + "xx"
	if _, err := m.VerifyAccess(tampered); err == nil {
		t.Fatal("tampered token was accepted")
	}
}
