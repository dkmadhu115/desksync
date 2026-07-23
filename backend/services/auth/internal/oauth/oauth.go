// Package oauth implements Google and GitHub authorization-code login with
// PKCE. Each provider exposes a URL to redirect the user to and an exchange
// that returns a normalized UserInfo. Network calls to the providers are made
// with the token-authenticated HTTP client from golang.org/x/oauth2.
package oauth

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/services/auth/internal/domain"
	"golang.org/x/oauth2"
	"golang.org/x/oauth2/github"
	"golang.org/x/oauth2/google"
)

// UserInfo is the normalized identity returned by a provider.
type UserInfo struct {
	ProviderUserID string
	Email          string
	Name           string
}

// Provider is a single OAuth identity provider.
type Provider interface {
	// Name is the domain provider identifier.
	Name() domain.Provider
	// AuthCodeURL builds the consent URL for the given state and PKCE verifier.
	AuthCodeURL(state, verifier string) string
	// Exchange swaps an authorization code (with PKCE verifier) for UserInfo.
	Exchange(ctx context.Context, code, verifier string) (UserInfo, error)
}

// Registry holds the configured providers.
type Registry struct {
	providers map[domain.Provider]Provider
}

// NewRegistry builds a Registry from OAuth config; only providers with
// credentials are registered.
func NewRegistry(cfg config.OAuthConfig) *Registry {
	r := &Registry{providers: map[domain.Provider]Provider{}}
	if cfg.Google.Enabled() {
		r.providers[domain.ProviderGoogle] = &googleProvider{cfg: oauthConfig(cfg.Google, google.Endpoint, []string{"openid", "email", "profile"})}
	}
	if cfg.GitHub.Enabled() {
		r.providers[domain.ProviderGitHub] = &githubProvider{cfg: oauthConfig(cfg.GitHub, github.Endpoint, []string{"read:user", "user:email"})}
	}
	return r
}

// Get returns the provider and whether it is configured.
func (r *Registry) Get(p domain.Provider) (Provider, bool) {
	prov, ok := r.providers[p]
	return prov, ok
}

func oauthConfig(c config.OAuthProviderConfig, ep oauth2.Endpoint, scopes []string) *oauth2.Config {
	return &oauth2.Config{
		ClientID:     c.ClientID,
		ClientSecret: c.ClientSecret,
		RedirectURL:  c.RedirectURL,
		Endpoint:     ep,
		Scopes:       scopes,
	}
}

// ---- Google ----

type googleProvider struct{ cfg *oauth2.Config }

func (g *googleProvider) Name() domain.Provider { return domain.ProviderGoogle }

func (g *googleProvider) AuthCodeURL(state, verifier string) string {
	return g.cfg.AuthCodeURL(state, oauth2.AccessTypeOffline, oauth2.S256ChallengeOption(verifier))
}

func (g *googleProvider) Exchange(ctx context.Context, code, verifier string) (UserInfo, error) {
	tok, err := g.cfg.Exchange(ctx, code, oauth2.VerifierOption(verifier))
	if err != nil {
		return UserInfo{}, fmt.Errorf("google: exchange: %w", err)
	}
	var body struct {
		Sub   string `json:"sub"`
		Email string `json:"email"`
		Name  string `json:"name"`
	}
	if err := getJSON(ctx, g.cfg, tok, "https://www.googleapis.com/oauth2/v3/userinfo", &body); err != nil {
		return UserInfo{}, err
	}
	return UserInfo{ProviderUserID: body.Sub, Email: body.Email, Name: body.Name}, nil
}

// ---- GitHub ----

type githubProvider struct{ cfg *oauth2.Config }

func (g *githubProvider) Name() domain.Provider { return domain.ProviderGitHub }

func (g *githubProvider) AuthCodeURL(state, verifier string) string {
	return g.cfg.AuthCodeURL(state, oauth2.S256ChallengeOption(verifier))
}

func (g *githubProvider) Exchange(ctx context.Context, code, verifier string) (UserInfo, error) {
	tok, err := g.cfg.Exchange(ctx, code, oauth2.VerifierOption(verifier))
	if err != nil {
		return UserInfo{}, fmt.Errorf("github: exchange: %w", err)
	}
	var user struct {
		ID    int64  `json:"id"`
		Login string `json:"login"`
		Name  string `json:"name"`
		Email string `json:"email"`
	}
	if err := getJSON(ctx, g.cfg, tok, "https://api.github.com/user", &user); err != nil {
		return UserInfo{}, err
	}
	email := user.Email
	if email == "" {
		email = g.primaryEmail(ctx, tok)
	}
	name := user.Name
	if name == "" {
		name = user.Login
	}
	return UserInfo{ProviderUserID: strconv.FormatInt(user.ID, 10), Email: email, Name: name}, nil
}

func (g *githubProvider) primaryEmail(ctx context.Context, tok *oauth2.Token) string {
	var emails []struct {
		Email    string `json:"email"`
		Primary  bool   `json:"primary"`
		Verified bool   `json:"verified"`
	}
	if err := getJSON(ctx, g.cfg, tok, "https://api.github.com/user/emails", &emails); err != nil {
		return ""
	}
	for _, e := range emails {
		if e.Primary && e.Verified {
			return e.Email
		}
	}
	return ""
}

// getJSON performs an authenticated GET and decodes the JSON body.
func getJSON(ctx context.Context, cfg *oauth2.Config, tok *oauth2.Token, url string, out interface{}) error {
	client := cfg.Client(ctx, tok)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("oauth: fetch %s: %w", url, err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		b, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return fmt.Errorf("oauth: %s returned %d: %s", url, resp.StatusCode, string(b))
	}
	return json.NewDecoder(resp.Body).Decode(out)
}
