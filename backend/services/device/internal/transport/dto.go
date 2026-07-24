package transport

import (
	"time"

	"github.com/desksync/backend/services/device/internal/domain"
)

// registerRequest mirrors the `DeviceRegistration` schema in the OpenAPI
// contract.
type registerRequest struct {
	Kind      string  `json:"kind"`
	Platform  string  `json:"platform"`
	Name      string  `json:"name"`
	PublicKey string  `json:"public_key"`
	FCMToken  *string `json:"fcm_token"`
}

func (r registerRequest) toRegistration() domain.Registration {
	return domain.Registration{
		Kind:      domain.Kind(r.Kind),
		Platform:  domain.Platform(r.Platform),
		Name:      r.Name,
		PublicKey: r.PublicKey,
		FCMToken:  r.FCMToken,
	}
}

// heartbeatRequest optionally carries the presence to record. Absent status
// defaults to online.
type heartbeatRequest struct {
	Status string `json:"status"`
}

// deviceResponse mirrors the `Device` schema in the OpenAPI contract.
type deviceResponse struct {
	ID         string     `json:"id"`
	Kind       string     `json:"kind"`
	Platform   string     `json:"platform"`
	Name       string     `json:"name"`
	Status     string     `json:"status"`
	LastSeenAt *time.Time `json:"last_seen_at"`
	CreatedAt  time.Time  `json:"created_at"`
}

func toDeviceResponse(d domain.Device) deviceResponse {
	return deviceResponse{
		ID:         d.ID,
		Kind:       string(d.Kind),
		Platform:   string(d.Platform),
		Name:       d.Name,
		Status:     string(d.Status),
		LastSeenAt: d.LastSeenAt,
		CreatedAt:  d.CreatedAt,
	}
}
