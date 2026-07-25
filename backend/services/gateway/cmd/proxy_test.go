package main

import (
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/desksync/backend/pkg/config"
	"github.com/gofiber/fiber/v2"
)

// upstream spins up a fake internal service that records the last request it
// received and replies with a fixed body/status.
type upstream struct {
	server     *httptest.Server
	lastPath   string
	lastMethod string
	lastBody   string
	lastXFF    string
	lastAuth   string
}

func newUpstream(t *testing.T, status int, body string) *upstream {
	t.Helper()
	u := &upstream{}
	u.server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		u.lastPath = r.URL.RequestURI()
		u.lastMethod = r.Method
		u.lastXFF = r.Header.Get("X-Forwarded-For")
		u.lastAuth = r.Header.Get("Authorization")
		b, _ := io.ReadAll(r.Body)
		u.lastBody = string(b)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = w.Write([]byte(body))
	}))
	t.Cleanup(u.server.Close)
	return u
}

func newGateway(t *testing.T, up config.GatewayConfig) *fiber.App {
	t.Helper()
	app := fiber.New()
	registerRoutes(app, up)
	return app
}

func send(t *testing.T, app *fiber.App, method, path, body string, headers map[string]string) (*http.Response, string) {
	t.Helper()
	var r io.Reader
	if body != "" {
		r = strings.NewReader(body)
	}
	req := httptest.NewRequest(method, path, r)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	raw, _ := io.ReadAll(resp.Body)
	_ = resp.Body.Close()
	return resp, string(raw)
}

func TestForwardsAuthPreservingPathBodyAndHeaders(t *testing.T) {
	auth := newUpstream(t, http.StatusOK, `{"access_token":"x"}`)
	app := newGateway(t, config.GatewayConfig{AuthURL: auth.server.URL})

	resp, body := send(t, app, http.MethodPost, "/api/v1/auth/login",
		`{"email":"a@b.c"}`, map[string]string{
			"Content-Type":  "application/json",
			"Authorization": "Bearer tok",
		})

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	if body != `{"access_token":"x"}` {
		t.Fatalf("body = %q, want upstream body echoed", body)
	}
	if auth.lastPath != "/api/v1/auth/login" {
		t.Fatalf("upstream path = %q, want /api/v1/auth/login", auth.lastPath)
	}
	if auth.lastMethod != http.MethodPost {
		t.Fatalf("method = %q, want POST", auth.lastMethod)
	}
	if auth.lastBody != `{"email":"a@b.c"}` {
		t.Fatalf("body forwarded = %q", auth.lastBody)
	}
	if auth.lastAuth != "Bearer tok" {
		t.Fatalf("Authorization not forwarded: %q", auth.lastAuth)
	}
	if auth.lastXFF == "" {
		t.Fatal("X-Forwarded-For should be set")
	}
}

func TestForwardsQueryString(t *testing.T) {
	sess := newUpstream(t, http.StatusOK, `[]`)
	app := newGateway(t, config.GatewayConfig{SessionURL: sess.server.URL})
	send(t, app, http.MethodGet, "/api/v1/sessions?limit=5", "", nil)
	if sess.lastPath != "/api/v1/sessions?limit=5" {
		t.Fatalf("upstream path = %q, want query preserved", sess.lastPath)
	}
}

func TestRoutesEachPrefixToItsUpstream(t *testing.T) {
	auth := newUpstream(t, http.StatusOK, `auth`)
	device := newUpstream(t, http.StatusOK, `device`)
	session := newUpstream(t, http.StatusOK, `session`)
	pairing := newUpstream(t, http.StatusOK, `pairing`)
	app := newGateway(t, config.GatewayConfig{
		AuthURL:    auth.server.URL,
		DeviceURL:  device.server.URL,
		SessionURL: session.server.URL,
		PairingURL: pairing.server.URL,
	})

	cases := []struct {
		method, path, wantBody string
	}{
		{http.MethodPost, "/api/v1/auth/register", "auth"},
		{http.MethodGet, "/api/v1/devices", "device"},
		{http.MethodGet, "/api/v1/devices/abc", "device"},
		{http.MethodPost, "/api/v1/sessions", "session"},
		{http.MethodPost, "/api/v1/sessions/s1/end", "session"},
		{http.MethodPost, "/api/v1/pairing/initiate", "pairing"},
		{http.MethodGet, "/api/v1/pairings", "pairing"},
		{http.MethodDelete, "/api/v1/pairings/p1", "pairing"},
	}
	for _, tc := range cases {
		t.Run(tc.method+" "+tc.path, func(t *testing.T) {
			resp, body := send(t, app, tc.method, tc.path, "", nil)
			if resp.StatusCode != http.StatusOK {
				t.Fatalf("status = %d, want 200", resp.StatusCode)
			}
			if body != tc.wantBody {
				t.Fatalf("routed to wrong upstream: body = %q, want %q", body, tc.wantBody)
			}
		})
	}
}

func TestUpstreamDownReturnsBadGateway(t *testing.T) {
	// Point at a closed port (reserved, nothing listening).
	app := newGateway(t, config.GatewayConfig{AuthURL: "http://127.0.0.1:1"})
	resp, _ := send(t, app, http.MethodPost, "/api/v1/auth/login", `{}`, nil)
	if resp.StatusCode != http.StatusBadGateway {
		t.Fatalf("status = %d, want 502", resp.StatusCode)
	}
}

func TestHealthEndpointIsLocalNotProxied(t *testing.T) {
	// registerRoutes only wires /api/v1/*; /health is provided by the shared
	// server, so an unrouted path here should 404 (not hit an upstream).
	app := newGateway(t, config.GatewayConfig{})
	resp, _ := send(t, app, http.MethodGet, "/api/v1/unknown", "", nil)
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 for unrouted path", resp.StatusCode)
	}
}
