// Command gateway is the single public ingress for DeskSync. In later phases it
// authenticates requests (JWT), enforces rate limits, and reverse-proxies to
// the internal services. In Phase 1 it boots with the standard ops endpoints
// and advertises the (not-yet-implemented) API surface.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

// version is overridden at build time via -ldflags in later phases.
var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "gateway",
		HTTPAddrEnv: "GATEWAY_HTTP_ADDR",
		DefaultAddr: ":8080",
		Version:     version,
	}, registerRoutes)
}

// registerRoutes wires the public API surface. Handlers return 501 until the
// corresponding phase implements them, but the routing contract is stable.
func registerRoutes(app *fiber.App, _ service.Deps) {
	api := app.Group("/api/v1")
	notImplemented := func(c *fiber.Ctx) error {
		return c.Status(fiber.StatusNotImplemented).JSON(fiber.Map{
			"error":   "not_implemented",
			"message": "endpoint will be implemented in a later phase",
			"path":    c.Path(),
		})
	}
	// Auth (Phase 2), devices (Phase 2/6), sessions (Phase 5/7), pairing (Phase 6).
	api.All("/auth/*", notImplemented)
	api.All("/devices/*", notImplemented)
	api.All("/sessions/*", notImplemented)
	api.All("/pairing/*", notImplemented)
}
