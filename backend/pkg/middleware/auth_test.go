package middleware

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/gofiber/fiber/v2"
)

func testManager(t *testing.T) *jwtauth.Manager {
	t.Helper()
	m, err := jwtauth.NewManager(config.JWTConfig{
		AccessSecret:  "0123456789abcdef0123456789abcdef",
		RefreshSecret: "abcdef0123456789abcdef0123456789",
		AccessTTL:     15 * time.Minute,
		RefreshTTL:    time.Hour,
		Issuer:        "desksync-test",
	})
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	return m
}

func newApp(m *jwtauth.Manager) *fiber.App {
	app := fiber.New()
	app.Get("/protected", RequireAuth(m), func(c *fiber.Ctx) error {
		return c.SendString(UserID(c))
	})
	return app
}

func TestRequireAuthRejectsMissingToken(t *testing.T) {
	app := newApp(testManager(t))
	resp, _ := app.Test(httptest.NewRequest(http.MethodGet, "/protected", nil), -1)
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}

func TestRequireAuthAcceptsValidToken(t *testing.T) {
	m := testManager(t)
	app := newApp(m)
	pair, _ := m.Issue("user-42", "jti-1")

	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("Authorization", "Bearer "+pair.AccessToken)
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("Test: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
}

func TestRequireAuthRejectsRefreshToken(t *testing.T) {
	m := testManager(t)
	app := newApp(m)
	pair, _ := m.Issue("user-42", "jti-1")

	// Presenting a refresh token as a bearer must be rejected.
	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("Authorization", "Bearer "+pair.RefreshToken)
	resp, _ := app.Test(req, -1)
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}
}
