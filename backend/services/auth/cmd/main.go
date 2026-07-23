// Command auth issues and validates credentials for DeskSync: email login,
// Google/GitHub OAuth, JWT access tokens, and refresh-token rotation. Full
// logic arrives in Phase 2; Phase 1 boots the service with ops endpoints.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "auth",
		HTTPAddrEnv: "AUTH_HTTP_ADDR",
		DefaultAddr: ":8081",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/auth/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "auth"})
		})
	})
}
