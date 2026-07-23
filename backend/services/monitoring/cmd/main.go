// Command monitoring provides an internal control-plane surface for the
// observability stack: aggregate health, synthetic checks, and alert routing
// hooks. The heavy lifting (metrics storage, dashboards, log aggregation) is
// done by Prometheus, Grafana, and Loki; see the monitoring/ directory and
// docs/adr/0002-monitoring-is-infrastructure.md for the rationale.
package main

import (
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
)

var version = "0.1.0-phase1"

func main() {
	service.Run(service.Spec{
		Name:        "monitoring",
		HTTPAddrEnv: "MONITORING_HTTP_ADDR",
		DefaultAddr: ":8088",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		app.All("/api/v1/monitoring/*", func(c *fiber.Ctx) error {
			return c.Status(fiber.StatusNotImplemented).
				JSON(fiber.Map{"error": "not_implemented", "service": "monitoring"})
		})
	})
}
