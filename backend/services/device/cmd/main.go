// Command device manages the lifecycle of registered devices (laptops and
// phones): registration and public-key storage, online/offline presence via
// heartbeats, and revocation (which cascades to the device's pairings).
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
	"github.com/desksync/backend/pkg/service"
	"github.com/desksync/backend/services/device/internal/repository"
	devicesvc "github.com/desksync/backend/services/device/internal/service"
	"github.com/desksync/backend/services/device/internal/transport"
	"github.com/gofiber/fiber/v2"
)

var version = "0.4.0-phase6"

func main() {
	base := config.LoadBase("device", "DEVICE_HTTP_ADDR", ":8082")
	log := logger.New(logger.Options{ServiceName: base.ServiceName, Level: base.LogLevel, Format: base.LogFormat})

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pool, err := postgres.Connect(ctx, config.LoadPostgres())
	if err != nil {
		log.Error("failed to connect to postgres", "error", err.Error())
		os.Exit(1)
	}
	defer pool.Close()

	jwtManager, err := jwtauth.NewManager(config.LoadJWT())
	if err != nil {
		log.Error("invalid jwt configuration", "error", err.Error())
		os.Exit(1)
	}

	svc := devicesvc.New(devicesvc.Config{
		Repo:   repository.New(pool),
		Logger: log,
	})
	handler := transport.New(svc, jwtManager)

	service.Run(service.Spec{
		Name:        "device",
		HTTPAddrEnv: "DEVICE_HTTP_ADDR",
		DefaultAddr: ":8082",
		Version:     version,
		ReadinessChecks: []httpx.ReadinessCheck{
			{Name: "postgres", Check: postgres.HealthCheck(pool)},
		},
	}, func(app *fiber.App, _ service.Deps) {
		handler.Register(app.Group("/api/v1"))
	})
}
