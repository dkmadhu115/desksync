// Command signaling relays WebRTC signaling (SDP offer/answer and ICE
// candidates) between paired peers over authenticated, secure WebSockets. It
// never sees decrypted media; it only brokers connection setup. Full logic
// arrives in Phase 5.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "signaling",
		HTTPAddrEnv: "SIGNALING_HTTP_ADDR",
		DefaultAddr: ":8085",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/signaling/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "signaling"})
		})
	})
}
