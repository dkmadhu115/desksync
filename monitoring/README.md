# Monitoring

Observability configuration for DeskSync. All services expose `/metrics` (RED
metrics + Go runtime) via `backend/pkg/observability` and emit structured JSON
logs to stdout via `backend/pkg/logger`.

## Layout

- `prometheus/prometheus.yml` — scrape config (static targets for compose).
- `prometheus/alerts.yml` — RED-metric alert rules (target down, high 5xx rate,
  high p99 latency). Loaded via `rule_files`.
- `grafana/provisioning/` — datasource (Prometheus + Loki) and dashboard
  providers, auto-loaded on Grafana start.
- `grafana/dashboards/desksync-overview.json` — services overview (request rate,
  error ratio, p99 latency, in-flight, logs).
- `loki/loki-config.yml` — single-binary Loki (filesystem storage).
- `promtail/promtail-config.yml` — ships Docker container logs to Loki, promoting
  `level`/`service` JSON fields to labels.

## Run the stack locally

```bash
make obs-up     # Prometheus :9090, Grafana :3000, Loki :3100
make obs-down
```

Grafana logs in with `admin`/`admin` (override via `GRAFANA_ADMIN_PASSWORD`) and
auto-loads the DeskSync datasources and dashboard.

## Kubernetes

Metric names are identical in-cluster. Enable the chart's
`serviceMonitor.enabled` and `prometheusRule.enabled` to scrape and alert via the
Prometheus Operator instead of the static config here.

Distributed tracing uses OpenTelemetry; the OTLP endpoint is configured via
`OTEL_EXPORTER_OTLP_ENDPOINT` (see `.env.example` / `config.otelExporterOtlpEndpoint`).

See [ADR 0002](../docs/adr/0002-monitoring-is-infrastructure.md) for why the
monitoring *service* is intentionally thin, and the
[deployment design](../docs/design/deployment.md) for the production topology.
