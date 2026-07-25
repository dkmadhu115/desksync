# Deployment & operations

How DeskSync is packaged, deployed, and observed in production (Phase 10). See
[ADR 0010](../adr/0010-kubernetes-helm-deployment.md) for the rationale.

## Container images

All nine services share one multi-stage Dockerfile,
[`docker/Dockerfile.service`](../../docker/Dockerfile.service): a `golang:1.25`
build stage compiles a static, trimmed binary (`CGO_ENABLED=0`, `-s -w`), and the
runtime stage is `gcr.io/distroless/static:nonroot` — no shell, no package
manager, runs as UID 65532. The target service is selected with the `SERVICE`
build-arg; the build context is `backend/` so the shared `pkg` module resolves.

The database migration image,
[`docker/Dockerfile.migrations`](../../docker/Dockerfile.migrations), layers the
`backend/migrations` SQL files onto `migrate/migrate` so the schema can be
advanced in-cluster with no volume mounts.

Images are published to GHCR as `ghcr.io/<owner>/desksync/<service>:<tag>`.

## Local stacks (docker-compose)

- `docker/docker-compose.yml` — infra (Postgres, Redis, coturn) + all nine
  services + an optional `migrate` tool profile.
- `docker/docker-compose.observability.yml` — Prometheus, Grafana, Loki,
  Promtail. Run overlaid on the app so they share the network:

```bash
make dev-infra                # infra only
make obs-up                   # Prometheus :9090 / Grafana :3000 / Loki :3100
```

## Kubernetes (Helm)

The umbrella chart `helm/desksync` renders one Deployment, Service, HPA, PDB, and
(optional) NetworkPolicy per entry in `.Values.services`, plus shared ConfigMap,
Secret, ServiceAccount, Ingress, and a migration hook Job. See
[helm/README](../../helm/README.md) for values.

Topology:

```
                 Internet
                     │  TLS
              ┌──────▼───────┐
              │   Ingress    │  host: api.desksync.example.com
              └──┬────────┬──┘
        /        │        │  /api/v1/signaling
        ▼        │        ▼
   ┌────────┐    │   ┌───────────┐
   │gateway │    │   │ signaling │
   └───┬────┘    │   └───────────┘
       │ ClusterIP (in-namespace)
   ┌───▼───────────────────────────────────────────┐
   │ auth · device · session · pairing · relay ·    │
   │ notification · monitoring                      │
   └───┬───────────────────────────────┬───────────┘
       ▼                                ▼
   Postgres (managed)             Redis (managed)
```

Lifecycle:

1. `helm upgrade --install` runs the **pre-upgrade migration Job** (hook weight
   `-5`) which applies `migrate ... up` against `DATABASE_URL`.
2. On success, Deployments roll out with `checksum/config` annotations so a
   ConfigMap change triggers a restart.
3. Readiness gates traffic on `/ready`; liveness restarts wedged pods on
   `/health`. HPAs scale on CPU/memory; PDBs preserve availability during
   node drains.

### Configuration & secrets

Non-secret settings live in one ConfigMap (`config.*`); secrets live in a Secret
that is either rendered from `secrets.data` (demo) or referenced via
`secrets.existingSecret` (production, populated by an external secret manager).
Every pod consumes both via `envFrom`, plus its own `*_HTTP_ADDR`.

### Hardening

Pod and container security contexts default to non-root, read-only root
filesystem, all capabilities dropped, `RuntimeDefault` seccomp, and no mounted
service-account token (services never call the K8s API). Optional NetworkPolicies
restrict pod ingress to in-namespace peers and the ingress controller.

## CI/CD

- **CI** (`.github/workflows/ci.yml`): builds/vets/tests Go, Rust, and Flutter;
  runs backend integration + migration up/down; lints OpenAPI; validates both
  compose files; **lints and templates the Helm chart**.
- **CD** (`.github/workflows/cd.yml`): on pushes to `main` and `v*` tags, a
  matrix builds and pushes all service images + the migrations image to GHCR
  (with build cache and immutable `sha-<commit>` tags), packages the chart, and
  runs a **guarded** `helm upgrade --install` — only on tags or explicit
  dispatch, and only when a `KUBE_CONFIG` secret is configured.

## Observability

Every service exports RED metrics (`http_requests_total`,
`http_request_duration_seconds`, `http_requests_in_flight`, all labelled with
`service`) and Go runtime metrics at `/metrics`, and logs structured JSON to
stdout.

- **Metrics**: Prometheus scrapes services (static targets locally; the chart's
  `ServiceMonitor` in-cluster). Alert rules cover target-down, >5% 5xx, and p99
  latency > 1s — as `monitoring/prometheus/alerts.yml` locally and the chart's
  `PrometheusRule` in-cluster.
- **Dashboards**: Grafana auto-provisions the Prometheus + Loki datasources and
  the DeskSync overview dashboard.
- **Logs**: Promtail ships container logs to Loki, promoting `level`/`service`
  to labels.
- **Tracing**: OpenTelemetry OTLP endpoint via `OTEL_EXPORTER_OTLP_ENDPOINT`.

See [monitoring/README](../../monitoring/README.md).
