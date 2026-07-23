// Package ws bridges an authenticated WebSocket connection to the signaling
// hub. Authentication happens before the upgrade: the client presents a
// short-lived signaling ticket (issued by the session service) plus the
// session id and role it claims. Only when the ticket verifies and matches is
// the connection upgraded and joined to the hub. Each connection has exactly
// one reader goroutine and one writer goroutine, so writes to the socket are
// serialized.
package ws

import (
	"log/slog"
	"time"

	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/signaling/internal/hub"
	"github.com/gofiber/contrib/websocket"
	"github.com/gofiber/fiber/v2"
)

const (
	// maxMessageBytes bounds a single signaling message (SDP can be a few KB).
	maxMessageBytes = 64 * 1024
	// readWait is how long we wait for the next client message before treating
	// the connection as dead. Clients send app-level heartbeats well within it.
	readWait = 75 * time.Second
	// writeWait bounds a single write.
	writeWait = 10 * time.Second

	localsTicket = "signal_ticket"
)

// Handler wires the signaling WebSocket endpoint.
type Handler struct {
	hub      *hub.Hub
	verifier *signalticket.Verifier
	log      *slog.Logger
}

// New builds a Handler.
func New(h *hub.Hub, v *signalticket.Verifier, log *slog.Logger) *Handler {
	if log == nil {
		log = slog.Default()
	}
	return &Handler{hub: h, verifier: v, log: log}
}

// Register mounts the signaling WebSocket route under the given router.
func (h *Handler) Register(r fiber.Router) {
	g := r.Group("/signaling")
	g.Use("/ws", h.authenticate)
	g.Get("/ws", websocket.New(h.serve))
}

// authenticate verifies the signaling ticket before the upgrade and stashes it
// for the connection handler. Non-upgrade requests are refused.
func (h *Handler) authenticate(c *fiber.Ctx) error {
	if !websocket.IsWebSocketUpgrade(c) {
		return fiber.ErrUpgradeRequired
	}
	ticket := c.Query("ticket")
	if ticket == "" {
		return unauthorized(c, "missing ticket")
	}
	tk, err := h.verifier.Verify(ticket)
	if err != nil {
		return unauthorized(c, "invalid or expired ticket")
	}
	// The ticket is authoritative; if the client also passed session/role they
	// must match to avoid confusion.
	if s := c.Query("session"); s != "" && s != tk.SessionID {
		return unauthorized(c, "session mismatch")
	}
	if r := c.Query("role"); r != "" && r != string(tk.Role) {
		return unauthorized(c, "role mismatch")
	}
	c.Locals(localsTicket, tk)
	return c.Next()
}

// serve runs the read/write pumps for one connection.
func (h *Handler) serve(c *websocket.Conn) {
	tk, ok := c.Locals(localsTicket).(*signalticket.Ticket)
	if !ok {
		_ = c.Close()
		return
	}

	peer, err := h.hub.Join(tk.SessionID, tk.Role)
	if err != nil {
		h.log.Warn("join rejected",
			slog.String("session_id", tk.SessionID),
			slog.String("role", string(tk.Role)),
			slog.String("error", err.Error()))
		_ = c.WriteControl(websocket.CloseMessage,
			websocket.FormatCloseMessage(websocket.ClosePolicyViolation, "role already connected"),
			time.Now().Add(writeWait))
		_ = c.Close()
		return
	}

	c.SetReadLimit(maxMessageBytes)

	// Writer pump: the only goroutine that writes to the socket.
	done := make(chan struct{})
	go func() {
		defer close(done)
		for msg := range peer.Outbound() {
			_ = c.SetWriteDeadline(time.Now().Add(writeWait))
			if err := c.WriteMessage(websocket.TextMessage, msg); err != nil {
				return
			}
		}
	}()

	// Reader pump.
	for {
		_ = c.SetReadDeadline(time.Now().Add(readWait))
		mt, data, err := c.ReadMessage()
		if err != nil {
			break
		}
		if mt != websocket.TextMessage {
			continue
		}
		switch h.hub.Dispatch(peer, data) {
		case hub.Reject, hub.Bye:
			goto cleanup
		}
	}

cleanup:
	h.hub.Leave(peer) // closes the peer's outbound channel, ending the writer
	_ = c.Close()     // unblock a writer stuck mid-write
	<-done
}

func unauthorized(c *fiber.Ctx, msg string) error {
	return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
		"error":   "unauthorized",
		"message": msg,
	})
}
