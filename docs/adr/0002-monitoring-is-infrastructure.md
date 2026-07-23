# 2. Monitoring is infrastructure, not a request-serving service

- Status: Accepted
- Date: 2026-07-23

## Context

The specification lists a "Monitoring Service" alongside the other eight
microservices. In practice, observability is delivered by Prometheus (metrics),
Grafana (dashboards), Loki (logs), and OpenTelemetry (traces) — infrastructure
components, not a bespoke HTTP service that serves product requests.

Creating a heavyweight custom monitoring *service* would duplicate what the
observability stack already does and add little value.

## Decision

- Keep a small `monitoring` service in the backend workspace for an internal
  control-plane surface only: aggregate health, synthetic checks, and alert
  routing hooks.
- Deliver the actual metrics/logs/traces via Prometheus, Grafana, Loki, and the
  OTel collector, configured under [`monitoring/`](../../monitoring).
- Every service exposes `/metrics`, `/health`, and `/ready` via the shared
  `pkg/httpx` so scraping and probing are uniform.

## Consequences

- Less custom code to maintain; standard, battle-tested observability tooling.
- The `monitoring` service stays intentionally thin; heavy lifting is delegated.
- This is flagged to the user as a deviation from a literal reading of the spec,
  with rationale, per the spec's instruction to challenge weak decisions.
