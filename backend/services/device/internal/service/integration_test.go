package service_test

import (
	"context"
	"encoding/base64"
	"os"
	"path/filepath"
	"sort"
	"testing"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/postgres"
	"github.com/desksync/backend/services/device/internal/domain"
	"github.com/desksync/backend/services/device/internal/repository"
	devicesvc "github.com/desksync/backend/services/device/internal/service"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Integration tests run only when DESKSYNC_INTEGRATION=1 and require a real
// PostgreSQL (DATABASE_URL). They validate the pgx repository + service against
// the real schema.
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

func seedUser(t *testing.T, pool *pgxpool.Pool, email string) (userID string) {
	t.Helper()
	if err := pool.QueryRow(context.Background(),
		`INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id`,
		email, "x",
	).Scan(&userID); err != nil {
		t.Fatalf("seed user: %v", err)
	}
	return userID
}

func newIntegration(t *testing.T) (*devicesvc.Service, *pgxpool.Pool) {
	t.Helper()
	ctx := context.Background()
	pool, err := postgres.Connect(ctx, config.LoadPostgres())
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	_, _ = pool.Exec(ctx, `DROP SCHEMA public CASCADE; CREATE SCHEMA public;`)
	applyMigrations(t, pool)
	svc := devicesvc.New(devicesvc.Config{Repo: repository.New(pool)})
	return svc, pool
}

func key(seed byte) string {
	b := make([]byte, 32)
	for i := range b {
		b[i] = seed
	}
	return base64.StdEncoding.EncodeToString(b)
}

func reg(kind domain.Kind, platform domain.Platform, name, publicKey string) domain.Registration {
	return domain.Registration{Kind: kind, Platform: platform, Name: name, PublicKey: publicKey}
}

func TestIntegrationDeviceLifecycle(t *testing.T) {
	requireIntegration(t)
	svc, pool := newIntegration(t)
	defer pool.Close()

	ctx := context.Background()
	userID := seedUser(t, pool, "dev@example.com")

	d, err := svc.Register(ctx, userID, reg(domain.KindMobile, domain.PlatformIOS, "Phone", key(1)))
	if err != nil {
		t.Fatalf("Register: %v", err)
	}
	if d.Status != domain.StatusOffline {
		t.Fatalf("new device status = %q, want offline", d.Status)
	}

	// Re-registering the same public key for the same user is idempotent and
	// updates the name in place (same id).
	d2, err := svc.Register(ctx, userID, reg(domain.KindMobile, domain.PlatformIOS, "Phone Renamed", key(1)))
	if err != nil {
		t.Fatalf("re-Register: %v", err)
	}
	if d2.ID != d.ID || d2.Name != "Phone Renamed" {
		t.Fatalf("expected in-place update, got %+v", d2)
	}

	// Heartbeat flips presence online.
	hb, err := svc.Heartbeat(ctx, userID, d.ID, domain.StatusOnline)
	if err != nil {
		t.Fatalf("Heartbeat: %v", err)
	}
	if hb.Status != domain.StatusOnline || hb.LastSeenAt == nil {
		t.Fatalf("heartbeat did not update presence: %+v", hb)
	}

	list, err := svc.List(ctx, userID)
	if err != nil || len(list) != 1 {
		t.Fatalf("List: err=%v n=%d", err, len(list))
	}

	if err := svc.Revoke(ctx, userID, d.ID); err != nil {
		t.Fatalf("Revoke: %v", err)
	}
	if _, err := svc.Get(ctx, userID, d.ID); err == nil {
		t.Fatal("expected revoked device to be absent")
	}
	list, _ = svc.List(ctx, userID)
	if len(list) != 0 {
		t.Fatalf("expected empty list after revoke, got %d", len(list))
	}
}

func TestIntegrationPublicKeyConflictAcrossUsers(t *testing.T) {
	requireIntegration(t)
	svc, pool := newIntegration(t)
	defer pool.Close()

	ctx := context.Background()
	userA := seedUser(t, pool, "a@example.com")
	userB := seedUser(t, pool, "b@example.com")

	if _, err := svc.Register(ctx, userA, reg(domain.KindDesktop, domain.PlatformMacOS, "A laptop", key(9))); err != nil {
		t.Fatalf("register A: %v", err)
	}
	// User B cannot claim user A's public key.
	if _, err := svc.Register(ctx, userB, reg(domain.KindDesktop, domain.PlatformMacOS, "B laptop", key(9))); err == nil {
		t.Fatal("expected conflict registering another user's public key")
	}
}

func TestIntegrationRevokeCascadesPairings(t *testing.T) {
	requireIntegration(t)
	svc, pool := newIntegration(t)
	defer pool.Close()

	ctx := context.Background()
	userID := seedUser(t, pool, "cascade@example.com")

	mobile, err := svc.Register(ctx, userID, reg(domain.KindMobile, domain.PlatformAndroid, "Phone", key(2)))
	if err != nil {
		t.Fatalf("register mobile: %v", err)
	}
	desktop, err := svc.Register(ctx, userID, reg(domain.KindDesktop, domain.PlatformLinux, "Box", key(3)))
	if err != nil {
		t.Fatalf("register desktop: %v", err)
	}

	var pairingID string
	if err := pool.QueryRow(ctx,
		`INSERT INTO pairings (user_id, mobile_device_id, desktop_device_id, status)
		 VALUES ($1, $2, $3, 'active') RETURNING id`, userID, mobile.ID, desktop.ID,
	).Scan(&pairingID); err != nil {
		t.Fatalf("seed pairing: %v", err)
	}

	if err := svc.Revoke(ctx, userID, desktop.ID); err != nil {
		t.Fatalf("Revoke: %v", err)
	}

	var status string
	if err := pool.QueryRow(ctx, `SELECT status FROM pairings WHERE id = $1`, pairingID).Scan(&status); err != nil {
		t.Fatalf("read pairing: %v", err)
	}
	if status != "revoked" {
		t.Fatalf("pairing status = %q, want revoked", status)
	}
}
