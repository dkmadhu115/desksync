package transport

import (
	"time"

	"github.com/desksync/backend/services/pairing/internal/domain"
	"github.com/desksync/backend/services/pairing/internal/service"
)

type initiateRequest struct {
	DesktopDeviceID string `json:"desktop_device_id"`
}

type confirmRequest struct {
	PairingID      string `json:"pairing_id"`
	Code           string `json:"code"`
	MobileDeviceID string `json:"mobile_device_id"`
}

// challengeResponse mirrors the `PairingChallenge` schema.
type challengeResponse struct {
	PairingID  string    `json:"pairing_id"`
	QRPayload  string    `json:"qr_payload"`
	ManualCode string    `json:"manual_code"`
	ExpiresAt  time.Time `json:"expires_at"`
}

func toChallengeResponse(c service.Challenge) challengeResponse {
	return challengeResponse{
		PairingID:  c.PairingID,
		QRPayload:  c.QRPayload,
		ManualCode: c.ManualCode,
		ExpiresAt:  c.ExpiresAt,
	}
}

// pairingResponse mirrors the `Pairing` schema.
type pairingResponse struct {
	ID              string     `json:"id"`
	MobileDeviceID  string     `json:"mobile_device_id"`
	DesktopDeviceID string     `json:"desktop_device_id"`
	Status          string     `json:"status"`
	Trusted         bool       `json:"trusted"`
	CreatedAt       time.Time  `json:"created_at"`
	ConfirmedAt     *time.Time `json:"confirmed_at"`
}

func toPairingResponse(p domain.Pairing) pairingResponse {
	return pairingResponse{
		ID:              p.ID,
		MobileDeviceID:  p.MobileDeviceID,
		DesktopDeviceID: p.DesktopDeviceID,
		Status:          string(p.Status),
		Trusted:         p.Trusted,
		CreatedAt:       p.CreatedAt,
		ConfirmedAt:     p.ConfirmedAt,
	}
}
