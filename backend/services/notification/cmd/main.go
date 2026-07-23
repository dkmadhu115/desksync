// Command notification delivers push notifications to mobile devices (via
// Firebase Cloud Messaging) for events such as incoming connection requests,
// session start/stop, and security alerts. Full logic arrives in later phases.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "notification",
		HTTPAddrEnv: "NOTIFICATION_HTTP_ADDR",
		DefaultAddr: ":8087",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/notifications/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "notification"})
		})
	})
}
