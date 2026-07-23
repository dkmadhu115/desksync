// Package protocol defines the signaling message envelope exchanged over the
// WebSocket, mirroring the Rust agent's `SignalEnvelope`
// (desktop-agent/crates/transport) and the Flutter client. The signaling
// service only inspects the envelope header (version, nonce, session id, and
// payload kind) for validation and routing; it relays the opaque payload
// between peers without interpreting SDP/ICE contents, and never sees decrypted
// media.
package protocol

import (
	"encoding/json"
	"errors"
)

// Version is the current protocol version.
const Version = 1

// Kind values for the payload discriminator (the `kind` field inside payload).
const (
	KindOffer        = "offer"
	KindAnswer       = "answer"
	KindICECandidate = "ice_candidate"
	KindHeartbeat    = "heartbeat"
	KindBye          = "bye"
	// Server-originated control kinds informing a peer of the other side's
	// presence. Clients that do not recognize them ignore them.
	KindPeerJoined = "peer_joined"
	KindPeerLeft   = "peer_left"
)

// Envelope is the wire envelope. Payload is kept raw so the server can relay it
// verbatim without depending on the media-negotiation schema.
type Envelope struct {
	V         uint8           `json:"v"`
	Nonce     uint64          `json:"nonce"`
	TsMs      uint64          `json:"ts_ms"`
	SessionID string          `json:"session_id"`
	Payload   json.RawMessage `json:"payload"`
}

// ErrInvalid indicates a structurally invalid or unsupported envelope.
var ErrInvalid = errors.New("protocol: invalid envelope")

// Parse decodes and structurally validates an incoming envelope.
func Parse(raw []byte) (*Envelope, error) {
	var e Envelope
	if err := json.Unmarshal(raw, &e); err != nil {
		return nil, ErrInvalid
	}
	if e.V != Version || e.SessionID == "" || len(e.Payload) == 0 {
		return nil, ErrInvalid
	}
	return &e, nil
}

// Kind extracts the payload discriminator, or "" when absent.
func (e *Envelope) Kind() string {
	var p struct {
		Kind string `json:"kind"`
	}
	if err := json.Unmarshal(e.Payload, &p); err != nil {
		return ""
	}
	return p.Kind
}

// Control builds a server-originated control envelope (peer_joined/peer_left)
// carrying the affected role.
func Control(sessionID, kind, role string) []byte {
	payload, _ := json.Marshal(map[string]string{"kind": kind, "role": role})
	env := Envelope{
		V:         Version,
		SessionID: sessionID,
		Payload:   payload,
	}
	b, _ := json.Marshal(env)
	return b
}

// NonceGuard enforces strictly increasing nonces on a per-connection stream,
// rejecting replays and reordered messages.
type NonceGuard struct {
	last    uint64
	hasLast bool
}

// Accept reports whether the nonce is acceptable (strictly greater than the
// last accepted), recording it when so.
func (g *NonceGuard) Accept(nonce uint64) bool {
	if g.hasLast && nonce <= g.last {
		return false
	}
	g.last = nonce
	g.hasLast = true
	return true
}
