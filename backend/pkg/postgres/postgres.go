// Package postgres provides a shared pgx connection pool configured from
// config.PostgresConfig, with sensible pool limits and a health check.
package postgres

import (
	"context"
	"fmt"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/jackc/pgx/v5/pgxpool"
)

// DSN builds a PostgreSQL connection string from configuration. If an explicit
// URL is provided it takes precedence.
func DSN(c config.PostgresConfig) string {
	if c.URL != "" {
		return c.URL
	}
	return fmt.Sprintf(
		"postgres://%s:%s@%s:%d/%s?sslmode=%s",
		c.User, c.Password, c.Host, c.Port, c.Database, c.SSLMode,
	)
}

// Connect opens and verifies a pgx connection pool. The context bounds the
// initial connection attempt.
func Connect(ctx context.Context, c config.PostgresConfig) (*pgxpool.Pool, error) {
	poolCfg, err := pgxpool.ParseConfig(DSN(c))
	if err != nil {
		return nil, fmt.Errorf("postgres: parse config: %w", err)
	}
	if c.MaxConns > 0 {
		poolCfg.MaxConns = c.MaxConns
	}
	if c.MinConns > 0 {
		poolCfg.MinConns = c.MinConns
	}
	if c.MaxConnLifetime > 0 {
		poolCfg.MaxConnLifetime = c.MaxConnLifetime
	}

	pool, err := pgxpool.NewWithConfig(ctx, poolCfg)
	if err != nil {
		return nil, fmt.Errorf("postgres: create pool: %w", err)
	}

	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := pool.Ping(pingCtx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("postgres: ping: %w", err)
	}
	return pool, nil
}

// HealthCheck returns a readiness function suitable for a /ready probe.
func HealthCheck(pool *pgxpool.Pool) func(context.Context) error {
	return func(ctx context.Context) error {
		return pool.Ping(ctx)
	}
}
