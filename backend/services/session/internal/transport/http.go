// Package transport exposes the session service over HTTP (Fiber), mapping
// requests to the application service and translating errors into the uniform
// JSON envelope. All routes require a valid access token.
package transport

import (
	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/pkg/middleware"
	"github.com/desksync/backend/services/session/internal/service"
	"github.com/gofiber/fiber/v2"
)

// Handler holds the session HTTP dependencies.
type Handler struct {
	svc *service.Service
	jwt *jwtauth.Manager
}

// New builds a Handler.
func New(svc *service.Service, jwt *jwtauth.Manager) *Handler {
	return &Handler{svc: svc, jwt: jwt}
}

// Register mounts the session routes (under /api/v1), all behind auth.
func (h *Handler) Register(r fiber.Router) {
	g := r.Group("/sessions", middleware.RequireAuth(h.jwt))
	g.Post("/", h.create)
	g.Get("/", h.list)
	g.Get("/:id", h.get)
	g.Post("/:id/end", h.end)
}

func (h *Handler) create(c *fiber.Ctx) error {
	var req createSessionRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	created, err := h.svc.CreateSession(c.Context(), middleware.UserID(c), req.PairingID)
	if err != nil {
		return respondError(c, err)
	}
	return c.Status(fiber.StatusCreated).JSON(toCreatedResponse(created))
}

func (h *Handler) list(c *fiber.Ctx) error {
	sessions, err := h.svc.ListSessions(c.Context(), middleware.UserID(c))
	if err != nil {
		return respondError(c, err)
	}
	out := make([]sessionResponse, 0, len(sessions))
	for _, s := range sessions {
		out = append(out, toSessionResponse(s))
	}
	return c.JSON(out)
}

func (h *Handler) get(c *fiber.Ctx) error {
	session, err := h.svc.GetSession(c.Context(), middleware.UserID(c), c.Params("id"))
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toSessionResponse(session))
}

func (h *Handler) end(c *fiber.Ctx) error {
	session, err := h.svc.EndSession(c.Context(), middleware.UserID(c), c.Params("id"), "")
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toSessionResponse(session))
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
