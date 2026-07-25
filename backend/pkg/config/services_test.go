package config

import (
	"testing"
	"time"
)

func TestOAuthProviderEnabled(t *testing.T) {
	if (OAuthProviderConfig{}).Enabled() {
		t.Fatal("empty provider should be disabled")
	}
	if (OAuthProviderConfig{ClientID: "id"}).Enabled() {
		t.Fatal("provider with only client id should be disabled")
	}
	if !(OAuthProviderConfig{ClientID: "id", ClientSecret: "secret"}).Enabled() {
		t.Fatal("provider with id+secret should be enabled")
	}
}

func TestLoadICESplitsAndTrimsCSV(t *testing.T) {
	t.Setenv("STUN_URLS", "stun:a:1, stun:b:2 ,")
	t.Setenv("TURN_URLS", " turn:c:3 ")
	t.Setenv("TURN_STATIC_AUTH_SECRET", "sekret")
	t.Setenv("TURN_CREDENTIAL_TTL", "30m")

	cfg := LoadICE()
	if len(cfg.STUNURLs) != 2 || cfg.STUNURLs[0] != "stun:a:1" || cfg.STUNURLs[1] != "stun:b:2" {
		t.Fatalf("STUNURLs = %v, want [stun:a:1 stun:b:2] (trimmed, empties dropped)", cfg.STUNURLs)
	}
	if len(cfg.TURNURLs) != 1 || cfg.TURNURLs[0] != "turn:c:3" {
		t.Fatalf("TURNURLs = %v, want [turn:c:3]", cfg.TURNURLs)
	}
	if cfg.TURNSecret != "sekret" {
		t.Fatalf("TURNSecret = %q", cfg.TURNSecret)
	}
	if cfg.CredentialTTL != 30*time.Minute {
		t.Fatalf("CredentialTTL = %v, want 30m", cfg.CredentialTTL)
	}
}

func TestLoadICEEmptyTurnYieldsNil(t *testing.T) {
	t.Setenv("STUN_URLS", "stun:only:1")
	t.Setenv("TURN_URLS", "")
	cfg := LoadICE()
	if cfg.TURNURLs != nil {
		t.Fatalf("TURNURLs = %v, want nil for empty env", cfg.TURNURLs)
	}
}

func TestLoadJWTDefaultsAndOverrides(t *testing.T) {
	// Defaults when unset (clear the ones the harness might set).
	t.Setenv("JWT_ACCESS_SECRET", "")
	t.Setenv("JWT_REFRESH_SECRET", "")
	t.Setenv("JWT_ACCESS_TTL", "")
	t.Setenv("JWT_ISSUER", "")
	def := LoadJWT()
	if def.AccessTTL != 15*time.Minute {
		t.Fatalf("default AccessTTL = %v, want 15m", def.AccessTTL)
	}
	if def.RefreshTTL != 720*time.Hour {
		t.Fatalf("default RefreshTTL = %v, want 720h", def.RefreshTTL)
	}
	if def.Issuer != "desksync" {
		t.Fatalf("default Issuer = %q, want desksync", def.Issuer)
	}

	t.Setenv("JWT_ACCESS_SECRET", "acc")
	t.Setenv("JWT_ACCESS_TTL", "5m")
	t.Setenv("JWT_ISSUER", "custom")
	over := LoadJWT()
	if over.AccessSecret != "acc" || over.AccessTTL != 5*time.Minute || over.Issuer != "custom" {
		t.Fatalf("override JWT = %+v", over)
	}
}

func TestLoadPostgresDefaults(t *testing.T) {
	t.Setenv("DATABASE_URL", "")
	t.Setenv("POSTGRES_HOST", "")
	t.Setenv("POSTGRES_PORT", "")
	t.Setenv("POSTGRES_DB", "")
	t.Setenv("POSTGRES_SSLMODE", "")
	cfg := LoadPostgres()
	if cfg.Host != "localhost" || cfg.Port != 5432 || cfg.Database != "desksync" || cfg.SSLMode != "disable" {
		t.Fatalf("postgres defaults = %+v", cfg)
	}
	if cfg.MaxConns != 10 || cfg.MinConns != 2 {
		t.Fatalf("pool defaults MaxConns=%d MinConns=%d, want 10/2", cfg.MaxConns, cfg.MinConns)
	}
}

func TestLoadRedisDefaultsAndOverrides(t *testing.T) {
	t.Setenv("REDIS_ADDR", "")
	t.Setenv("REDIS_DB", "")
	if def := LoadRedis(); def.Addr != "localhost:6379" || def.DB != 0 {
		t.Fatalf("redis defaults = %+v", def)
	}
	t.Setenv("REDIS_ADDR", "redis:6380")
	t.Setenv("REDIS_PASSWORD", "pw")
	t.Setenv("REDIS_DB", "3")
	over := LoadRedis()
	if over.Addr != "redis:6380" || over.Password != "pw" || over.DB != 3 {
		t.Fatalf("redis overrides = %+v", over)
	}
}

func TestLoadSignalingDefaults(t *testing.T) {
	t.Setenv("SIGNALING_TICKET_SECRET", "")
	t.Setenv("SIGNALING_TICKET_TTL", "")
	t.Setenv("SIGNALING_PUBLIC_URL", "")
	cfg := LoadSignaling()
	if cfg.TicketTTL != 2*time.Minute {
		t.Fatalf("default TicketTTL = %v, want 2m", cfg.TicketTTL)
	}
	if cfg.PublicURL == "" {
		t.Fatal("PublicURL should have a default")
	}
}

func TestLoadOAuthReadsProviders(t *testing.T) {
	t.Setenv("GOOGLE_OAUTH_CLIENT_ID", "gid")
	t.Setenv("GOOGLE_OAUTH_CLIENT_SECRET", "gsecret")
	t.Setenv("GITHUB_OAUTH_CLIENT_ID", "")
	t.Setenv("GITHUB_OAUTH_CLIENT_SECRET", "")
	cfg := LoadOAuth()
	if !cfg.Google.Enabled() {
		t.Fatal("google should be enabled")
	}
	if cfg.GitHub.Enabled() {
		t.Fatal("github should be disabled")
	}
}
