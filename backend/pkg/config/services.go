package config

import "time"

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
