// Command gateway is the single public ingress for DeskSync. It exposes the
// standard ops endpoints (/health, /ready, /metrics) and reverse-proxies the
// public REST API surface to the internal services (auth, device, session,
// pairing). WebSocket signaling is reached by clients directly at the signaling
// service's public URL, so it is intentionally not proxied here.
package main

import (
	"strings"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/service"
	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/proxy"
)

// version is overridden at build time via -ldflags in later phases.
var version = "1.0.0"

func main() {
	upstreams := config.LoadGateway()
	service.Run(service.Spec{
		Name:        "gateway",
		HTTPAddrEnv: "GATEWAY_HTTP_ADDR",
		DefaultAddr: ":8080",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		registerRoutes(app, upstreams)
	})
}

// registerRoutes wires the public API surface to the internal services. Each
// group forwards the original path (e.g. /api/v1/auth/login) verbatim to the
// matching upstream, which mounts the same route.
func registerRoutes(app *fiber.App, up config.GatewayConfig) {
	api := app.Group("/api/v1")

	// Auth (register/login/refresh/logout/oauth) — always sub-paths.
	api.All("/auth/*", forward(up.AuthURL))

	// Devices — exact collection route plus item/sub-resource routes.
	api.All("/devices", forward(up.DeviceURL))
	api.All("/devices/*", forward(up.DeviceURL))

	// Sessions.
	api.All("/sessions", forward(up.SessionURL))
	api.All("/sessions/*", forward(up.SessionURL))

	// Pairing challenges (initiate/confirm) and persistent pairing management.
	api.All("/pairing/*", forward(up.PairingURL))
	api.All("/pairings", forward(up.PairingURL))
	api.All("/pairings/*", forward(up.PairingURL))
}

// forward returns a handler that reverse-proxies the current request to the
// given upstream base URL, preserving the original path, query, method, body,
// and headers. It records the client address in X-Forwarded-For so downstream
// services log the real caller.
func forward(base string) fiber.Handler {
	base = strings.TrimRight(base, "/")
	return func(c *fiber.Ctx) error {
		appendForwardedFor(c)
		target := base + c.OriginalURL()
		if err := proxy.Do(c, target); err != nil {
			return fiber.NewError(fiber.StatusBadGateway, "upstream unavailable")
		}
		// Drop the hop-by-hop Server header from the upstream response.
		c.Response().Header.Del(fiber.HeaderServer)
		return nil
	}
}

// appendForwardedFor appends the client IP to X-Forwarded-For.
func appendForwardedFor(c *fiber.Ctx) {
	ip := c.IP()
	if prior := c.Get(fiber.HeaderXForwardedFor); prior != "" {
		ip = prior + ", " + ip
	}
	c.Request().Header.Set(fiber.HeaderXForwardedFor, ip)
}
