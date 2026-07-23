package transport

import (
	"time"

	"github.com/desksync/backend/services/session/internal/domain"
	"github.com/desksync/backend/services/session/internal/ice"
	"github.com/desksync/backend/services/session/internal/service"
)

type createSessionRequest struct {
	PairingID string `json:"pairing_id"`
}

// sessionResponse mirrors the `Session` schema in the OpenAPI contract.
type sessionResponse struct {
	ID             string     `json:"id"`
	PairingID      string     `json:"pairing_id"`
	Status         string     `json:"status"`
	ConnectionType *string    `json:"connection_type"`
	StartedAt      time.Time  `json:"started_at"`
	EndedAt        *time.Time `json:"ended_at"`
}

func toSessionResponse(s domain.Session) sessionResponse {
	var ct *string
	if s.ConnectionType != nil {
		v := string(*s.ConnectionType)
		ct = &v
	}
	return sessionResponse{
		ID:             s.ID,
		PairingID:      s.PairingID,
		Status:         string(s.Status),
		ConnectionType: ct,
		StartedAt:      s.StartedAt,
		EndedAt:        s.EndedAt,
	}
}

// sessionCreatedResponse mirrors the `SessionCreated` schema.
type sessionCreatedResponse struct {
	Session         sessionResponse `json:"session"`
	SignalingURL    string          `json:"signaling_url"`
	SignalingTicket string          `json:"signaling_ticket"`
	ICEServers      []ice.Server    `json:"ice_servers"`
}

func toCreatedResponse(c *service.Created) sessionCreatedResponse {
	servers := c.ICEServers
	if servers == nil {
		servers = []ice.Server{}
	}
	return sessionCreatedResponse{
		Session:         toSessionResponse(c.Session),
		SignalingURL:    c.SignalingURL,
		SignalingTicket: c.SignalingTicket,
		ICEServers:      servers,
	}
}
