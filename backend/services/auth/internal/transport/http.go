// Package transport exposes the auth service over HTTP (Fiber). It maps
// requests to the application service and translates domain/application errors
// into the uniform JSON error envelope.
package transport

import (
	"context"
	"log/slog"
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

// Handler holds the dependencies for the auth HTTP endpoints.
type Handler struct {
	svc           *service.Service
	oauth         *oauth.Registry
	states        StateStore
	log           *slog.Logger
	secureCookies bool
}

// Config configures a Handler.
type Config struct {
	Service       *service.Service
	OAuth         *oauth.Registry
	States        StateStore
	Logger        *slog.Logger
	SecureCookies bool
}

// New builds a Handler.
func New(c Config) *Handler {
	return &Handler{svc: c.Service, oauth: c.OAuth, states: c.States, log: c.Logger, secureCookies: c.SecureCookies}
}

// Register mounts the auth routes on the given router group (mounted at
// /api/v1 by the shared server).
func (h *Handler) Register(r fiber.Router) {
	g := r.Group("/auth")
	g.Post("/register", h.register)
	g.Post("/login", h.login)
	g.Post("/refresh", h.refresh)
	g.Post("/logout", h.logout)
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

func (h *Handler) oauthStart(c *fiber.Ctx) error {
	provider := domain.Provider(c.Params("provider"))
	prov, ok := h.oauth.Get(provider)
	if !ok {
		return respondError(c, apperr.New(apperr.CodeNotFound, "oauth provider not configured"))
	}
	state, err := crypto.GenerateToken(24)
	if err != nil {
		return respondError(c, apperr.Wrap(apperr.CodeInternal, "state generation failed", err))
	}
	verifier, err := crypto.GenerateToken(32)
	if err != nil {
		return respondError(c, apperr.Wrap(apperr.CodeInternal, "verifier generation failed", err))
	}
	if err := h.states.Save(c.Context(), state, verifier, 10*time.Minute); err != nil {
		return respondError(c, apperr.Wrap(apperr.CodeInternal, "failed to persist oauth state", err))
	}
	return c.Redirect(prov.AuthCodeURL(state, verifier), fiber.StatusFound)
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
	verifier, err := h.states.Consume(c.Context(), state)
	if err != nil {
		return respondError(c, apperr.New(apperr.CodeUnauthorized, "invalid or expired oauth state"))
	}
	info, err := prov.Exchange(c.Context(), code, verifier)
	if err != nil {
		return respondError(c, apperr.Wrap(apperr.CodeUnauthorized, "oauth exchange failed", err))
	}
	if info.Email == "" {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "provider did not return a verified email"))
	}
	tokens, err := h.svc.UpsertOAuthUser(c.Context(), prov.Name(), info.ProviderUserID, info.Email, info.Name, h.meta(c))
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toTokenResponse(tokens))
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
