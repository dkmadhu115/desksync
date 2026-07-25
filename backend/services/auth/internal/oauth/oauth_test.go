package oauth

import (
	"net/url"
	"strings"
	"testing"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/services/auth/internal/domain"
)

func fullConfig() config.OAuthConfig {
	return config.OAuthConfig{
		Google: config.OAuthProviderConfig{
			ClientID: "google-client", ClientSecret: "google-secret",
			RedirectURL: "https://app.example.com/cb/google",
		},
		GitHub: config.OAuthProviderConfig{
			ClientID: "gh-client", ClientSecret: "gh-secret",
			RedirectURL: "https://app.example.com/cb/github",
		},
	}
}

func TestRegistryOnlyRegistersConfiguredProviders(t *testing.T) {
	// Only Google configured.
	reg := NewRegistry(config.OAuthConfig{
		Google: config.OAuthProviderConfig{ClientID: "id", ClientSecret: "secret", RedirectURL: "https://x/cb"},
	})
	if _, ok := reg.Get(domain.ProviderGoogle); !ok {
		t.Fatal("google should be registered")
	}
	if _, ok := reg.Get(domain.ProviderGitHub); ok {
		t.Fatal("github should NOT be registered when unconfigured")
	}
}

func TestEmptyRegistryHasNoProviders(t *testing.T) {
	reg := NewRegistry(config.OAuthConfig{})
	if _, ok := reg.Get(domain.ProviderGoogle); ok {
		t.Fatal("no providers should be registered for empty config")
	}
	if _, ok := reg.Get(domain.ProviderGitHub); ok {
		t.Fatal("no providers should be registered for empty config")
	}
}

func TestGoogleAuthCodeURLCarriesPKCEAndState(t *testing.T) {
	reg := NewRegistry(fullConfig())
	prov, ok := reg.Get(domain.ProviderGoogle)
	if !ok {
		t.Fatal("google not registered")
	}
	raw := prov.AuthCodeURL("state-123", "verifier-abcdefghijklmnopqrstuvwxyz")
	u, err := url.Parse(raw)
	if err != nil {
		t.Fatalf("parse url: %v", err)
	}
	q := u.Query()
	if q.Get("state") != "state-123" {
		t.Fatalf("state = %q, want state-123", q.Get("state"))
	}
	if q.Get("code_challenge_method") != "S256" {
		t.Fatalf("code_challenge_method = %q, want S256", q.Get("code_challenge_method"))
	}
	if q.Get("code_challenge") == "" {
		t.Fatal("code_challenge must be present (PKCE)")
	}
	if q.Get("client_id") != "google-client" {
		t.Fatalf("client_id = %q", q.Get("client_id"))
	}
	if q.Get("access_type") != "offline" {
		t.Fatalf("access_type = %q, want offline", q.Get("access_type"))
	}
	if !strings.Contains(q.Get("scope"), "email") {
		t.Fatalf("scope = %q, want it to include email", q.Get("scope"))
	}
	if q.Get("redirect_uri") != "https://app.example.com/cb/google" {
		t.Fatalf("redirect_uri = %q", q.Get("redirect_uri"))
	}
}

func TestGitHubAuthCodeURLCarriesPKCE(t *testing.T) {
	reg := NewRegistry(fullConfig())
	prov, ok := reg.Get(domain.ProviderGitHub)
	if !ok {
		t.Fatal("github not registered")
	}
	raw := prov.AuthCodeURL("st", "verifier-abcdefghijklmnopqrstuvwxyz")
	u, _ := url.Parse(raw)
	q := u.Query()
	if q.Get("code_challenge_method") != "S256" || q.Get("code_challenge") == "" {
		t.Fatalf("github url missing PKCE: %s", raw)
	}
	if q.Get("client_id") != "gh-client" {
		t.Fatalf("client_id = %q", q.Get("client_id"))
	}
}

func TestProviderNames(t *testing.T) {
	reg := NewRegistry(fullConfig())
	g, _ := reg.Get(domain.ProviderGoogle)
	if g.Name() != domain.ProviderGoogle {
		t.Fatalf("google name = %q", g.Name())
	}
	gh, _ := reg.Get(domain.ProviderGitHub)
	if gh.Name() != domain.ProviderGitHub {
		t.Fatalf("github name = %q", gh.Name())
	}
}

// The PKCE challenge must differ for different verifiers (sanity that the
// verifier actually feeds the challenge).
func TestDifferentVerifiersProduceDifferentChallenges(t *testing.T) {
	reg := NewRegistry(fullConfig())
	prov, _ := reg.Get(domain.ProviderGoogle)
	c1 := challengeOf(t, prov.AuthCodeURL("s", "verifier-one-abcdefghijklmnopqrst"))
	c2 := challengeOf(t, prov.AuthCodeURL("s", "verifier-two-abcdefghijklmnopqrst"))
	if c1 == c2 {
		t.Fatal("different verifiers should yield different code_challenge values")
	}
}

func challengeOf(t *testing.T, raw string) string {
	t.Helper()
	u, err := url.Parse(raw)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	return u.Query().Get("code_challenge")
}
