// Package redisx provides a shared go-redis client configured from
// config.RedisConfig, with a connection health check. Redis backs presence,
// signaling pub/sub, rate limiting, and short-lived codes.
package redisx

import (
	"context"
	"fmt"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/redis/go-redis/v9"
)

// Connect creates and verifies a Redis client.
func Connect(ctx context.Context, c config.RedisConfig) (*redis.Client, error) {
	client := redis.NewClient(&redis.Options{
		Addr:         c.Addr,
		Password:     c.Password,
		DB:           c.DB,
		DialTimeout:  5 * time.Second,
		ReadTimeout:  3 * time.Second,
		WriteTimeout: 3 * time.Second,
	})

	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := client.Ping(pingCtx).Err(); err != nil {
		_ = client.Close()
		return nil, fmt.Errorf("redisx: ping: %w", err)
	}
	return client, nil
}

// HealthCheck returns a readiness function suitable for a /ready probe.
func HealthCheck(client *redis.Client) func(context.Context) error {
	return func(ctx context.Context) error {
		return client.Ping(ctx).Err()
	}
}
