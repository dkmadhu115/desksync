package hub

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/signaling/internal/protocol"
)

func drain(t *testing.T, p *Peer) []byte {
	t.Helper()
	select {
	case msg := <-p.Outbound():
		return msg
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for a message")
		return nil
	}
}

func kindOf(t *testing.T, raw []byte) string {
	t.Helper()
	e, err := protocol.Parse(raw)
	if err != nil {
		t.Fatalf("parse relayed message: %v", err)
	}
	return e.Kind()
}

func offer(sessionID string, nonce uint64) []byte {
	env := map[string]any{
		"v":          1,
		"nonce":      nonce,
		"ts_ms":      1,
		"session_id": sessionID,
		"payload":    map[string]any{"kind": "offer", "sdp": "v=0"},
	}
	b, _ := json.Marshal(env)
	return b
}

func msg(sessionID string, nonce uint64, kind string) []byte {
	env := map[string]any{
		"v":          1,
		"nonce":      nonce,
		"ts_ms":      1,
		"session_id": sessionID,
		"payload":    map[string]any{"kind": kind},
	}
	b, _ := json.Marshal(env)
	return b
}

func TestJoinExchangesPresence(t *testing.T) {
	h := New(nil)
	ctrl, err := h.Join("s1", signalticket.RoleController)
	if err != nil {
		t.Fatalf("join controller: %v", err)
	}
	agent, err := h.Join("s1", signalticket.RoleAgent)
	if err != nil {
		t.Fatalf("join agent: %v", err)
	}

	// Controller is told the agent joined; agent is told the controller is present.
	if k := kindOf(t, drain(t, ctrl)); k != protocol.KindPeerJoined {
		t.Fatalf("controller expected peer_joined, got %q", k)
	}
	if k := kindOf(t, drain(t, agent)); k != protocol.KindPeerJoined {
		t.Fatalf("agent expected peer_joined, got %q", k)
	}
	if h.PeerCount("s1") != 2 {
		t.Fatalf("peer count = %d, want 2", h.PeerCount("s1"))
	}
}

func TestJoinRejectsDuplicateRole(t *testing.T) {
	h := New(nil)
	if _, err := h.Join("s1", signalticket.RoleController); err != nil {
		t.Fatalf("first join: %v", err)
	}
	if _, err := h.Join("s1", signalticket.RoleController); err != ErrRoleTaken {
		t.Fatalf("expected ErrRoleTaken, got %v", err)
	}
}

func TestDispatchRelaysToOtherPeer(t *testing.T) {
	h := New(nil)
	ctrl, _ := h.Join("s1", signalticket.RoleController)
	agent, _ := h.Join("s1", signalticket.RoleAgent)
	drain(t, ctrl) // consume presence
	drain(t, agent)

	if res := h.Dispatch(ctrl, offer("s1", 1)); res != Continue {
		t.Fatalf("dispatch result = %v, want Continue", res)
	}
	if k := kindOf(t, drain(t, agent)); k != protocol.KindOffer {
		t.Fatalf("agent expected relayed offer, got %q", k)
	}
}

func TestDispatchRejectsReplayedNonce(t *testing.T) {
	h := New(nil)
	ctrl, _ := h.Join("s1", signalticket.RoleController)
	h.Join("s1", signalticket.RoleAgent)
	drain(t, ctrl)

	if res := h.Dispatch(ctrl, offer("s1", 5)); res != Continue {
		t.Fatalf("first dispatch = %v", res)
	}
	if res := h.Dispatch(ctrl, offer("s1", 5)); res != Reject {
		t.Fatalf("replayed nonce should be Reject, got %v", res)
	}
	if res := h.Dispatch(ctrl, offer("s1", 3)); res != Reject {
		t.Fatalf("reordered nonce should be Reject, got %v", res)
	}
}

func TestDispatchRejectsForeignSession(t *testing.T) {
	h := New(nil)
	ctrl, _ := h.Join("s1", signalticket.RoleController)
	if res := h.Dispatch(ctrl, offer("other", 1)); res != Reject {
		t.Fatalf("foreign session should be Reject, got %v", res)
	}
}

func TestDispatchByeSignalsClosure(t *testing.T) {
	h := New(nil)
	ctrl, _ := h.Join("s1", signalticket.RoleController)
	agent, _ := h.Join("s1", signalticket.RoleAgent)
	drain(t, ctrl)
	drain(t, agent)

	if res := h.Dispatch(ctrl, msg("s1", 1, "bye")); res != Bye {
		t.Fatalf("bye should return Bye, got %v", res)
	}
	// Bye is relayed to the other peer.
	if k := kindOf(t, drain(t, agent)); k != protocol.KindBye {
		t.Fatalf("agent expected relayed bye, got %q", k)
	}
}

func TestLeaveNotifiesRemainingPeer(t *testing.T) {
	h := New(nil)
	ctrl, _ := h.Join("s1", signalticket.RoleController)
	agent, _ := h.Join("s1", signalticket.RoleAgent)
	drain(t, ctrl)
	drain(t, agent)

	h.Leave(ctrl)
	if k := kindOf(t, drain(t, agent)); k != protocol.KindPeerLeft {
		t.Fatalf("agent expected peer_left, got %q", k)
	}
	if h.PeerCount("s1") != 1 {
		t.Fatalf("peer count = %d, want 1", h.PeerCount("s1"))
	}
	// Room is cleaned up after the last peer leaves.
	h.Leave(agent)
	if h.PeerCount("s1") != 0 {
		t.Fatalf("peer count = %d, want 0", h.PeerCount("s1"))
	}
}

func TestHeartbeatAcceptedNotRelayed(t *testing.T) {
	h := New(nil)
	ctrl, _ := h.Join("s1", signalticket.RoleController)
	agent, _ := h.Join("s1", signalticket.RoleAgent)
	drain(t, ctrl)
	drain(t, agent)

	if res := h.Dispatch(ctrl, msg("s1", 1, "heartbeat")); res != Continue {
		t.Fatalf("heartbeat should Continue, got %v", res)
	}
	select {
	case unexpected := <-agent.Outbound():
		t.Fatalf("heartbeat must not be relayed, got %q", unexpected)
	case <-time.After(50 * time.Millisecond):
	}
}

func TestRelayWithNoPeerIsDropped(t *testing.T) {
	h := New(nil)
	ctrl, _ := h.Join("s1", signalticket.RoleController)
	// No agent present; dispatch should not block or error.
	if res := h.Dispatch(ctrl, offer("s1", 1)); res != Continue {
		t.Fatalf("dispatch = %v, want Continue", res)
	}
}
