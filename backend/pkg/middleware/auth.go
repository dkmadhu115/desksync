// Package middleware provides reusable Fiber middleware shared by services and
// the gateway, starting with JWT bearer authentication.
package middleware

import (
	"strings"

	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/gofiber/fiber/v2"
)

// LocalsUserID is the Fiber locals key under which the authenticated user id is
// stored for downstream handlers.
const LocalsUserID = "user_id"

// RequireAuth returns middleware that requires a valid access-token bearer
// header. On success it stores the user id in c.Locals(LocalsUserID).
func RequireAuth(m *jwtauth.Manager) fiber.Handler {
	return func(c *fiber.Ctx) error {
		token, ok := bearerToken(c)
		if !ok {
			return unauthorized(c, "missing bearer token")
		}
		claims, err := m.VerifyAccess(token)
		if err != nil {
			return unauthorized(c, "invalid or expired token")
		}
		c.Locals(LocalsUserID, claims.UserID)
		return c.Next()
	}
}

// UserID returns the authenticated user id previously set by RequireAuth.
func UserID(c *fiber.Ctx) string {
	if v, ok := c.Locals(LocalsUserID).(string); ok {
		return v
	}
	return ""
}

func bearerToken(c *fiber.Ctx) (string, bool) {
	h := c.Get(fiber.HeaderAuthorization)
	const prefix = "Bearer "
	if len(h) <= len(prefix) || !strings.EqualFold(h[:len(prefix)], prefix) {
		return "", false
	}
	return strings.TrimSpace(h[len(prefix):]), true
}

func unauthorized(c *fiber.Ctx, msg string) error {
	return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
		"error":      "unauthorized",
		"message":    msg,
		"request_id": c.Get("X-Request-ID"),
	})
}
