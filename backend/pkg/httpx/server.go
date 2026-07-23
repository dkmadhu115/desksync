// Package httpx builds a consistently-configured Fiber application for every
// DeskSync service. Centralizing this guarantees that all services expose the
// same operational endpoints (/health, /ready, /metrics), propagate a request
// ID / correlation ID, record the standard metrics, and log uniformly.
package httpx

import (
	"context"
	"log/slog"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/observability"
	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/recover"
	"github.com/gofiber/fiber/v2/middleware/requestid"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

// ReadinessCheck reports whether a dependency is ready to serve traffic.
type ReadinessCheck struct {
	Name  string
	Check func(context.Context) error
}

// Options configures the shared Fiber server.
type Options struct {
	Base    config.Base
	Logger  *slog.Logger
	Metrics *observability.Metrics
	// Version is reported by the /health endpoint.
	Version string
	// ReadinessChecks are evaluated on GET /ready; any failure yields 503.
	ReadinessChecks []ReadinessCheck
}

// New constructs a Fiber app with the standard DeskSync middleware stack and
// operational endpoints wired up. Callers register their domain routes on the
// returned *fiber.App before calling Listen.
func New(opts Options) *fiber.App {
	app := fiber.New(fiber.Config{
		AppName:               opts.Base.ServiceName,
		DisableStartupMessage: true,
		ReadTimeout:           15 * time.Second,
		WriteTimeout:          15 * time.Second,
		IdleTimeout:           60 * time.Second,
		// Trust no proxy headers by default; the gateway is the only ingress.
		EnableTrustedProxyCheck: true,
	})

	// Panic recovery keeps a single bad request from crashing the process.
	app.Use(recover.New())
	// Request ID doubles as a correlation ID propagated across services.
	app.Use(requestid.New(requestid.Config{Header: "X-Request-ID"}))
	// Metrics + structured access logging.
	app.Use(metricsMiddleware(opts.Metrics))
	app.Use(accessLogMiddleware(opts.Logger))

	registerOps(app, opts)
	return app
}

// registerOps wires the liveness, readiness, and metrics endpoints.
func registerOps(app *fiber.App, opts Options) {
	app.Get("/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{
			"status":  "ok",
			"service": opts.Base.ServiceName,
			"version": opts.Version,
		})
	})

	// Readiness evaluates each registered dependency check.
	app.Get("/ready", func(c *fiber.Ctx) error {
		ctx, cancel := context.WithTimeout(c.Context(), 3*time.Second)
		defer cancel()
		for _, rc := range opts.ReadinessChecks {
			if err := rc.Check(ctx); err != nil {
				return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{
					"status":     "not_ready",
					"dependency": rc.Name,
				})
			}
		}
		return c.JSON(fiber.Map{"status": "ready"})
	})

	// Expose Prometheus metrics via the service's dedicated registry.
	promHandler := promhttp.HandlerFor(opts.Metrics.Registry, promhttp.HandlerOpts{})
	app.Get("/metrics", adaptHTTP(promHandler.ServeHTTP))
}

// metricsMiddleware records RED metrics for every request.
func metricsMiddleware(m *observability.Metrics) fiber.Handler {
	return func(c *fiber.Ctx) error {
		m.InFlight.Inc()
		start := time.Now()

		err := c.Next()

		m.InFlight.Dec()
		route := c.Route().Path
		method := c.Method()
		status := statusText(c.Response().StatusCode())

		m.RequestsTotal.WithLabelValues(method, route, status).Inc()
		m.RequestDuration.WithLabelValues(method, route).Observe(time.Since(start).Seconds())
		return err
	}
}

// accessLogMiddleware emits one structured log line per request.
func accessLogMiddleware(l *slog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		start := time.Now()
		err := c.Next()
		l.Info("http_request",
			slog.String("method", c.Method()),
			slog.String("path", c.Path()),
			slog.Int("status", c.Response().StatusCode()),
			slog.String("request_id", c.Get("X-Request-ID")),
			slog.Duration("latency", time.Since(start)),
			slog.String("ip", c.IP()),
		)
		return err
	}
}
