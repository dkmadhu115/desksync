// Command pairing brokers the initial trust handshake between a phone and a
// laptop via QR codes or short-lived manual codes, and records the resulting
// persistent, trusted pairings. Pending challenges live in Redis (short-lived,
// single-use); confirmed pairings are persisted in PostgreSQL.
package main

import (
	"context"
	"os"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/httpx"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/pkg/logger"
	"github.com/desksync/backend/pkg/postgres"
	"github.com/desksync/backend/pkg/redisx"
	"github.com/desksync/backend/pkg/service"
	"github.com/desksync/backend/services/pairing/internal/repository"
	pairingsvc "github.com/desksync/backend/services/pairing/internal/service"
	"github.com/desksync/backend/services/pairing/internal/store"
	"github.com/desksync/backend/services/pairing/internal/transport"
	"github.com/gofiber/fiber/v2"
)

var version = "0.4.0-phase6"

func main() {
	base := config.LoadBase("pairing", "PAIRING_HTTP_ADDR", ":8084")
	log := logger.New(logger.Options{ServiceName: base.ServiceName, Level: base.LogLevel, Format: base.LogFormat})

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

	jwtManager, err := jwtauth.NewManager(config.LoadJWT())
	if err != nil {
		log.Error("invalid jwt configuration", "error", err.Error())
		os.Exit(1)
	}

	svc := pairingsvc.New(pairingsvc.Config{
		Repo:   repository.New(pool),
		Store:  store.New(rdb),
		Logger: log,
	})
	handler := transport.New(svc, jwtManager)

	service.Run(service.Spec{
		Name:        "pairing",
		HTTPAddrEnv: "PAIRING_HTTP_ADDR",
		DefaultAddr: ":8084",
		Version:     version,
		ReadinessChecks: []httpx.ReadinessCheck{
			{Name: "postgres", Check: postgres.HealthCheck(pool)},
			{Name: "redis", Check: redisx.HealthCheck(rdb)},
		},
	}, func(app *fiber.App, _ service.Deps) {
		handler.Register(app.Group("/api/v1"))
	})
}
