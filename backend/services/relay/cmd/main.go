// Command relay is the control plane for the TURN relay (Coturn). It mints
// short-lived, HMAC-based TURN credentials for peers that cannot establish a
// direct connection, so media falls back through the relay. Coturn itself runs
// as a separate process/container; this service only issues credentials.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "relay",
		HTTPAddrEnv: "RELAY_HTTP_ADDR",
		DefaultAddr: ":8086",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/relay/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "relay"})
		})
	})
}
