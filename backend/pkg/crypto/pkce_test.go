package crypto

import "testing"

func TestS256ChallengeMatchesRFC7636Example(t *testing.T) {
	// RFC 7636 Appendix B reference vector.
	const (
		verifier  = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
		challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
	)
	if got := S256Challenge(verifier); got != challenge {
		t.Fatalf("S256Challenge = %q, want %q", got, challenge)
	}
}

func TestS256ChallengeIsUnpaddedBase64URL(t *testing.T) {
	got := S256Challenge("any-verifier-value")
	// SHA-256 is 32 bytes → 43 unpadded base64url characters.
	if len(got) != 43 {
		t.Fatalf("challenge length = %d, want 43 (%q)", len(got), got)
	}
	for _, r := range got {
		switch {
		case r >= 'A' && r <= 'Z', r >= 'a' && r <= 'z', r >= '0' && r <= '9', r == '-', r == '_':
		default:
			t.Fatalf("challenge %q contains non-base64url character %q", got, r)
		}
	}
}

func TestEqualS256Challenge(t *testing.T) {
	const verifier = "a-high-entropy-code-verifier"
	challenge := S256Challenge(verifier)

	if !EqualS256Challenge(verifier, challenge) {
		t.Fatal("matching verifier should satisfy its challenge")
	}
	if EqualS256Challenge("a-different-verifier", challenge) {
		t.Fatal("a different verifier must not satisfy the challenge")
	}
	if EqualS256Challenge(verifier, "") {
		t.Fatal("an empty challenge must never match")
	}
}

func TestS256ChallengeIsDeterministicAndDistinct(t *testing.T) {
	a1 := S256Challenge("verifier-a")
	a2 := S256Challenge("verifier-a")
	b := S256Challenge("verifier-b")
	if a1 != a2 {
		t.Fatal("same verifier must produce the same challenge")
	}
	if a1 == b {
		t.Fatal("different verifiers must produce different challenges")
	}
}
