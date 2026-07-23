// Package hub implements the signaling relay: a set of two-peer rooms keyed by
// session id. It is transport-agnostic — it deals only in byte messages and a
// per-peer outbound channel — so the routing, presence, replay, and lifecycle
// logic is fully unit-tested without a real WebSocket. The thin WebSocket
// adapter (internal/ws) bridges a socket to a Peer.
package hub

import (
	"log/slog"
	"sync"

	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/signaling/internal/protocol"
)

// outboundBuffer bounds how many messages may queue for a slow peer before it
// is disconnected (a slow consumer must not stall the relay).
const outboundBuffer = 64

// Errors returned by Join.
var (
	// ErrRoleTaken means the room already has a peer with the requested role.
	ErrRoleTaken = &JoinError{"role already connected for this session"}
)

// JoinError is returned when a peer cannot join a room.
type JoinError struct{ msg string }

func (e *JoinError) Error() string { return e.msg }

// Peer is one side of a session (controller or agent).
type Peer struct {
	SessionID string
	Role      signalticket.Role

	out   chan []byte
	guard protocol.NonceGuard

	mu     sync.Mutex
	closed bool
}

// Outbound is the channel of messages to write to this peer's socket. It is
// closed when the peer is removed from the hub.
func (p *Peer) Outbound() <-chan []byte { return p.out }

// enqueue attempts a non-blocking send; it reports false if the buffer is full
// (the caller then disconnects the slow peer).
func (p *Peer) enqueue(msg []byte) bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed {
		return false
	}
	select {
	case p.out <- msg:
		return true
	default:
		return false
	}
}

func (p *Peer) close() {
	p.mu.Lock()
	defer p.mu.Unlock()
	if !p.closed {
		p.closed = true
		close(p.out)
	}
}

type room struct {
	peers map[signalticket.Role]*Peer
}

// Hub is the collection of active rooms.
type Hub struct {
	mu    sync.Mutex
	rooms map[string]*room
	log   *slog.Logger
}

// New builds a Hub.
func New(log *slog.Logger) *Hub {
	if log == nil {
		log = slog.Default()
	}
	return &Hub{rooms: make(map[string]*room), log: log}
}

// Join adds a peer for (sessionID, role). It returns an error if that role is
// already connected for the session. On success, presence is exchanged: the
// new peer learns about an already-present peer, and the present peer is told
// the new one joined.
func (h *Hub) Join(sessionID string, role signalticket.Role) (*Peer, error) {
	h.mu.Lock()
	defer h.mu.Unlock()

	r := h.rooms[sessionID]
	if r == nil {
		r = &room{peers: make(map[signalticket.Role]*Peer)}
		h.rooms[sessionID] = r
	}
	if _, exists := r.peers[role]; exists {
		return nil, ErrRoleTaken
	}

	p := &Peer{
		SessionID: sessionID,
		Role:      role,
		out:       make(chan []byte, outboundBuffer),
	}
	r.peers[role] = p

	// Presence exchange with any already-connected peer.
	for otherRole, other := range r.peers {
		if otherRole == role {
			continue
		}
		other.enqueue(protocol.Control(sessionID, protocol.KindPeerJoined, string(role)))
		p.enqueue(protocol.Control(sessionID, protocol.KindPeerJoined, string(otherRole)))
	}

	h.log.Info("peer joined", slog.String("session_id", sessionID), slog.String("role", string(role)))
	return p, nil
}

// Leave removes a peer and notifies the remaining peer, cleaning up empty rooms.
func (h *Hub) Leave(p *Peer) {
	h.mu.Lock()
	defer h.mu.Unlock()

	r := h.rooms[p.SessionID]
	if r == nil {
		return
	}
	if cur, ok := r.peers[p.Role]; !ok || cur != p {
		return
	}
	delete(r.peers, p.Role)
	p.close()

	for _, other := range r.peers {
		other.enqueue(protocol.Control(p.SessionID, protocol.KindPeerLeft, string(p.Role)))
	}
	if len(r.peers) == 0 {
		delete(h.rooms, p.SessionID)
	}
	h.log.Info("peer left", slog.String("session_id", p.SessionID), slog.String("role", string(p.Role)))
}

// DispatchResult tells the caller how to proceed after handling a message.
type DispatchResult int

const (
	// Continue: keep the connection open.
	Continue DispatchResult = iota
	// Reject: the message was invalid (protocol/replay/session mismatch); the
	// caller should close the connection.
	Reject
	// Bye: the peer asked to end; relay done, caller should close.
	Bye
)

// Dispatch validates and routes one inbound message from p. Offer/answer/ICE
// are relayed verbatim to the other peer; heartbeats are accepted and dropped;
// bye is relayed then signals closure. Invalid version, session mismatch, or a
// replayed nonce yields Reject.
func (h *Hub) Dispatch(p *Peer, raw []byte) DispatchResult {
	env, err := protocol.Parse(raw)
	if err != nil {
		return Reject
	}
	// A peer may only send for its own session.
	if env.SessionID != p.SessionID {
		return Reject
	}
	if !p.guard.Accept(env.Nonce) {
		return Reject
	}

	switch env.Kind() {
	case protocol.KindHeartbeat:
		return Continue
	case protocol.KindBye:
		h.relay(p, raw)
		return Bye
	case protocol.KindOffer, protocol.KindAnswer, protocol.KindICECandidate:
		h.relay(p, raw)
		return Continue
	default:
		// Unknown/again server-only kinds from a client are rejected.
		return Reject
	}
}

// relay forwards raw to the other peer in the same room, if present. A peer
// that cannot keep up (full buffer) is disconnected.
func (h *Hub) relay(from *Peer, raw []byte) {
	h.mu.Lock()
	r := h.rooms[from.SessionID]
	var target *Peer
	if r != nil {
		for role, peer := range r.peers {
			if role != from.Role {
				target = peer
				break
			}
		}
	}
	h.mu.Unlock()

	if target == nil {
		return
	}
	if !target.enqueue(raw) {
		h.log.Warn("dropping slow peer", slog.String("session_id", target.SessionID), slog.String("role", string(target.Role)))
		h.Leave(target)
	}
}

// PeerCount returns the number of connected peers for a session (test/metrics).
func (h *Hub) PeerCount(sessionID string) int {
	h.mu.Lock()
	defer h.mu.Unlock()
	if r := h.rooms[sessionID]; r != nil {
		return len(r.peers)
	}
	return 0
}
