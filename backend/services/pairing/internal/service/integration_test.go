package service_test

import (
	"context"
	"os"
	"path/filepath"
	"sort"
	"testing"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/postgres"
	"github.com/desksync/backend/pkg/redisx"
	"github.com/desksync/backend/services/pairing/internal/domain"
	"github.com/desksync/backend/services/pairing/internal/repository"
	pairingsvc "github.com/desksync/backend/services/pairing/internal/service"
	"github.com/desksync/backend/services/pairing/internal/store"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Integration tests run only when DESKSYNC_INTEGRATION=1 and require a real
// PostgreSQL (DATABASE_URL) and Redis (REDIS_ADDR).
func requireIntegration(t *testing.T) {
	t.Helper()
	if os.Getenv("DESKSYNC_INTEGRATION") == "" {
		t.Skip("set DESKSYNC_INTEGRATION=1 to run integration tests")
	}
}

func applyMigrations(t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	ctx := context.Background()
	dir := "../../../../migrations"
	ups, err := filepath.Glob(filepath.Join(dir, "*.up.sql"))
	if err != nil || len(ups) == 0 {
		t.Fatalf("no migrations found in %s: %v", dir, err)
	}
	sort.Strings(ups)
	for _, f := range ups {
		sql, err := os.ReadFile(f)
		if err != nil {
			t.Fatalf("read %s: %v", f, err)
		}
		if _, err := pool.Exec(ctx, string(sql)); err != nil {
			t.Fatalf("apply %s: %v", filepath.Base(f), err)
		}
	}
}

// seedUserAndDevices inserts a user plus a mobile and desktop device, returning
// the ids.
func seedUserAndDevices(t *testing.T, pool *pgxpool.Pool) (userID, mobileID, desktopID string) {
	t.Helper()
	ctx := context.Background()
	if err := pool.QueryRow(ctx,
		`INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id`,
		"dev@example.com", "x",
	).Scan(&userID); err != nil {
		t.Fatalf("seed user: %v", err)
	}
	if err := pool.QueryRow(ctx,
		`INSERT INTO devices (user_id, kind, platform, name, public_key)
		 VALUES ($1, 'mobile', 'ios', 'Phone', 'pk-mobile') RETURNING id`, userID,
	).Scan(&mobileID); err != nil {
		t.Fatalf("seed mobile: %v", err)
	}
	if err := pool.QueryRow(ctx,
		`INSERT INTO devices (user_id, kind, platform, name, public_key)
		 VALUES ($1, 'desktop', 'macos', 'Laptop', 'pk-desktop') RETURNING id`, userID,
	).Scan(&desktopID); err != nil {
		t.Fatalf("seed desktop: %v", err)
	}
	return userID, mobileID, desktopID
}

func newIntegration(t *testing.T) (*pairingsvc.Service, *pgxpool.Pool) {
	t.Helper()
	ctx := context.Background()

	pool, err := postgres.Connect(ctx, config.LoadPostgres())
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	_, _ = pool.Exec(ctx, `DROP SCHEMA public CASCADE; CREATE SCHEMA public;`)
	applyMigrations(t, pool)

	rdb, err := redisx.Connect(ctx, config.LoadRedis())
	if err != nil {
		t.Fatalf("connect redis: %v", err)
	}
	if err := rdb.FlushDB(ctx).Err(); err != nil {
		t.Fatalf("flush redis: %v", err)
	}

	svc := pairingsvc.New(pairingsvc.Config{
		Repo:  repository.New(pool),
		Store: store.New(rdb),
	})
	return svc, pool
}

func TestIntegrationPairingLifecycle(t *testing.T) {
	requireIntegration(t)
	svc, pool := newIntegration(t)
	defer pool.Close()

	ctx := context.Background()
	userID, mobileID, desktopID := seedUserAndDevices(t, pool)

	ch, err := svc.Initiate(ctx, userID, desktopID)
	if err != nil {
		t.Fatalf("Initiate: %v", err)
	}

	pairing, err := svc.Confirm(ctx, userID, ch.PairingID, ch.ManualCode, mobileID)
	if err != nil {
		t.Fatalf("Confirm: %v", err)
	}
	if pairing.Status != domain.StatusActive || !pairing.Trusted {
		t.Fatalf("expected active+trusted pairing, got %+v", pairing)
	}

	// Confirming again with the (now consumed) challenge must fail.
	if _, err := svc.Confirm(ctx, userID, ch.PairingID, ch.ManualCode, mobileID); err == nil {
		t.Fatal("expected second confirm to fail (challenge consumed)")
	}

	list, err := svc.List(ctx, userID)
	if err != nil || len(list) != 1 {
		t.Fatalf("List: err=%v n=%d", err, len(list))
	}

	if err := svc.Revoke(ctx, userID, pairing.ID); err != nil {
		t.Fatalf("Revoke: %v", err)
	}
	list, _ = svc.List(ctx, userID)
	if len(list) != 0 {
		t.Fatalf("expected no active pairings after revoke, got %d", len(list))
	}
}

func TestIntegrationConfirmWrongCode(t *testing.T) {
	requireIntegration(t)
	svc, pool := newIntegration(t)
	defer pool.Close()

	ctx := context.Background()
	userID, mobileID, desktopID := seedUserAndDevices(t, pool)

	ch, err := svc.Initiate(ctx, userID, desktopID)
	if err != nil {
		t.Fatalf("Initiate: %v", err)
	}
	wrong := "00000000"
	if wrong == ch.ManualCode {
		wrong = "11111111"
	}
	if _, err := svc.Confirm(ctx, userID, ch.PairingID, wrong, mobileID); err == nil {
		t.Fatal("expected wrong code to be rejected")
	}
	// The correct code still works while attempts remain.
	if _, err := svc.Confirm(ctx, userID, ch.PairingID, ch.ManualCode, mobileID); err != nil {
		t.Fatalf("correct code should succeed after one wrong attempt: %v", err)
	}
}
