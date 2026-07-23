package signalticket

import (
	"testing"
	"time"
)

const testSecret = "super-secret-signaling-key-0123456789"

func TestIssueAndVerifyRoundTrip(t *testing.T) {
	iss, err := NewIssuer(testSecret, time.Minute)
	if err != nil {
		t.Fatalf("NewIssuer: %v", err)
	}
	ver, err := NewVerifier(testSecret)
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}

	tok, err := iss.Issue("sess-1", "user-1", RoleController)
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}

	got, err := ver.Verify(tok)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	if got.SessionID != "sess-1" || got.UserID != "user-1" || got.Role != RoleController {
		t.Fatalf("unexpected ticket: %+v", got)
	}
}

func TestVerifyRejectsTamperedPayload(t *testing.T) {
	iss, _ := NewIssuer(testSecret, time.Minute)
	ver, _ := NewVerifier(testSecret)

	tok, _ := iss.Issue("sess-1", "user-1", RoleAgent)
	// Flip a character in the payload segment.
	b := []byte(tok)
	// Find the first '.' then mutate the next byte.
	for i := 0; i < len(b); i++ {
		if b[i] == '.' {
			b[i+1] ^= 0x01
			break
		}
	}
	if _, err := ver.Verify(string(b)); err == nil {
		t.Fatal("expected verification to fail on tampered payload")
	}
}

func TestVerifyRejectsWrongSecret(t *testing.T) {
	iss, _ := NewIssuer(testSecret, time.Minute)
	ver, _ := NewVerifier("another-secret-key-0123456789abcd")

	tok, _ := iss.Issue("sess-1", "user-1", RoleController)
	if _, err := ver.Verify(tok); err != ErrSignature {
		t.Fatalf("expected ErrSignature, got %v", err)
	}
}

func TestVerifyRejectsExpired(t *testing.T) {
	iss, _ := NewIssuer(testSecret, time.Minute)
	// Force issuance in the past.
	iss.now = func() time.Time { return time.Now().Add(-2 * time.Minute) }
	tok, _ := iss.Issue("sess-1", "user-1", RoleController)

	ver, _ := NewVerifier(testSecret)
	if _, err := ver.Verify(tok); err != ErrExpired {
		t.Fatalf("expected ErrExpired, got %v", err)
	}
}

func TestVerifyRejectsMalformed(t *testing.T) {
	ver, _ := NewVerifier(testSecret)
	for _, bad := range []string{"", "v1.only-two", "v2.aaa.bbb", "not-a-ticket"} {
		if _, err := ver.Verify(bad); err == nil {
			t.Fatalf("expected malformed error for %q", bad)
		}
	}
}

func TestIssueRejectsInvalidRole(t *testing.T) {
	iss, _ := NewIssuer(testSecret, time.Minute)
	if _, err := iss.Issue("s", "u", Role("bogus")); err != ErrRole {
		t.Fatalf("expected ErrRole, got %v", err)
	}
}

func TestConstructorsValidateInputs(t *testing.T) {
	if _, err := NewIssuer("short", time.Minute); err == nil {
		t.Fatal("expected error for short secret")
	}
	if _, err := NewIssuer(testSecret, 0); err == nil {
		t.Fatal("expected error for non-positive ttl")
	}
	if _, err := NewVerifier("short"); err == nil {
		t.Fatal("expected error for short secret")
	}
}
