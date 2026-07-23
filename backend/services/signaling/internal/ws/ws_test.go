package ws_test

import (
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/signaling/internal/hub"
	"github.com/desksync/backend/services/signaling/internal/protocol"
	"github.com/desksync/backend/services/signaling/internal/ws"
	fws "github.com/fasthttp/websocket"
	"github.com/gofiber/contrib/websocket"
	"github.com/gofiber/fiber/v2"
)

const secret = "ws-integration-secret-0123456789abcd"

// startServer spins up a real Fiber listener on a random local port and returns
// its base ws URL plus a shutdown func.
func startServer(t *testing.T) (baseURL string, stop func()) {
	t.Helper()

	verifier, err := signalticket.NewVerifier(secret)
	if err != nil {
		t.Fatalf("verifier: %v", err)
	}
	h := ws.New(hub.New(nil), verifier, nil)

	app := fiber.New(fiber.Config{DisableStartupMessage: true})
	// The upgrade middleware needs the websocket package linked; register it.
	_ = websocket.IsWebSocketUpgrade
	h.Register(app.Group("/api/v1"))

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	go func() { _ = app.Listener(ln) }()

	port := ln.Addr().(*net.TCPAddr).Port
	// Give the server a moment to start serving.
	time.Sleep(50 * time.Millisecond)
	return fmt.Sprintf("ws://127.0.0.1:%d/api/v1/signaling/ws", port), func() { _ = app.Shutdown() }
}

func dial(t *testing.T, baseURL, ticket, role string) (*fws.Conn, *http.Response, error) {
	t.Helper()
	url := fmt.Sprintf("%s?ticket=%s&role=%s", baseURL, ticket, role)
	return fws.DefaultDialer.Dial(url, nil)
}

func readEnvelope(t *testing.T, c *fws.Conn) *protocol.Envelope {
	t.Helper()
	_ = c.SetReadDeadline(time.Now().Add(2 * time.Second))
	_, data, err := c.ReadMessage()
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	e, err := protocol.Parse(data)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	return e
}

func TestRejectsMissingOrInvalidTicket(t *testing.T) {
	baseURL, stop := startServer(t)
	defer stop()

	if _, resp, err := dial(t, baseURL, "", "controller"); err == nil {
		t.Fatal("expected handshake failure without a ticket")
	} else if resp != nil && resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", resp.StatusCode)
	}

	if _, _, err := dial(t, baseURL, "garbage", "controller"); err == nil {
		t.Fatal("expected handshake failure with a bad ticket")
	}
}

func TestEndToEndPresenceAndRelay(t *testing.T) {
	baseURL, stop := startServer(t)
	defer stop()

	issuer, _ := signalticket.NewIssuer(secret, time.Minute)
	ctrlTicket, _ := issuer.Issue("sess-1", "user-1", signalticket.RoleController)
	agentTicket, _ := issuer.Issue("sess-1", "user-1", signalticket.RoleAgent)

	ctrl, _, err := dial(t, baseURL, ctrlTicket, "controller")
	if err != nil {
		t.Fatalf("dial controller: %v", err)
	}
	defer ctrl.Close()

	agent, _, err := dial(t, baseURL, agentTicket, "agent")
	if err != nil {
		t.Fatalf("dial agent: %v", err)
	}
	defer agent.Close()

	// Agent immediately learns the controller is present.
	if e := readEnvelope(t, agent); e.Kind() != protocol.KindPeerJoined {
		t.Fatalf("agent expected peer_joined, got %q", e.Kind())
	}
	// Controller is notified when the agent joins.
	if e := readEnvelope(t, ctrl); e.Kind() != protocol.KindPeerJoined {
		t.Fatalf("controller expected peer_joined, got %q", e.Kind())
	}

	// Controller sends an offer; the agent receives it verbatim.
	offer := mustEnvelope("sess-1", 1, map[string]any{"kind": "offer", "sdp": "v=0"})
	_ = ctrl.SetWriteDeadline(time.Now().Add(2 * time.Second))
	if err := ctrl.WriteMessage(fws.TextMessage, offer); err != nil {
		t.Fatalf("write offer: %v", err)
	}
	if e := readEnvelope(t, agent); e.Kind() != protocol.KindOffer {
		t.Fatalf("agent expected offer, got %q", e.Kind())
	}
}

func TestRejectsDuplicateRole(t *testing.T) {
	baseURL, stop := startServer(t)
	defer stop()

	issuer, _ := signalticket.NewIssuer(secret, time.Minute)
	ticket, _ := issuer.Issue("sess-dup", "user-1", signalticket.RoleController)

	first, _, err := dial(t, baseURL, ticket, "controller")
	if err != nil {
		t.Fatalf("first dial: %v", err)
	}
	defer first.Close()

	// Second controller for the same session is closed by the server.
	second, _, err := dial(t, baseURL, ticket, "controller")
	if err != nil {
		// Some stacks surface this as a handshake error; acceptable.
		return
	}
	defer second.Close()
	_ = second.SetReadDeadline(time.Now().Add(2 * time.Second))
	if _, _, err := second.ReadMessage(); err == nil {
		t.Fatal("expected the duplicate connection to be closed")
	}
}

func mustEnvelope(sessionID string, nonce uint64, payload map[string]any) []byte {
	env := map[string]any{
		"v":          1,
		"nonce":      nonce,
		"ts_ms":      1,
		"session_id": sessionID,
		"payload":    payload,
	}
	b, _ := json.Marshal(env)
	return b
}
