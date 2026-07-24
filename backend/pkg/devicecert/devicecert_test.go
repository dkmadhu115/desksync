package devicecert

import (
	"crypto/ed25519"
	"testing"
	"time"
)

func newTestIssuer(t *testing.T, now func() time.Time) (*Issuer, ed25519.PublicKey) {
	t.Helper()
	pub, priv, err := GenerateCA()
	if err != nil {
		t.Fatalf("GenerateCA: %v", err)
	}
	iss, err := NewIssuer(IssuerConfig{PrivateKey: priv, TTL: time.Hour, Now: now})
	if err != nil {
		t.Fatalf("NewIssuer: %v", err)
	}
	return iss, pub
}

func subject() Subject {
	return Subject{
		DeviceID:  "dev-1",
		UserID:    "user-1",
		Kind:      "desktop",
		PublicKey: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
	}
}

func TestIssueAndVerify(t *testing.T) {
	fixed := time.Unix(1_700_000_000, 0).UTC()
	iss, pub := newTestIssuer(t, func() time.Time { return fixed })
	v, err := NewVerifier(pub)
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}

	cert, err := iss.Issue(subject())
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	if cert.Fingerprint != Fingerprint(cert.Token) {
		t.Fatal("fingerprint mismatch")
	}

	claims, err := v.Verify(cert.Token, fixed)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	if claims.DeviceID != "dev-1" || claims.UserID != "user-1" || claims.Kind != "desktop" {
		t.Fatalf("unexpected claims: %+v", claims)
	}
	if claims.Serial == "" {
		t.Fatal("serial should not be empty")
	}
}

func TestVerifyRejectsTamperedToken(t *testing.T) {
	fixed := time.Unix(1_700_000_000, 0).UTC()
	iss, pub := newTestIssuer(t, func() time.Time { return fixed })
	v, _ := NewVerifier(pub)

	cert, _ := iss.Issue(subject())
	// Flip a character in the payload segment.
	tampered := []byte(cert.Token)
	tampered[0] ^= 0x01
	if _, err := v.Verify(string(tampered), fixed); err == nil {
		t.Fatal("expected verification to fail on tampered token")
	}
}

func TestVerifyRejectsWrongCA(t *testing.T) {
	fixed := time.Unix(1_700_000_000, 0).UTC()
	iss, _ := newTestIssuer(t, func() time.Time { return fixed })
	otherPub, _, _ := GenerateCA()
	v, _ := NewVerifier(otherPub)

	cert, _ := iss.Issue(subject())
	if _, err := v.Verify(cert.Token, fixed); err != ErrBadSignature {
		t.Fatalf("expected ErrBadSignature, got %v", err)
	}
}

func TestVerifyExpiredAndNotYetValid(t *testing.T) {
	issuedAt := time.Unix(1_700_000_000, 0).UTC()
	iss, pub := newTestIssuer(t, func() time.Time { return issuedAt })
	v, _ := NewVerifier(pub)
	cert, _ := iss.Issue(subject())

	// After the TTL window.
	if _, err := v.Verify(cert.Token, issuedAt.Add(2*time.Hour)); err != ErrExpired {
		t.Fatalf("expected ErrExpired, got %v", err)
	}
	// Before the (backdated) not-before.
	if _, err := v.Verify(cert.Token, issuedAt.Add(-time.Hour)); err != ErrNotYetValid {
		t.Fatalf("expected ErrNotYetValid, got %v", err)
	}
}

func TestVerifyRejectsMalformed(t *testing.T) {
	pub, _, _ := GenerateCA()
	v, _ := NewVerifier(pub)
	for _, bad := range []string{"", "nodot", "!!!.$$$"} {
		if _, err := v.Verify(bad, time.Now()); err == nil {
			t.Fatalf("expected error for %q", bad)
		}
	}
}

func TestFingerprintStableAndDistinct(t *testing.T) {
	fixed := time.Unix(1_700_000_000, 0).UTC()
	iss, _ := newTestIssuer(t, func() time.Time { return fixed })
	c1, _ := iss.Issue(subject())
	c2, _ := iss.Issue(subject())
	if c1.Fingerprint == c2.Fingerprint {
		t.Fatal("distinct serials must yield distinct fingerprints")
	}
	if !EqualFingerprint(c1.Fingerprint, Fingerprint(c1.Token)) {
		t.Fatal("EqualFingerprint should match")
	}
}

func TestIssueRequiresFields(t *testing.T) {
	fixed := time.Unix(1_700_000_000, 0).UTC()
	iss, _ := newTestIssuer(t, func() time.Time { return fixed })
	if _, err := iss.Issue(Subject{UserID: "u", PublicKey: "k"}); err == nil {
		t.Fatal("expected error for missing device id")
	}
}
