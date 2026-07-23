package crypto

import (
	"regexp"
	"testing"
)

func TestHashAndVerifyPassword(t *testing.T) {
	p := DefaultArgon2Params()
	// Keep the test fast while still exercising the real KDF.
	p.Memory = 8 * 1024
	p.Iterations = 1

	hash, err := HashPassword("correct horse battery staple", p)
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	if !regexp.MustCompile(`^\$argon2id\$v=19\$`).MatchString(hash) {
		t.Fatalf("unexpected hash format: %s", hash)
	}

	ok, err := VerifyPassword("correct horse battery staple", hash)
	if err != nil || !ok {
		t.Fatalf("VerifyPassword valid = (%v, %v), want (true, nil)", ok, err)
	}

	ok, err = VerifyPassword("wrong password", hash)
	if err != nil {
		t.Fatalf("VerifyPassword err = %v", err)
	}
	if ok {
		t.Fatal("VerifyPassword accepted a wrong password")
	}
}

func TestHashPasswordRejectsEmpty(t *testing.T) {
	if _, err := HashPassword("", DefaultArgon2Params()); err == nil {
		t.Fatal("expected error for empty password")
	}
}

func TestVerifyPasswordRejectsMalformed(t *testing.T) {
	for _, bad := range []string{"", "not-a-hash", "$argon2id$v=19$bad"} {
		if _, err := VerifyPassword("x", bad); err == nil {
			t.Errorf("expected error for malformed hash %q", bad)
		}
	}
}

func TestTokenHashingRoundTrip(t *testing.T) {
	tok, err := GenerateToken(32)
	if err != nil {
		t.Fatalf("GenerateToken: %v", err)
	}
	if len(tok) < 40 {
		t.Fatalf("token too short: %q", tok)
	}
	h := HashToken(tok)
	if len(h) != 64 { // sha256 hex
		t.Fatalf("hash length = %d, want 64", len(h))
	}
	if !EqualTokenHash(tok, h) {
		t.Fatal("EqualTokenHash returned false for matching token")
	}
	if EqualTokenHash("other", h) {
		t.Fatal("EqualTokenHash returned true for non-matching token")
	}
}

func TestGenerateNumericCode(t *testing.T) {
	code, err := GenerateNumericCode(8)
	if err != nil {
		t.Fatalf("GenerateNumericCode: %v", err)
	}
	if !regexp.MustCompile(`^[0-9]{8}$`).MatchString(code) {
		t.Fatalf("code = %q, want 8 digits", code)
	}
}

func TestGeneratedTokensAreUnique(t *testing.T) {
	seen := make(map[string]struct{}, 100)
	for i := 0; i < 100; i++ {
		tok, err := GenerateToken(16)
		if err != nil {
			t.Fatalf("GenerateToken: %v", err)
		}
		if _, dup := seen[tok]; dup {
			t.Fatal("duplicate token generated")
		}
		seen[tok] = struct{}{}
	}
}
