// Command session owns remote-control session lifecycle: creation, timeouts,
// termination, and the append-only session event/audit log. Full logic arrives
// in Phases 5/7.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "session",
		HTTPAddrEnv: "SESSION_HTTP_ADDR",
		DefaultAddr: ":8083",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/sessions/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "session"})
		})
	})
}
