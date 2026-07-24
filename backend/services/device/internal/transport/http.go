// Package transport exposes the device service over HTTP (Fiber), mapping
// requests to the application service and translating errors into the uniform
// JSON envelope. All routes require a valid access token.
package transport

import (
	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/pkg/jwtauth"
	"github.com/desksync/backend/pkg/middleware"
	"github.com/desksync/backend/services/device/internal/domain"
	"github.com/desksync/backend/services/device/internal/service"
	"github.com/gofiber/fiber/v2"
)

// Handler holds the device HTTP dependencies.
type Handler struct {
	svc *service.Service
	jwt *jwtauth.Manager
}

// New builds a Handler.
func New(svc *service.Service, jwt *jwtauth.Manager) *Handler {
	return &Handler{svc: svc, jwt: jwt}
}

// Register mounts the device routes (under /api/v1), all behind auth.
func (h *Handler) Register(r fiber.Router) {
	g := r.Group("/devices", middleware.RequireAuth(h.jwt))
	g.Post("/", h.register)
	g.Get("/", h.list)
	g.Get("/:id", h.get)
	g.Delete("/:id", h.revoke)
	g.Post("/:id/heartbeat", h.heartbeat)
}

func (h *Handler) register(c *fiber.Ctx) error {
	var req registerRequest
	if err := c.BodyParser(&req); err != nil {
		return respondError(c, apperr.New(apperr.CodeInvalidInput, "invalid request body"))
	}
	device, err := h.svc.Register(c.Context(), middleware.UserID(c), req.toRegistration())
	if err != nil {
		return respondError(c, err)
	}
	return c.Status(fiber.StatusCreated).JSON(toDeviceResponse(device))
}

func (h *Handler) list(c *fiber.Ctx) error {
	devices, err := h.svc.List(c.Context(), middleware.UserID(c))
	if err != nil {
		return respondError(c, err)
	}
	out := make([]deviceResponse, 0, len(devices))
	for _, d := range devices {
		out = append(out, toDeviceResponse(d))
	}
	return c.JSON(out)
}

func (h *Handler) get(c *fiber.Ctx) error {
	device, err := h.svc.Get(c.Context(), middleware.UserID(c), c.Params("id"))
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toDeviceResponse(device))
}

func (h *Handler) revoke(c *fiber.Ctx) error {
	if err := h.svc.Revoke(c.Context(), middleware.UserID(c), c.Params("id")); err != nil {
		return respondError(c, err)
	}
	return c.SendStatus(fiber.StatusNoContent)
}

func (h *Handler) heartbeat(c *fiber.Ctx) error {
	var req heartbeatRequest
	// A body is optional; ignore parse errors and default to online.
	_ = c.BodyParser(&req)
	device, err := h.svc.Heartbeat(c.Context(), middleware.UserID(c), c.Params("id"), domain.Status(req.Status))
	if err != nil {
		return respondError(c, err)
	}
	return c.JSON(toDeviceResponse(device))
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
