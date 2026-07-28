// Package transport exposes the auth service over HTTP (Fiber). It maps
// requests to the application service and translates domain/application errors
// into the uniform JSON error envelope.
package transport

import (
	"context"
	"fmt"
	"log/slog"
	"net/url"
	"strconv"
	"time"

	"github.com/desksync/backend/pkg/crypto"
	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/services/auth/internal/domain"
	"github.com/desksync/backend/services/auth/internal/oauth"
	"github.com/desksync/backend/services/auth/internal/service"
	"github.com/gofiber/fiber/v2"
)

// StateStore persists short-lived OAuth PKCE verifiers keyed by state.
type StateStore interface {
	Save(ctx context.Context, state, verifier string, ttl time.Duration) error
	Consume(ctx context.Context, state string) (string, error)
}

// DesktopStore persists desktop sign-in flows (keyed by the OAuth state) and the
// one-time grants they produce (keyed by an opaque code). Both are single-use.
type DesktopStore interface {
	SaveFlow(ctx context.Context, state string, flow domain.DesktopFlow, ttl time.Duration) error
	// ConsumeFlow returns the flow for a state. found is false for ordinary
	// browser sign-ins, which have no desktop context.
	ConsumeFlow(ctx context.Context, state string) (flow domain.DesktopFlow, found bool, err error)
	SaveGrant(ctx context.Context, code string, grant domain.DesktopGrant, ttl time.Duration) error
	ConsumeGrant(ctx context.Context, code string) (domain.DesktopGrant, error)
}

// Lifetimes for the two halves of a sign-in: the browser leg may involve typing
// a password and 2FA, while redemption by an already-waiting local process is
// nearly instant.
const (
	oauthStateTTL    = 10 * time.Minute
	desktopGrantTTL  = 2 * time.Minute
	minLoopbackPort  = 1024
	minCodeChallenge = 43
	maxCodeChallenge = 128
)

// Handler holds the dependencies for the auth HTTP endpoints.
type Handler struct {
	svc           *service.Service
	oauth         *oauth.Registry
	states        StateStore
	desktops      DesktopStore
	log           *slog.Logger
	secureCookies bool
}

// Config configures a Handler.
type Config struct {
	Service       *service.Service
	OAuth         *oauth.Registry
	States        StateStore
	Desktops      DesktopStore
	Logger        *slog.Logger
	SecureCookies bool
}

// New builds a Handler.
func New(c Config) *Handler {
	return &Handler{
		svc:           c.Service,
		oauth:         c.OAuth,
		states:        c.States,
		desktops:      c.Desktops,
		log:           c.Logger,
		secureCookies: c.SecureCookies,
	}
}

// Register mounts the auth routes on the given router group (mounted at
// /api/v1 by the shared server).
func (h *Handler) Register(r fiber.Router) {
	g := r.Group("/auth")
	g.Post("/register", h.register)
	g.Post("/login", h.login)
	g.Post("/refresh", h.refresh)
	g.Post("/logout", h.logout)
	// Registered before the :provider routes so the literal path always wins.
	g.Post("/oauth/desktop/exchange", h.oauthDesktopExchange)
	g.Get("/oauth/:provider/start", h.oauthStart)
	g.Get("/oauth/:provider/callback", h.oauthCallback)
}

func (h *Handler) meta(c *fiber.Ctx) service.Metadata {
	return service.Metadata{UserAgent: c.Get("User-Agent"), IPAddress: c.IP()}
}

func (h *Handler) register(c *fiber.Ctx) error {
	var req registerRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	tokens, err := h.svc.Register(c.Context(), req.Email, req.Password, req.DisplayName, h.meta(c))
	if err != nil {
		return respondError(c, err)
	}
	return c.Status(fiber.StatusCreated).JSON(toTokenResponse(tokens))
}

func (h *Handler) login(c *fiber.Ctx) error {
	var req loginRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	tokens, err := h.svc.Login(c.Context(), req.Email, req.Password, h.meta(c))
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toTokenResponse(tokens))
}

func (h *Handler) refresh(c *fiber.Ctx) error {
	var req refreshRequest
	if err := c.BodyParser(&req); err != nil || req.RefreshToken == "" {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "refresh_token is required"))
	}
	tokens, err := h.svc.Refresh(c.Context(), req.RefreshToken, h.meta(c))
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toTokenResponse(tokens))
}

func (h *Handler) logout(c *fiber.Ctx) error {
	var req refreshRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	if err := h.svc.Logout(c.Context(), req.RefreshToken); err != nil {
		return respondError(c, err)
	}
	return c.SendStatus(fiber.StatusNoContent)
}

// oauthStart redirects the user agent to the provider's consent screen.
//
// Native clients additionally pass `redirect_port` and `code_challenge`, which
// turns this into a desktop sign-in: the result is handed back to a loopback
// listener on the user's machine instead of being rendered in the browser. Note
// the provider itself always redirects to *this* service's registered callback,
// so the provider client secret never leaves the backend.
func (h *Handler) oauthStart(c *fiber.Ctx) error {
	provider := domain.Provider(c.Params("provider"))
	prov, ok := h.oauth.Get(provider)
	if !ok {
		return respondError(c, apperr.New(apperr.CodeNotFound, "oauth provider not configured"))
	}
	flow, isDesktop, err := parseDesktopFlow(c)
	if err != nil {
		return respondError(c, err)
	}

	state, err := crypto.GenerateToken(24)
	if err != nil {
		return respondError(c, apperr.Wrap(apperr.CodeInternal, "state generation failed", err))
	}
	verifier, err := crypto.GenerateToken(32)
	if err != nil {
		return respondError(c, apperr.Wrap(apperr.CodeInternal, "verifier generation failed", err))
	}
	if err := h.states.Save(c.Context(), state, verifier, oauthStateTTL); err != nil {
		return respondError(c, apperr.Wrap(apperr.CodeInternal, "failed to persist oauth state", err))
	}
	if isDesktop {
		if h.desktops == nil {
			return respondError(c, apperr.New(apperr.CodeNotFound, "desktop sign-in is not enabled"))
		}
		if err := h.desktops.SaveFlow(c.Context(), state, flow, oauthStateTTL); err != nil {
			return respondError(c, apperr.Wrap(apperr.CodeInternal, "failed to persist desktop flow", err))
		}
	}
	return c.Redirect(prov.AuthCodeURL(state, verifier), fiber.StatusFound)
}

// parseDesktopFlow extracts and validates the native-client parameters. It
// returns found=false when neither is present (an ordinary browser sign-in) and
// an error when only one is present or a value is out of range.
func parseDesktopFlow(c *fiber.Ctx) (domain.DesktopFlow, bool, error) {
	portRaw := c.Query("redirect_port")
	challenge := c.Query("code_challenge")
	if portRaw == "" && challenge == "" {
		return domain.DesktopFlow{}, false, nil
	}
	if portRaw == "" || challenge == "" {
		return domain.DesktopFlow{}, false, apperr.New(apperr.CodeInvalidInput,
			"redirect_port and code_challenge must be provided together")
	}
	port, err := strconv.Atoi(portRaw)
	if err != nil || port < minLoopbackPort || port > 65535 {
		return domain.DesktopFlow{}, false, apperr.New(apperr.CodeInvalidInput,
			"redirect_port must be a port in 1024..65535")
	}
	if len(challenge) < minCodeChallenge || len(challenge) > maxCodeChallenge || !isBase64URL(challenge) {
		return domain.DesktopFlow{}, false, apperr.New(apperr.CodeInvalidInput,
			"code_challenge must be an unpadded base64url S256 challenge")
	}
	return domain.DesktopFlow{RedirectPort: port, CodeChallenge: challenge}, true, nil
}

// isBase64URL reports whether s contains only unpadded base64url characters.
// Applied to the challenge before it is echoed into a redirect URL.
func isBase64URL(s string) bool {
	for _, r := range s {
		switch {
		case r >= 'A' && r <= 'Z', r >= 'a' && r <= 'z', r >= '0' && r <= '9', r == '-', r == '_':
		default:
			return false
		}
	}
	return true
}

func (h *Handler) oauthCallback(c *fiber.Ctx) error {
	provider := domain.Provider(c.Params("provider"))
	prov, ok := h.oauth.Get(provider)
	if !ok {
		return respondError(c, apperr.New(apperr.CodeNotFound, "oauth provider not configured"))
	}
	state := c.Query("state")
	code := c.Query("code")
	if state == "" || code == "" {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "missing state or code"))
	}

	// Resolve the desktop context first: if anything below fails we can still
	// send the browser back to the waiting local listener with an error, instead
	// of leaving the desktop app hanging until it times out.
	var (
		flow      domain.DesktopFlow
		isDesktop bool
	)
	if h.desktops != nil {
		var err error
		if flow, isDesktop, err = h.desktops.ConsumeFlow(c.Context(), state); err != nil {
			h.logWarn("failed to read desktop sign-in flow", err)
		}
	}

	verifier, err := h.states.Consume(c.Context(), state)
	if err != nil {
		return h.callbackError(c, flow, isDesktop,
			apperr.New(apperr.CodeUnauthorized, "invalid or expired oauth state"))
	}
	info, err := prov.Exchange(c.Context(), code, verifier)
	if err != nil {
		return h.callbackError(c, flow, isDesktop,
			apperr.Wrap(apperr.CodeUnauthorized, "oauth exchange failed", err))
	}
	if info.Email == "" {
		return h.callbackError(c, flow, isDesktop,
			apperr.New(apperr.CodeInvalidInput, "provider did not return a verified email"))
	}

	if !isDesktop {
		tokens, err := h.svc.UpsertOAuthUser(c.Context(), prov.Name(), info.ProviderUserID, info.Email, info.Name, h.meta(c))
		if err != nil {
			return respondError(c, err)
		}
		return c.JSON(toTokenResponse(tokens))
	}

	// Desktop flow: resolve the account but mint nothing here. Hand the desktop
	// a one-time code it redeems with its PKCE verifier.
	user, err := h.svc.ResolveOAuthUser(c.Context(), prov.Name(), info.ProviderUserID, info.Email, info.Name)
	if err != nil {
		return h.callbackError(c, flow, isDesktop, err)
	}
	grantCode, err := crypto.GenerateToken(32)
	if err != nil {
		return h.callbackError(c, flow, isDesktop,
			apperr.Wrap(apperr.CodeInternal, "code generation failed", err))
	}
	grant := domain.DesktopGrant{UserID: user.ID, CodeChallenge: flow.CodeChallenge}
	if err := h.desktops.SaveGrant(c.Context(), grantCode, grant, desktopGrantTTL); err != nil {
		return h.callbackError(c, flow, isDesktop,
			apperr.Wrap(apperr.CodeInternal, "failed to persist sign-in result", err))
	}
	return c.Redirect(loopbackURL(flow.RedirectPort, "code", grantCode), fiber.StatusFound)
}

// oauthDesktopExchange redeems a one-time desktop sign-in code for a token pair.
// The PKCE verifier must hash to the challenge supplied when the flow started,
// proving the caller is the process that initiated it.
func (h *Handler) oauthDesktopExchange(c *fiber.Ctx) error {
	if h.desktops == nil {
		return respondError(c, apperr.New(apperr.CodeNotFound, "desktop sign-in is not enabled"))
	}
	var req desktopExchangeRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	if req.Code == "" || req.CodeVerifier == "" {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "code and code_verifier are required"))
	}
	grant, err := h.desktops.ConsumeGrant(c.Context(), req.Code)
	if err != nil {
		return respondError(c, apperr.New(apperr.CodeUnauthorized, "invalid or expired code"))
	}
	if !crypto.EqualS256Challenge(req.CodeVerifier, grant.CodeChallenge) {
		return respondError(c, apperr.New(apperr.CodeUnauthorized, "code_verifier does not match"))
	}
	tokens, err := h.svc.IssueForUserID(c.Context(), grant.UserID, h.meta(c))
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toTokenResponse(tokens))
}

// callbackError reports a callback failure to whoever is waiting: the desktop's
// loopback listener for a native sign-in, otherwise the browser directly.
func (h *Handler) callbackError(c *fiber.Ctx, flow domain.DesktopFlow, isDesktop bool, err error) error {
	if !isDesktop {
		return respondError(c, err)
	}
	message := "sign-in failed"
	if de, ok := apperr.As(err); ok {
		message = de.Message
	}
	return c.Redirect(loopbackURL(flow.RedirectPort, "error", message), fiber.StatusFound)
}

// loopbackURL builds the desktop callback URL. Loopback is hard-coded (only the
// port is caller-supplied) so this can never redirect off-machine.
func loopbackURL(port int, key, value string) string {
	return fmt.Sprintf("http://127.0.0.1:%d/callback?%s=%s", port, key, url.QueryEscape(value))
}

func (h *Handler) logWarn(msg string, err error) {
	if h.log != nil {
		h.log.Warn(msg, slog.String("error", err.Error()))
	}
}

// respondError writes the uniform error envelope for an application error.
func respondError(c *fiber.Ctx, err error) error {
	if de, ok := apperr.As(err); ok {
		return c.Status(de.HTTPStatus()).JSON(fiber.Map{
			"error":      string(de.Code),
			"message":    de.Message,
			"request_id": c.Get("X-Request-ID"),
		})
	}
	return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
		"error":      "internal_error",
		"message":    "unexpected error",
		"request_id": c.Get("X-Request-ID"),
	})
}
