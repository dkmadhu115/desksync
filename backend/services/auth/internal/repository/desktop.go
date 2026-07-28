package repository

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/desksync/backend/services/auth/internal/domain"
	"github.com/redis/go-redis/v9"
)

// RedisDesktopStore stores the two short-lived artifacts of a desktop sign-in in
// Redis: the pending flow (keyed by OAuth state) and the one-time grant it
// produces (keyed by an opaque code). Both are consumed atomically with GETDEL so
// a code or state can never be redeemed twice.
//
// It implements transport.DesktopStore.
type RedisDesktopStore struct {
	client      *redis.Client
	flowPrefix  string
	grantPrefix string
}

// NewRedisDesktopStore builds a RedisDesktopStore.
func NewRedisDesktopStore(client *redis.Client) *RedisDesktopStore {
	return &RedisDesktopStore{
		client:      client,
		flowPrefix:  "oauth:desktop:flow:",
		grantPrefix: "oauth:desktop:grant:",
	}
}

// SaveFlow persists the desktop context for an in-progress sign-in.
func (s *RedisDesktopStore) SaveFlow(ctx context.Context, state string, flow domain.DesktopFlow, ttl time.Duration) error {
	return s.save(ctx, s.flowPrefix+state, flow, ttl)
}

// ConsumeFlow fetches and deletes the desktop context for a state. A missing key
// is not an error: it simply means this was an ordinary browser sign-in.
func (s *RedisDesktopStore) ConsumeFlow(ctx context.Context, state string) (domain.DesktopFlow, bool, error) {
	var flow domain.DesktopFlow
	raw, err := s.client.GetDel(ctx, s.flowPrefix+state).Result()
	if errors.Is(err, redis.Nil) {
		return flow, false, nil
	}
	if err != nil {
		return flow, false, fmt.Errorf("desktop store consume flow: %w", err)
	}
	if err := json.Unmarshal([]byte(raw), &flow); err != nil {
		return flow, false, fmt.Errorf("desktop store decode flow: %w", err)
	}
	return flow, true, nil
}

// SaveGrant persists a completed sign-in awaiting redemption.
func (s *RedisDesktopStore) SaveGrant(ctx context.Context, code string, grant domain.DesktopGrant, ttl time.Duration) error {
	return s.save(ctx, s.grantPrefix+code, grant, ttl)
}

// ConsumeGrant fetches and deletes a grant, making redemption single-use.
func (s *RedisDesktopStore) ConsumeGrant(ctx context.Context, code string) (domain.DesktopGrant, error) {
	var grant domain.DesktopGrant
	raw, err := s.client.GetDel(ctx, s.grantPrefix+code).Result()
	if errors.Is(err, redis.Nil) {
		return grant, errors.New("grant not found")
	}
	if err != nil {
		return grant, fmt.Errorf("desktop store consume grant: %w", err)
	}
	if err := json.Unmarshal([]byte(raw), &grant); err != nil {
		return grant, fmt.Errorf("desktop store decode grant: %w", err)
	}
	return grant, nil
}

func (s *RedisDesktopStore) save(ctx context.Context, key string, v any, ttl time.Duration) error {
	payload, err := json.Marshal(v)
	if err != nil {
		return fmt.Errorf("desktop store encode: %w", err)
	}
	if err := s.client.Set(ctx, key, payload, ttl).Err(); err != nil {
		return fmt.Errorf("desktop store save: %w", err)
	}
	return nil
}
