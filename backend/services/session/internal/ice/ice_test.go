package ice

import (
	"crypto/hmac"
	"crypto/sha1"
	"encoding/base64"
	"fmt"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
)

func TestBuildStunOnly(t *testing.T) {
	b := NewBuilder(config.ICEConfig{STUNURLs: []string{"stun:stun.example.com:3478"}})
	servers := b.Build("sess-1")
	if len(servers) != 1 {
		t.Fatalf("expected 1 server, got %d", len(servers))
	}
	if servers[0].Username != "" || servers[0].Credential != "" {
		t.Fatal("STUN server must not carry credentials")
	}
}

func TestBuildIncludesTurnWithDerivedCredentials(t *testing.T) {
	cfg := config.ICEConfig{
		STUNURLs:      []string{"stun:stun.example.com:3478"},
		TURNURLs:      []string{"turn:turn.example.com:3478"},
		TURNSecret:    "turn-shared-secret",
		CredentialTTL: time.Hour,
	}
	b := NewBuilder(cfg)
	fixed := time.Unix(1_000_000, 0)
	b.now = func() time.Time { return fixed }

	servers := b.Build("sess-42")
	if len(servers) != 2 {
		t.Fatalf("expected STUN + TURN, got %d", len(servers))
	}
	turn := servers[1]

	wantUser := fmt.Sprintf("%d:%s", fixed.Add(time.Hour).Unix(), "sess-42")
	if turn.Username != wantUser {
		t.Fatalf("username = %q, want %q", turn.Username, wantUser)
	}

	mac := hmac.New(sha1.New, []byte(cfg.TURNSecret))
	mac.Write([]byte(wantUser))
	wantCred := base64.StdEncoding.EncodeToString(mac.Sum(nil))
	if turn.Credential != wantCred {
		t.Fatalf("credential mismatch")
	}
}

func TestBuildOmitsTurnWithoutSecret(t *testing.T) {
	b := NewBuilder(config.ICEConfig{
		TURNURLs: []string{"turn:turn.example.com:3478"},
		// no secret
	})
	if servers := b.Build("s"); len(servers) != 0 {
		t.Fatalf("expected no servers without STUN or a TURN secret, got %d", len(servers))
	}
}
