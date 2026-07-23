// Package config provides typed, environment-driven configuration loading that
// is shared by every DeskSync backend service. It intentionally has zero
// third-party dependencies so it can be imported anywhere without bloating a
// service's dependency graph.
//
// Configuration precedence: explicit environment variables override the
// documented defaults. Services embed a Base config and extend it with their
// own fields.
package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

// Environment identifies the deployment environment.
type Environment string

const (
	EnvDevelopment Environment = "development"
	EnvStaging     Environment = "staging"
	EnvProduction  Environment = "production"
)

// Base holds configuration common to every service.
type Base struct {
	ServiceName string
	Environment Environment
	LogLevel    string
	LogFormat   string
	HTTPAddr    string
}

// LoadBase builds the shared configuration for a service. serviceName is the
// canonical name used in logs and metrics; httpAddrEnv is the environment
// variable that carries this service's listen address (e.g. "GATEWAY_HTTP_ADDR").
func LoadBase(serviceName, httpAddrEnv, defaultAddr string) Base {
	return Base{
		ServiceName: serviceName,
		Environment: Environment(GetString("ENVIRONMENT", string(EnvDevelopment))),
		LogLevel:    GetString("LOG_LEVEL", "info"),
		LogFormat:   GetString("LOG_FORMAT", "json"),
		HTTPAddr:    GetString(httpAddrEnv, defaultAddr),
	}
}

// IsProduction reports whether the service runs in a production environment.
func (b Base) IsProduction() bool { return b.Environment == EnvProduction }

// GetString returns the value of key or def when unset/empty.
func GetString(key, def string) string {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		return v
	}
	return def
}

// GetInt returns the integer value of key or def when unset or unparsable.
func GetInt(key string, def int) int {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		if n, err := strconv.Atoi(strings.TrimSpace(v)); err == nil {
			return n
		}
	}
	return def
}

// GetBool returns the boolean value of key or def when unset or unparsable.
func GetBool(key string, def bool) bool {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		if b, err := strconv.ParseBool(strings.TrimSpace(v)); err == nil {
			return b
		}
	}
	return def
}

// GetDuration returns the duration value of key or def when unset or unparsable.
func GetDuration(key string, def time.Duration) time.Duration {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		if d, err := time.ParseDuration(strings.TrimSpace(v)); err == nil {
			return d
		}
	}
	return def
}

// MustGet returns the value of key or an error when it is unset. Use for
// secrets that have no safe default (e.g. JWT signing keys in production).
func MustGet(key string) (string, error) {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		return v, nil
	}
	return "", fmt.Errorf("required environment variable %q is not set", key)
}
