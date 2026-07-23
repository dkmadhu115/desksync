# Monitoring

Observability configuration for DeskSync.

- `prometheus/` — Prometheus scrape config. All services expose `/metrics` (RED
  metrics + Go runtime) via `backend/pkg/observability`.
- `grafana/` — dashboards and datasource provisioning (added in Phase 10).
- `loki/` — log aggregation config; services emit structured JSON logs to stdout
  which are collected into Loki (added in Phase 10).

Distributed tracing uses OpenTelemetry; the OTLP endpoint is configured via
`OTEL_EXPORTER_OTLP_ENDPOINT` (see `.env.example`).

See [ADR 0002](../docs/adr/0002-monitoring-is-infrastructure.md) for why the
monitoring *service* is intentionally thin and the heavy lifting is delegated to
this stack.
