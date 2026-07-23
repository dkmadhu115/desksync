// Command pairing brokers the initial trust handshake between a phone and a
// laptop via QR codes or short-lived manual codes, and records persistent
// trusted-device relationships. Full logic arrives in Phase 6.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "pairing",
		HTTPAddrEnv: "PAIRING_HTTP_ADDR",
		DefaultAddr: ":8084",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/pairing/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "pairing"})
		})
	})
}
