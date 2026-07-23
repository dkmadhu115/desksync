// Package service provides the canonical bootstrap sequence shared by every
// DeskSync microservice: load configuration, build the logger and metrics,
// construct the Fiber app with the standard middleware/ops endpoints, register
// the service's domain routes, and run with graceful shutdown on SIGINT/SIGTERM.
//
// Keeping this in one place means each service's main.go is a few lines and all
// services behave identically operationally.
package service

import (
	"context"
	"log/slog"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/httpx"
	"github.com/desksync/backend/pkg/logger"
	"github.com/desksync/backend/pkg/observability"
	"github.com/gofiber/fiber/v2"
)

// Spec describes a service's identity and network binding.
type Spec struct {
	// Name is the canonical service name (e.g. "gateway").
	Name string
	// HTTPAddrEnv is the env var holding the listen address.
	HTTPAddrEnv string
	// DefaultAddr is used when HTTPAddrEnv is unset (e.g. ":8080").
	DefaultAddr string
	// Version is reported by /health; injected at build time in later phases.
	Version string
	// ShutdownTimeout bounds graceful shutdown; defaults to 15s when zero.
	ShutdownTimeout time.Duration
}

// RegisterFunc lets a service attach its domain routes to the shared app.
type RegisterFunc func(app *fiber.App, deps Deps)

// Deps exposes shared infrastructure to a service's route registration.
type Deps struct {
	Config  config.Base
	Logger  *slog.Logger
	Metrics *observability.Metrics
}

// Run bootstraps and runs the service until a termination signal is received.
// It blocks until the server has shut down (gracefully or on error).
func Run(spec Spec, register RegisterFunc) {
	if spec.Version == "" {
		spec.Version = "dev"
	}
	if spec.ShutdownTimeout == 0 {
		spec.ShutdownTimeout = 15 * time.Second
	}

	base := config.LoadBase(spec.Name, spec.HTTPAddrEnv, spec.DefaultAddr)
	log := logger.New(logger.Options{
		ServiceName: base.ServiceName,
		Level:       base.LogLevel,
		Format:      base.LogFormat,
	})
	metrics := observability.NewMetrics(base.ServiceName)

	app := httpx.New(httpx.Options{
		Base:    base,
		Logger:  log,
		Metrics: metrics,
		Version: spec.Version,
	})

	if register != nil {
		register(app, Deps{Config: base, Logger: log, Metrics: metrics})
	}

	// Run the listener in a goroutine so we can wait for signals.
	errCh := make(chan error, 1)
	go func() {
		log.Info("starting http server",
			slog.String("service", base.ServiceName),
			slog.String("addr", base.HTTPAddr),
			slog.String("version", spec.Version),
		)
		errCh <- app.Listen(base.HTTPAddr)
	}()

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	select {
	case err := <-errCh:
		if err != nil {
			log.Error("http server failed", slog.String("error", err.Error()))
			os.Exit(1)
		}
	case <-ctx.Done():
		log.Info("shutdown signal received, draining connections")
		shutdownCtx, cancel := context.WithTimeout(context.Background(), spec.ShutdownTimeout)
		defer cancel()
		if err := app.ShutdownWithContext(shutdownCtx); err != nil {
			log.Error("graceful shutdown failed", slog.String("error", err.Error()))
			os.Exit(1)
		}
		log.Info("shutdown complete")
	}
}
