// Package store provides the Redis-backed ephemeral pairing challenge store.
// Pending challenges live here (not PostgreSQL) because they are short-lived,
// single-use, and never become a persistent row until confirmed.
package store

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/desksync/backend/services/pairing/internal/domain"
	"github.com/redis/go-redis/v9"
)

// RedisStore implements domain.ChallengeStore on Redis.
type RedisStore struct {
	client *redis.Client
}

// New builds a RedisStore.
func New(client *redis.Client) *RedisStore { return &RedisStore{client: client} }

func challengeKey(pairingID string) string { return "pairing:challenge:" + pairingID }
func attemptsKey(pairingID string) string  { return "pairing:attempts:" + pairingID }

// Save stores a challenge with the given TTL and clears any prior attempt count.
func (s *RedisStore) Save(ctx context.Context, ch domain.Challenge, ttl time.Duration) error {
	payload, err := json.Marshal(ch)
	if err != nil {
		return fmt.Errorf("marshal challenge: %w", err)
	}
	pipe := s.client.TxPipeline()
	pipe.Set(ctx, challengeKey(ch.PairingID), payload, ttl)
	pipe.Del(ctx, attemptsKey(ch.PairingID))
	if _, err := pipe.Exec(ctx); err != nil {
		return fmt.Errorf("save challenge: %w", err)
	}
	return nil
}

// Get returns a pending challenge or ErrChallengeNotFound.
func (s *RedisStore) Get(ctx context.Context, pairingID string) (domain.Challenge, error) {
	raw, err := s.client.Get(ctx, challengeKey(pairingID)).Bytes()
	if errors.Is(err, redis.Nil) {
		return domain.Challenge{}, domain.ErrChallengeNotFound
	}
	if err != nil {
		return domain.Challenge{}, fmt.Errorf("get challenge: %w", err)
	}
	var ch domain.Challenge
	if err := json.Unmarshal(raw, &ch); err != nil {
		return domain.Challenge{}, fmt.Errorf("unmarshal challenge: %w", err)
	}
	return ch, nil
}

// RecordFailedAttempt increments and returns the failed-attempt counter,
// keeping it aligned with the challenge's remaining TTL.
func (s *RedisStore) RecordFailedAttempt(ctx context.Context, pairingID string) (int, error) {
	n, err := s.client.Incr(ctx, attemptsKey(pairingID)).Result()
	if err != nil {
		return 0, fmt.Errorf("incr attempts: %w", err)
	}
	// Mirror the challenge TTL so the counter expires with the challenge.
	if ttl, err := s.client.TTL(ctx, challengeKey(pairingID)).Result(); err == nil && ttl > 0 {
		_ = s.client.Expire(ctx, attemptsKey(pairingID), ttl).Err()
	}
	return int(n), nil
}

// Consume deletes a challenge and its attempt counter (one-time use).
func (s *RedisStore) Consume(ctx context.Context, pairingID string) error {
	if err := s.client.Del(ctx, challengeKey(pairingID), attemptsKey(pairingID)).Err(); err != nil {
		return fmt.Errorf("consume challenge: %w", err)
	}
	return nil
}
