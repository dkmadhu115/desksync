package transport

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/crypto"
	"github.com/desksync/backend/services/auth/internal/domain"
	"github.com/gofiber/fiber/v2"
)

// ---- fake desktop store ----

type fakeDesktops struct {
	flows  map[string]domain.DesktopFlow
	grants map[string]domain.DesktopGrant
}

func newFakeDesktops() *fakeDesktops {
	return &fakeDesktops{
		flows:  map[string]domain.DesktopFlow{},
		grants: map[string]domain.DesktopGrant{},
	}
}

func (f *fakeDesktops) SaveFlow(_ context.Context, state string, flow domain.DesktopFlow, _ time.Duration) error {
	f.flows[state] = flow
	return nil
}

func (f *fakeDesktops) ConsumeFlow(_ context.Context, state string) (domain.DesktopFlow, bool, error) {
	flow, ok := f.flows[state]
	if !ok {
		return domain.DesktopFlow{}, false, nil
	}
	delete(f.flows, state)
	return flow, true, nil
}

func (f *fakeDesktops) SaveGrant(_ context.Context, code string, grant domain.DesktopGrant, _ time.Duration) error {
	f.grants[code] = grant
	return nil
}

func (f *fakeDesktops) ConsumeGrant(_ context.Context, code string) (domain.DesktopGrant, error) {
	grant, ok := f.grants[code]
	if !ok {
		return domain.DesktopGrant{}, errors.New("grant not found")
	}
	delete(f.grants, code)
	return grant, nil
}

// ---- exchange endpoint ----

const exchangePath = "/api/v1/auth/oauth/desktop/exchange"

// seedGrant creates a user and a pending grant bound to verifier's challenge,
// mimicking what the OAuth callback stores after a successful consent screen.
func seedGrant(t *testing.T, env testEnv, verifier string) string {
	t.Helper()
	user, err := env.users.CreateUser(context.Background(), domain.User{
		Email:       "desktop@example.com",
		DisplayName: "Desktop",
		IsActive:    true,
	})
	if err != nil {
		t.Fatalf("create user: %v", err)
	}
	code := "one-time-code"
	grant := domain.DesktopGrant{UserID: user.ID, CodeChallenge: crypto.S256Challenge(verifier)}
	if err := env.desktops.SaveGrant(context.Background(), code, grant, time.Minute); err != nil {
		t.Fatalf("save grant: %v", err)
	}
	return code
}

func TestDesktopExchangeIssuesTokens(t *testing.T) {
	env := newTestEnv(t, newFakeDesktops())
	const verifier = "a-high-entropy-code-verifier-value"
	code := seedGrant(t, env, verifier)

	status, raw := do(t, env.app, fiber.MethodPost, exchangePath,
		`{"code":"`+code+`","code_verifier":"`+verifier+`"}`)
	if status != fiber.StatusOK {
		t.Fatalf("status = %d, want 200 (body=%s)", status, raw)
	}
	access, refresh := tokens(t, raw)
	if access == "" || refresh == "" {
		t.Fatal("exchange did not return both tokens")
	}
}

func TestDesktopExchangeRejectsWrongVerifier(t *testing.T) {
	env := newTestEnv(t, newFakeDesktops())
	code := seedGrant(t, env, "the-real-verifier-value-here")

	status, raw := do(t, env.app, fiber.MethodPost, exchangePath,
		`{"code":"`+code+`","code_verifier":"a-different-verifier-value"}`)
	if status != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401 (body=%s)", status, raw)
	}
}

func TestDesktopExchangeCodeIsSingleUse(t *testing.T) {
	env := newTestEnv(t, newFakeDesktops())
	const verifier = "a-high-entropy-code-verifier-value"
	code := seedGrant(t, env, verifier)
	body := `{"code":"` + code + `","code_verifier":"` + verifier + `"}`

	if status, raw := do(t, env.app, fiber.MethodPost, exchangePath, body); status != fiber.StatusOK {
		t.Fatalf("first exchange status = %d, want 200 (body=%s)", status, raw)
	}
	// Replaying the same code must fail: the grant was consumed.
	if status, _ := do(t, env.app, fiber.MethodPost, exchangePath, body); status != fiber.StatusUnauthorized {
		t.Fatalf("replayed exchange status = %d, want 401", status)
	}
}

func TestDesktopExchangeUnknownCodeUnauthorized(t *testing.T) {
	env := newTestEnv(t, newFakeDesktops())
	status, _ := do(t, env.app, fiber.MethodPost, exchangePath,
		`{"code":"nope","code_verifier":"whatever-value-here"}`)
	if status != fiber.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", status)
	}
}

func TestDesktopExchangeValidatesBody(t *testing.T) {
	env := newTestEnv(t, newFakeDesktops())
	cases := map[string]string{
		"missing verifier": `{"code":"c"}`,
		"missing code":     `{"code_verifier":"v"}`,
		"empty object":     `{}`,
		"not json":         `garbage`,
	}
	for name, body := range cases {
		t.Run(name, func(t *testing.T) {
			if status, _ := do(t, env.app, fiber.MethodPost, exchangePath, body); status != fiber.StatusBadRequest {
				t.Fatalf("status = %d, want 400", status)
			}
		})
	}
}

func TestDesktopExchangeDisabledWithoutStore(t *testing.T) {
	app := newTestApp(t) // no desktop store wired
	status, _ := do(t, app, fiber.MethodPost, exchangePath,
		`{"code":"c","code_verifier":"v"}`)
	if status != fiber.StatusNotFound {
		t.Fatalf("status = %d, want 404 when desktop sign-in is not enabled", status)
	}
}

// ---- native-client parameter parsing ----

// parseFlow exercises parseDesktopFlow through a throwaway route, since it needs
// a real *fiber.Ctx to read the query string.
func parseFlow(t *testing.T, query string) (int, domain.DesktopFlow, bool) {
	t.Helper()
	app := fiber.New()
	app.Get("/t", func(c *fiber.Ctx) error {
		flow, found, err := parseDesktopFlow(c)
		if err != nil {
			return respondError(c, err)
		}
		return c.JSON(fiber.Map{
			"found":     found,
			"port":      flow.RedirectPort,
			"challenge": flow.CodeChallenge,
		})
	})

	status, raw := do(t, app, fiber.MethodGet, "/t"+query, "")
	var body struct {
		Found     bool   `json:"found"`
		Port      int    `json:"port"`
		Challenge string `json:"challenge"`
	}
	if status == fiber.StatusOK {
		if err := json.Unmarshal(raw, &body); err != nil {
			t.Fatalf("unmarshal: %v (body=%s)", err, raw)
		}
	}
	return status, domain.DesktopFlow{RedirectPort: body.Port, CodeChallenge: body.Challenge}, body.Found
}

func TestParseDesktopFlowAcceptsValidParams(t *testing.T) {
	challenge := crypto.S256Challenge("some-verifier")
	status, flow, found := parseFlow(t, "?redirect_port=49152&code_challenge="+challenge)
	if status != fiber.StatusOK {
		t.Fatalf("status = %d, want 200", status)
	}
	if !found {
		t.Fatal("expected a desktop flow to be detected")
	}
	if flow.RedirectPort != 49152 || flow.CodeChallenge != challenge {
		t.Fatalf("flow = %+v, want port 49152 and the supplied challenge", flow)
	}
}

func TestParseDesktopFlowAbsentIsBrowserFlow(t *testing.T) {
	status, _, found := parseFlow(t, "")
	if status != fiber.StatusOK {
		t.Fatalf("status = %d, want 200", status)
	}
	if found {
		t.Fatal("a request without native params must not be treated as a desktop flow")
	}
}

func TestParseDesktopFlowRejectsBadParams(t *testing.T) {
	valid := crypto.S256Challenge("some-verifier")
	cases := map[string]string{
		"port without challenge": "?redirect_port=49152",
		"challenge without port": "?code_challenge=" + valid,
		"non-numeric port":       "?redirect_port=abc&code_challenge=" + valid,
		"privileged port":        "?redirect_port=80&code_challenge=" + valid,
		"port out of range":      "?redirect_port=70000&code_challenge=" + valid,
		"challenge too short":    "?redirect_port=49152&code_challenge=tooshort",
		"challenge bad charset":  "?redirect_port=49152&code_challenge=" + valid[:42] + "*",
	}
	for name, query := range cases {
		t.Run(name, func(t *testing.T) {
			if status, _, _ := parseFlow(t, query); status != fiber.StatusBadRequest {
				t.Fatalf("status = %d, want 400", status)
			}
		})
	}
}
