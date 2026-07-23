// Command device manages the lifecycle of registered devices (laptops and
// phones): registration, public-key/certificate storage, online/offline
// presence, heartbeats, and revocation. Full logic arrives in later phases.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "device",
		HTTPAddrEnv: "DEVICE_HTTP_ADDR",
		DefaultAddr: ":8082",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/devices/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "device"})
		})
	})
}
