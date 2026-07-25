package config

import (
	"strings"
	"time"
)

// PostgresConfig holds PostgreSQL connection settings. Prefer DATABASE_URL when
// set; otherwise the discrete fields are assembled into a DSN.
type PostgresConfig struct {
	URL             string
	Host            string
	Port            int
	User            string
	Password        string
	Database        string
	SSLMode         string
	MaxConns        int32
	MinConns        int32
	MaxConnLifetime time.Duration
}

// LoadPostgres reads PostgreSQL configuration from the environment.
func LoadPostgres() PostgresConfig {
	return PostgresConfig{
		URL:             GetString("DATABASE_URL", ""),
		Host:            GetString("POSTGRES_HOST", "localhost"),
		Port:            GetInt("POSTGRES_PORT", 5432),
		User:            GetString("POSTGRES_USER", "desksync"),
		Password:        GetString("POSTGRES_PASSWORD", ""),
		Database:        GetString("POSTGRES_DB", "desksync"),
		SSLMode:         GetString("POSTGRES_SSLMODE", "disable"),
		MaxConns:        int32(GetInt("POSTGRES_MAX_CONNS", 10)),
		MinConns:        int32(GetInt("POSTGRES_MIN_CONNS", 2)),
		MaxConnLifetime: GetDuration("POSTGRES_MAX_CONN_LIFETIME", time.Hour),
	}
}

// RedisConfig holds Redis connection settings.
type RedisConfig struct {
	Addr     string
	Password string
	DB       int
}

// LoadRedis reads Redis configuration from the environment.
func LoadRedis() RedisConfig {
	return RedisConfig{
		Addr:     GetString("REDIS_ADDR", "localhost:6379"),
		Password: GetString("REDIS_PASSWORD", ""),
		DB:       GetInt("REDIS_DB", 0),
	}
}

// JWTConfig holds token signing configuration.
type JWTConfig struct {
	AccessSecret  string
	RefreshSecret string
	AccessTTL     time.Duration
	RefreshTTL    time.Duration
	Issuer        string
}

// LoadJWT reads JWT configuration from the environment.
func LoadJWT() JWTConfig {
	return JWTConfig{
		AccessSecret:  GetString("JWT_ACCESS_SECRET", ""),
		RefreshSecret: GetString("JWT_REFRESH_SECRET", ""),
		AccessTTL:     GetDuration("JWT_ACCESS_TTL", 15*time.Minute),
		RefreshTTL:    GetDuration("JWT_REFRESH_TTL", 720*time.Hour),
		Issuer:        GetString("JWT_ISSUER", "desksync"),
	}
}

// SignalingConfig holds the shared secret and TTL for signaling tickets. The
// session service issues tickets and the signaling service verifies them, so
// both read the same SIGNALING_TICKET_SECRET.
type SignalingConfig struct {
	TicketSecret string
	TicketTTL    time.Duration
	// PublicURL is the externally reachable base WebSocket URL of the signaling
	// service, returned to clients in the session response.
	PublicURL string
}

// LoadSignaling reads signaling configuration from the environment.
func LoadSignaling() SignalingConfig {
	return SignalingConfig{
		TicketSecret: GetString("SIGNALING_TICKET_SECRET", ""),
		TicketTTL:    GetDuration("SIGNALING_TICKET_TTL", 2*time.Minute),
		PublicURL:    GetString("SIGNALING_PUBLIC_URL", "ws://localhost:8085/api/v1/signaling/ws"),
	}
}

// ICEConfig holds STUN/TURN server configuration used to build the ICE server
// list returned to clients. TURN credentials are time-limited and derived per
// session using the TURN REST API (coturn "use-auth-secret") scheme, so no
// static TURN password is ever handed to clients.
type ICEConfig struct {
	STUNURLs   []string
	TURNURLs   []string
	TURNSecret string
	// CredentialTTL bounds the lifetime of a generated TURN credential.
	CredentialTTL time.Duration
}

// LoadICE reads ICE configuration from the environment. STUN_URLS and TURN_URLS
// are comma-separated.
func LoadICE() ICEConfig {
	return ICEConfig{
		STUNURLs:      splitCSV(GetString("STUN_URLS", "stun:stun.l.google.com:19302")),
		TURNURLs:      splitCSV(GetString("TURN_URLS", "")),
		TURNSecret:    GetString("TURN_STATIC_AUTH_SECRET", ""),
		CredentialTTL: GetDuration("TURN_CREDENTIAL_TTL", time.Hour),
	}
}

func splitCSV(s string) []string {
	if s == "" {
		return nil
	}
	parts := strings.Split(s, ",")
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if t := strings.TrimSpace(p); t != "" {
			out = append(out, t)
		}
	}
	return out
}

// GatewayConfig holds the internal upstream base URLs the API gateway
// reverse-proxies REST traffic to. Defaults match the docker-compose service
// names so the gateway works out of the box in the local/VPS stack.
type GatewayConfig struct {
	AuthURL      string
	DeviceURL    string
	SessionURL   string
	PairingURL   string
	SignalingURL string
}

// LoadGateway reads the gateway upstream configuration from the environment.
func LoadGateway() GatewayConfig {
	return GatewayConfig{
		AuthURL:      GetString("AUTH_UPSTREAM_URL", "http://auth:8081"),
		DeviceURL:    GetString("DEVICE_UPSTREAM_URL", "http://device:8082"),
		SessionURL:   GetString("SESSION_UPSTREAM_URL", "http://session:8083"),
		PairingURL:   GetString("PAIRING_UPSTREAM_URL", "http://pairing:8084"),
		SignalingURL: GetString("SIGNALING_UPSTREAM_URL", "http://signaling:8085"),
	}
}

// OAuthProviderConfig holds a single OAuth provider's credentials.
type OAuthProviderConfig struct {
	ClientID     string
	ClientSecret string
	RedirectURL  string
}

// Enabled reports whether the provider has credentials configured.
func (o OAuthProviderConfig) Enabled() bool {
	return o.ClientID != "" && o.ClientSecret != ""
}

// OAuthConfig aggregates the supported providers.
type OAuthConfig struct {
	Google OAuthProviderConfig
	GitHub OAuthProviderConfig
}

// LoadOAuth reads OAuth configuration from the environment.
func LoadOAuth() OAuthConfig {
	return OAuthConfig{
		Google: OAuthProviderConfig{
			ClientID:     GetString("GOOGLE_OAUTH_CLIENT_ID", ""),
			ClientSecret: GetString("GOOGLE_OAUTH_CLIENT_SECRET", ""),
			RedirectURL:  GetString("GOOGLE_OAUTH_REDIRECT_URL", ""),
		},
		GitHub: OAuthProviderConfig{
			ClientID:     GetString("GITHUB_OAUTH_CLIENT_ID", ""),
			ClientSecret: GetString("GITHUB_OAUTH_CLIENT_SECRET", ""),
			RedirectURL:  GetString("GITHUB_OAUTH_REDIRECT_URL", ""),
		},
	}
}
