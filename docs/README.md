# DeskSync Documentation

Design artifacts for the DeskSync remote-desktop system. See the root
[`PROJECT_SPEC.md`](../PROJECT_SPEC.md) for the full specification.

## Design

- [Architecture & diagrams](design/architecture.md) — components, sequences
  (pairing, reconnect), streaming pipeline, and Kubernetes deployment.
- [Database design](design/database.md) — ER diagram and table reference;
  migrations live in [`backend/migrations/`](../backend/migrations).
- [API design](design/api.md) — REST surface and the WebSocket signaling
  protocol; contract in [`backend/api/openapi.yaml`](../backend/api/openapi.yaml).
- [Signaling & session plane](design/signaling.md) — session lifecycle,
  signaling tickets, ICE/TURN credentials, and the WebSocket relay hub.
- [Device pairing & trust](design/pairing.md) — device registration/presence,
  QR/manual-code challenges, the trust handshake, and persistent pairings.
- [Security design](design/security.md) — auth, E2E crypto, pinning, replay
  protection, revocation, rate limiting.
- [Threat model](design/threat-model.md) — STRIDE per trust boundary.
- [Desktop agent](design/desktop-agent.md) — crate layout, subsystem model, the
  `native` feature, capture pipeline, input mapping, and device identity.
- [Mobile app](design/mobile-app.md) — Flutter architecture, auth/session
  lifecycle, devices/pairing, and the touch-control input pipeline.
- [Developer features](design/devtools.md) — Quick Launch editors/terminals,
  saved workspaces, curated tool shortcuts, and the allowlisted control channel.
- [Deployment & operations](design/deployment.md) — container images, the Helm
  chart, CI/CD pipelines, and the metrics/logs/tracing stack.

## Architecture Decision Records

- [ADR 0001 — Record architecture decisions](adr/0001-record-architecture-decisions.md)
- [ADR 0002 — Monitoring is infrastructure](adr/0002-monitoring-is-infrastructure.md)
- [ADR 0003 — Go workspace with per-service modules](adr/0003-go-workspace-per-service-modules.md)
- [ADR 0004 — WebRTC with E2E encryption and TURN fallback](adr/0004-webrtc-with-e2e-encryption.md)
- [ADR 0005 — Desktop agent native backends behind a feature](adr/0005-desktop-agent-native-backends.md)
- [ADR 0006 — Stateless, ticket-authorized signaling](adr/0006-stateless-signaling-tickets.md)
- [ADR 0007 — Ephemeral, hashed pairing challenges in Redis](adr/0007-ephemeral-pairing-challenges.md)
- [ADR 0008 — Allowlisted developer actions (no remote command execution)](adr/0008-devtools-allowlist.md)
- [ADR 0009 — Application-layer end-to-end secure channel](adr/0009-e2e-secure-channel.md)
- [ADR 0010 — Kubernetes deployment via a single umbrella Helm chart](adr/0010-kubernetes-helm-deployment.md)

## Phase status

Phase 1 (planning, architecture, scaffolding, CI skeleton) is complete. Later
phases implement the backend, desktop agent, mobile app, WebRTC, pairing,
remote desktop, developer features, security hardening, and production
deployment.
