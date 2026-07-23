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
- [Security design](design/security.md) — auth, E2E crypto, pinning, replay
  protection, revocation, rate limiting.
- [Threat model](design/threat-model.md) — STRIDE per trust boundary.

## Architecture Decision Records

- [ADR 0001 — Record architecture decisions](adr/0001-record-architecture-decisions.md)
- [ADR 0002 — Monitoring is infrastructure](adr/0002-monitoring-is-infrastructure.md)
- [ADR 0003 — Go workspace with per-service modules](adr/0003-go-workspace-per-service-modules.md)
- [ADR 0004 — WebRTC with E2E encryption and TURN fallback](adr/0004-webrtc-with-e2e-encryption.md)

## Phase status

Phase 1 (planning, architecture, scaffolding, CI skeleton) is complete. Later
phases implement the backend, desktop agent, mobile app, WebRTC, pairing,
remote desktop, developer features, security hardening, and production
deployment.
