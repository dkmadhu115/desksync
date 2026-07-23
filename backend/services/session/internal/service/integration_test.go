package service_test

import (
	"context"
	"os"
	"path/filepath"
	"sort"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/postgres"
	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/session/internal/domain"
	"github.com/desksync/backend/services/session/internal/ice"
	"github.com/desksync/backend/services/session/internal/repository"
	sessionsvc "github.com/desksync/backend/services/session/internal/service"
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

// seedActivePairing inserts a user, two devices, and an active pairing,
// returning the user id and pairing id.
func seedActivePairing(t *testing.T, pool *pgxpool.Pool) (userID, pairingID string) {
	t.Helper()
	ctx := context.Background()

	if err := pool.QueryRow(ctx,
		`INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id`,
		"dev@example.com", "x",
	).Scan(&userID); err != nil {
		t.Fatalf("seed user: %v", err)
	}

	var mobileID, desktopID string
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

	if err := pool.QueryRow(ctx,
		`INSERT INTO pairings (user_id, mobile_device_id, desktop_device_id, status)
		 VALUES ($1, $2, $3, 'active') RETURNING id`, userID, mobileID, desktopID,
	).Scan(&pairingID); err != nil {
		t.Fatalf("seed pairing: %v", err)
	}
	return userID, pairingID
}

func newIntegrationService(t *testing.T) (*sessionsvc.Service, *pgxpool.Pool) {
	t.Helper()
	ctx := context.Background()

	pool, err := postgres.Connect(ctx, config.LoadPostgres())
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	_, _ = pool.Exec(ctx, `DROP SCHEMA public CASCADE; CREATE SCHEMA public;`)
	applyMigrations(t, pool)

	issuer, err := signalticket.NewIssuer("integration-signaling-secret-0123456789", time.Minute)
	if err != nil {
		t.Fatalf("issuer: %v", err)
	}
	svc := sessionsvc.New(sessionsvc.Config{
		Repo:         repository.New(pool),
		Tickets:      issuer,
		ICE:          ice.NewBuilder(config.ICEConfig{STUNURLs: []string{"stun:stun.example.com:3478"}}),
		SignalingURL: "ws://localhost:8085/api/v1/signaling/ws",
	})
	return svc, pool
}

func TestIntegrationSessionLifecycle(t *testing.T) {
	requireIntegration(t)
	svc, pool := newIntegrationService(t)
	defer pool.Close()

	userID, pairingID := seedActivePairing(t, pool)
	ctx := context.Background()

	created, err := svc.CreateSession(ctx, userID, pairingID)
	if err != nil {
		t.Fatalf("CreateSession: %v", err)
	}
	if created.Session.Status != domain.StatusConnecting {
		t.Fatalf("status = %q, want connecting", created.Session.Status)
	}
	if created.SignalingTicket == "" || len(created.ICEServers) == 0 {
		t.Fatal("expected ticket and ICE servers")
	}

	got, err := svc.GetSession(ctx, userID, created.Session.ID)
	if err != nil {
		t.Fatalf("GetSession: %v", err)
	}
	if got.ID != created.Session.ID {
		t.Fatal("GetSession returned a different session")
	}

	list, err := svc.ListSessions(ctx, userID)
	if err != nil || len(list) != 1 {
		t.Fatalf("ListSessions: err=%v n=%d", err, len(list))
	}

	ended, err := svc.EndSession(ctx, userID, created.Session.ID, "test")
	if err != nil {
		t.Fatalf("EndSession: %v", err)
	}
	if ended.Status != domain.StatusEnded {
		t.Fatalf("status = %q, want ended", ended.Status)
	}
	// End is idempotent.
	if _, err := svc.EndSession(ctx, userID, created.Session.ID, "test"); err != nil {
		t.Fatalf("EndSession (2nd) should be idempotent: %v", err)
	}
}

func TestIntegrationCreateSessionForeignPairingDenied(t *testing.T) {
	requireIntegration(t)
	svc, pool := newIntegrationService(t)
	defer pool.Close()

	_, pairingID := seedActivePairing(t, pool)
	// A different user must not be able to create a session for this pairing.
	if _, err := svc.CreateSession(context.Background(), "00000000-0000-0000-0000-000000000000", pairingID); err == nil {
		t.Fatal("expected error creating a session for a foreign pairing")
	}
}
