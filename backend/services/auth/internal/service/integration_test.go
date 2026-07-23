package service_test

import (
	"context"
	"os"
	"path/filepath"
	"sort"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/crypto"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/pkg/postgres"
	"github.com/desksync/backend/pkg/redisx"
	"github.com/desksync/backend/services/auth/internal/repository"
	authsvc "github.com/desksync/backend/services/auth/internal/service"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Integration tests run only when DESKSYNC_INTEGRATION=1 and require a real
// PostgreSQL (DATABASE_URL) and Redis (REDIS_ADDR). They validate the full auth
// stack (pgx repositories + service) end-to-end.
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

func newIntegrationService(t *testing.T) (*authsvc.Service, *pgxpool.Pool) {
	t.Helper()
	ctx := context.Background()

	pool, err := postgres.Connect(ctx, config.LoadPostgres())
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	// Clean slate for a deterministic run.
	_, _ = pool.Exec(ctx, `DROP SCHEMA public CASCADE; CREATE SCHEMA public;`)
	applyMigrations(t, pool)

	// Redis must be reachable too (used by other flows); verify connectivity.
	rdb, err := redisx.Connect(ctx, config.LoadRedis())
	if err != nil {
		t.Fatalf("connect redis: %v", err)
	}
	t.Cleanup(func() { _ = rdb.Close() })

	jm, err := jwtauth.NewManager(config.JWTConfig{
		AccessSecret:  "0123456789abcdef0123456789abcdef",
		RefreshSecret: "abcdef0123456789abcdef0123456789",
		AccessTTL:     15 * time.Minute,
		RefreshTTL:    720 * time.Hour,
		Issuer:        "desksync-it",
	})
	if err != nil {
		t.Fatalf("jwt: %v", err)
	}
	argon := crypto.DefaultArgon2Params()
	argon.Memory = 8 * 1024
	argon.Iterations = 1

	svc := authsvc.New(authsvc.Config{
		Users:      repository.NewUserRepo(pool),
		Refresh:    repository.NewRefreshRepo(pool),
		JWT:        jm,
		Argon:      argon,
		RefreshTTL: 720 * time.Hour,
	})
	t.Cleanup(func() { pool.Close() })
	return svc, pool
}

func TestIntegrationRegisterLoginRefresh(t *testing.T) {
	requireIntegration(t)
	svc, _ := newIntegrationService(t)
	ctx := context.Background()

	reg, err := svc.Register(ctx, "it@example.com", "supersecretpw12", "IT", authsvc.Metadata{IPAddress: "127.0.0.1", UserAgent: "go-test"})
	if err != nil {
		t.Fatalf("Register: %v", err)
	}

	if _, err := svc.Login(ctx, "it@example.com", "supersecretpw12", authsvc.Metadata{}); err != nil {
		t.Fatalf("Login: %v", err)
	}

	rot, err := svc.Refresh(ctx, reg.RefreshToken, authsvc.Metadata{})
	if err != nil {
		t.Fatalf("Refresh: %v", err)
	}
	if rot.RefreshToken == reg.RefreshToken {
		t.Fatal("refresh token not rotated")
	}

	// Reuse of the old token must be rejected (theft detection).
	if _, err := svc.Refresh(ctx, reg.RefreshToken, authsvc.Metadata{}); err == nil {
		t.Fatal("expected reuse detection to reject the old token")
	}
}

func TestIntegrationDuplicateEmailConflict(t *testing.T) {
	requireIntegration(t)
	svc, _ := newIntegrationService(t)
	ctx := context.Background()

	if _, err := svc.Register(ctx, "dup@example.com", "supersecretpw12", "", authsvc.Metadata{}); err != nil {
		t.Fatalf("first register: %v", err)
	}
	if _, err := svc.Register(ctx, "dup@example.com", "supersecretpw12", "", authsvc.Metadata{}); err == nil {
		t.Fatal("expected conflict on duplicate email")
	}
}

func TestIntegrationOAuthUpsert(t *testing.T) {
	requireIntegration(t)
	svc, _ := newIntegrationService(t)
	ctx := context.Background()

	a, err := svc.UpsertOAuthUser(ctx, "github", "gh-1", "oauth@example.com", "OAuth User", authsvc.Metadata{})
	if err != nil {
		t.Fatalf("first upsert: %v", err)
	}
	b, err := svc.UpsertOAuthUser(ctx, "github", "gh-1", "oauth@example.com", "OAuth User", authsvc.Metadata{})
	if err != nil {
		t.Fatalf("second upsert: %v", err)
	}
	if a.User.ID != b.User.ID {
		t.Fatal("oauth upsert created a duplicate user")
	}
}
