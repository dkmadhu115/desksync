package crypto

import (
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"math/big"
)

// GenerateToken returns a URL-safe, cryptographically random token with the
// given number of random bytes (before encoding). Used for refresh tokens and
// signaling tickets.
func GenerateToken(nBytes int) (string, error) {
	if nBytes <= 0 {
		nBytes = 32
	}
	b := make([]byte, nBytes)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("crypto: generate token: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(b), nil
}

// HashToken returns the hex-encoded SHA-256 of a token. Refresh tokens and
// pairing codes are stored as this hash so a database leak does not expose
// usable secrets.
func HashToken(token string) string {
	sum := sha256.Sum256([]byte(token))
	return hex.EncodeToString(sum[:])
}

// EqualTokenHash compares a plaintext token against a stored hash in constant
// time.
func EqualTokenHash(token, storedHash string) bool {
	computed := HashToken(token)
	return subtle.ConstantTimeCompare([]byte(computed), []byte(storedHash)) == 1
}

// GenerateNumericCode returns a cryptographically random numeric code of the
// given length (e.g. an 8-digit pairing code). Uses rejection-free modulo over
// crypto/rand big integers to avoid modulo bias.
func GenerateNumericCode(digits int) (string, error) {
	if digits <= 0 {
		digits = 8
	}
	out := make([]byte, digits)
	for i := 0; i < digits; i++ {
		n, err := rand.Int(rand.Reader, big.NewInt(10))
		if err != nil {
			return "", fmt.Errorf("crypto: generate code: %w", err)
		}
		out[i] = byte('0' + n.Int64())
	}
	return string(out), nil
}
