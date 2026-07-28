// Command auth issues and validates credentials for DeskSync: email login,
// Google/GitHub OAuth, JWT access tokens, and refresh-token rotation.
//
// It wires configuration, PostgreSQL, Redis, the JWT manager, the domain
// repositories, the application service, and the HTTP transport, then serves
// via the shared service runtime with readiness checks and graceful shutdown.
package main

import (
	"context"
	"os"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/crypto"
	"github.com/desksync/backend/pkg/httpx"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/pkg/logger"
	"github.com/desksync/backend/pkg/postgres"
	"github.com/desksync/backend/pkg/redisx"
	"github.com/desksync/backend/pkg/service"
	"github.com/desksync/backend/services/auth/internal/oauth"
	"github.com/desksync/backend/services/auth/internal/repository"
	authsvc "github.com/desksync/backend/services/auth/internal/service"
	"github.com/desksync/backend/services/auth/internal/transport"
	"github.com/gofiber/fiber/v2"
)

var version = "0.2.0-phase2"

func main() {
	base := config.LoadBase("auth", "AUTH_HTTP_ADDR", ":8081")
	log := logger.New(logger.Options{ServiceName: base.ServiceName, Level: base.LogLevel, Format: base.LogFormat})

	// --- Infrastructure ---
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pool, err := postgres.Connect(ctx, config.LoadPostgres())
	if err != nil {
		log.Error("failed to connect to postgres", "error", err.Error())
		os.Exit(1)
	}
	defer pool.Close()

	rdb, err := redisx.Connect(ctx, config.LoadRedis())
	if err != nil {
		log.Error("failed to connect to redis", "error", err.Error())
		os.Exit(1)
	}
	defer func() { _ = rdb.Close() }()

	// --- JWT ---
	jwtCfg := config.LoadJWT()
	jwtManager, err := jwtauth.NewManager(jwtCfg)
	if err != nil {
		log.Error("invalid jwt configuration", "error", err.Error())
		os.Exit(1)
	}

	// --- Wiring (Clean Architecture: repo -> service -> transport) ---
	userRepo := repository.NewUserRepo(pool)
	refreshRepo := repository.NewRefreshRepo(pool)
	svc := authsvc.New(authsvc.Config{
		Users:      userRepo,
		Refresh:    refreshRepo,
		JWT:        jwtManager,
		Argon:      crypto.DefaultArgon2Params(),
		RefreshTTL: jwtCfg.RefreshTTL,
	})
	handler := transport.New(transport.Config{
		Service:       svc,
		OAuth:         oauth.NewRegistry(config.LoadOAuth()),
		States:        repository.NewRedisStateStore(rdb),
		Desktops:      repository.NewRedisDesktopStore(rdb),
		Logger:        log,
		SecureCookies: base.IsProduction(),
	})

	// --- Serve ---
	service.Run(service.Spec{
		Name:        "auth",
		HTTPAddrEnv: "AUTH_HTTP_ADDR",
		DefaultAddr: ":8081",
		Version:     version,
		ReadinessChecks: []httpx.ReadinessCheck{
			{Name: "postgres", Check: postgres.HealthCheck(pool)},
			{Name: "redis", Check: redisx.HealthCheck(rdb)},
		},
	}, func(app *fiber.App, _ service.Deps) {
		handler.Register(app.Group("/api/v1"))
	})
}
