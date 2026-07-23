// Package service implements the session application logic: authorizing a
// session against its pairing, persisting it, issuing a signaling ticket, and
// assembling the ICE configuration the client needs to connect.
package service

import (
	"context"
	"errors"
	"log/slog"
	"time"

	apperr "github.com/desksync/backend/pkg/errors"
	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/session/internal/domain"
	"github.com/desksync/backend/services/session/internal/ice"
)

// TicketIssuer mints signaling tickets.
type TicketIssuer interface {
	Issue(sessionID, userID string, role signalticket.Role) (string, error)
}

// ICEBuilder assembles the ICE server list for a session.
type ICEBuilder interface {
	Build(sessionID string) []ice.Server
}

// Config configures the Service.
type Config struct {
	Repo           domain.Repository
	Tickets        TicketIssuer
	ICE            ICEBuilder
	SignalingURL   string
	DefaultTimeout time.Duration
	Logger         *slog.Logger
}

// Service is the session application service.
type Service struct {
	repo         domain.Repository
	tickets      TicketIssuer
	ice          ICEBuilder
	signalingURL string
	timeout      time.Duration
	log          *slog.Logger
}

// New builds a Service.
func New(c Config) *Service {
	timeout := c.DefaultTimeout
	if timeout <= 0 {
		timeout = 15 * time.Minute
	}
	log := c.Logger
	if log == nil {
		log = slog.Default()
	}
	return &Service{
		repo:         c.Repo,
		tickets:      c.Tickets,
		ice:          c.ICE,
		signalingURL: c.SignalingURL,
		timeout:      timeout,
		log:          log,
	}
}

// Created is the result of creating a session: the persisted session plus the
// signaling connection info the controller needs.
type Created struct {
	Session         domain.Session
	SignalingURL    string
	SignalingTicket string
	ICEServers      []ice.Server
}

// CreateSession authorizes and creates a session for the user's pairing.
func (s *Service) CreateSession(ctx context.Context, userID, pairingID string) (*Created, error) {
	if pairingID == "" {
		return nil, apperr.New(apperr.CodeInvalidInput, "pairing_id is required")
	}

	pairing, err := s.repo.PairingForUser(ctx, pairingID, userID)
	if err != nil {
		if errors.Is(err, domain.ErrPairingNotFound) {
			return nil, apperr.New(apperr.CodeNotFound, "pairing not found")
		}
		return nil, apperr.Wrap(apperr.CodeInternal, "failed to load pairing", err)
	}
	if pairing.Status != "active" {
		return nil, apperr.New(apperr.CodePreconditionF, "pairing is not active")
	}

	session, err := s.repo.CreateSession(ctx, domain.Session{
		PairingID:      pairingID,
		UserID:         userID,
		Status:         domain.StatusConnecting,
		TimeoutSeconds: int(s.timeout.Seconds()),
	})
	if err != nil {
		return nil, apperr.Wrap(apperr.CodeInternal, "failed to create session", err)
	}

	ticket, err := s.tickets.Issue(session.ID, userID, signalticket.RoleController)
	if err != nil {
		return nil, apperr.Wrap(apperr.CodeInternal, "failed to issue signaling ticket", err)
	}

	iceServers := s.ice.Build(session.ID)

	if err := s.repo.AppendEvent(ctx, session.ID, "created", map[string]any{
		"pairing_id": pairingID,
	}); err != nil {
		// Non-fatal: the session is usable even if the audit event failed.
		s.log.Warn("failed to append session created event",
			slog.String("session_id", session.ID), slog.String("error", err.Error()))
	}

	return &Created{
		Session:         session,
		SignalingURL:    s.signalingURL,
		SignalingTicket: ticket,
		ICEServers:      iceServers,
	}, nil
}

// GetSession returns a user-owned session.
func (s *Service) GetSession(ctx context.Context, userID, id string) (domain.Session, error) {
	session, err := s.repo.GetSession(ctx, id, userID)
	if err != nil {
		if errors.Is(err, domain.ErrSessionNotFound) {
			return domain.Session{}, apperr.New(apperr.CodeNotFound, "session not found")
		}
		return domain.Session{}, apperr.Wrap(apperr.CodeInternal, "failed to load session", err)
	}
	return session, nil
}

// ListSessions returns the user's recent sessions.
func (s *Service) ListSessions(ctx context.Context, userID string) ([]domain.Session, error) {
	sessions, err := s.repo.ListSessions(ctx, userID, 50)
	if err != nil {
		return nil, apperr.Wrap(apperr.CodeInternal, "failed to list sessions", err)
	}
	return sessions, nil
}

// EndSession terminates a session (idempotent).
func (s *Service) EndSession(ctx context.Context, userID, id, reason string) (domain.Session, error) {
	if reason == "" {
		reason = "client_ended"
	}
	session, err := s.repo.EndSession(ctx, id, userID, reason)
	if err != nil {
		if errors.Is(err, domain.ErrSessionNotFound) {
			return domain.Session{}, apperr.New(apperr.CodeNotFound, "session not found")
		}
		return domain.Session{}, apperr.Wrap(apperr.CodeInternal, "failed to end session", err)
	}
	if err := s.repo.AppendEvent(ctx, id, "ended", map[string]any{"reason": reason}); err != nil {
		s.log.Warn("failed to append session ended event",
			slog.String("session_id", id), slog.String("error", err.Error()))
	}
	return session, nil
}
