// Package transport exposes the pairing service over HTTP (Fiber), mapping
// requests to the application service and translating errors into the uniform
// JSON envelope. All routes require a valid access token.
package transport

import (
	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/pkg/middleware"
	"github.com/desksync/backend/services/pairing/internal/service"
	"github.com/gofiber/fiber/v2"
)

// Handler holds the pairing HTTP dependencies.
type Handler struct {
	svc *service.Service
	jwt *jwtauth.Manager
}

// New builds a Handler.
func New(svc *service.Service, jwt *jwtauth.Manager) *Handler {
	return &Handler{svc: svc, jwt: jwt}
}

// Register mounts the pairing routes (under /api/v1), all behind auth.
func (h *Handler) Register(r fiber.Router) {
	auth := middleware.RequireAuth(h.jwt)

	p := r.Group("/pairing", auth)
	p.Post("/initiate", h.initiate)
	p.Post("/confirm", h.confirm)

	// Persistent pairing management.
	g := r.Group("/pairings", auth)
	g.Get("/", h.list)
	g.Delete("/:id", h.revoke)
}

func (h *Handler) initiate(c *fiber.Ctx) error {
	var req initiateRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	ch, err := h.svc.Initiate(c.Context(), middleware.UserID(c), req.DesktopDeviceID)
	if err != nil {
		return respondError(c, err)
	}
	return c.Status(fiber.StatusCreated).JSON(toChallengeResponse(ch))
}

func (h *Handler) confirm(c *fiber.Ctx) error {
	var req confirmRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	pairing, err := h.svc.Confirm(c.Context(), middleware.UserID(c), req.PairingID, req.Code, req.MobileDeviceID)
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toPairingResponse(pairing))
}

func (h *Handler) list(c *fiber.Ctx) error {
	pairings, err := h.svc.List(c.Context(), middleware.UserID(c))
	if err != nil {
		return respondError(c, err)
	}
	out := make([]pairingResponse, 0, len(pairings))
	for _, p := range pairings {
		out = append(out, toPairingResponse(p))
	}
	return c.JSON(out)
}

func (h *Handler) revoke(c *fiber.Ctx) error {
	if err := h.svc.Revoke(c.Context(), middleware.UserID(c), c.Params("id")); err != nil {
		return respondError(c, err)
	}
	return c.SendStatus(fiber.StatusNoContent)
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
