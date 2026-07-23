package repository

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/redis/go-redis/v9"
)

// RedisStateStore stores short-lived OAuth PKCE verifiers keyed by state in
// Redis. It implements transport.StateStore.
type RedisStateStore struct {
	client *redis.Client
	prefix string
}

// NewRedisStateStore builds a RedisStateStore.
func NewRedisStateStore(client *redis.Client) *RedisStateStore {
	return &RedisStateStore{client: client, prefix: "oauth:state:"}
}

// Save persists the verifier under the state key with a TTL.
func (s *RedisStateStore) Save(ctx context.Context, state, verifier string, ttl time.Duration) error {
	if err := s.client.Set(ctx, s.prefix+state, verifier, ttl).Err(); err != nil {
		return fmt.Errorf("state store save: %w", err)
	}
	return nil
}

// Consume atomically fetches and deletes the verifier for a state, ensuring a
// state can be used only once.
func (s *RedisStateStore) Consume(ctx context.Context, state string) (string, error) {
	verifier, err := s.client.GetDel(ctx, s.prefix+state).Result()
	if errors.Is(err, redis.Nil) {
		return "", errors.New("state not found")
	}
	if err != nil {
		return "", fmt.Errorf("state store consume: %w", err)
	}
	return verifier, nil
}
