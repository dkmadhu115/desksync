# 10. Kubernetes deployment via a single umbrella Helm chart

- Status: Accepted
- Date: 2026-07-25

## Context

DeskSync ships as nine near-identical Go microservices. They differ only by
name, listen port, and the `*_HTTP_ADDR` env var; they share the same base
image (`docker/Dockerfile.service`, distroless/static:nonroot), config, secrets,
and operational endpoints (`/health`, `/ready`, `/metrics`). Phase 10 needs a
production deployment story — container images, Kubernetes manifests, CI/CD, and
monitoring — without hand-maintaining nine copies of every manifest.

Options considered:

1. Nine standalone charts / raw manifests per service — high duplication, drift.
2. One chart with a subchart per service — still nine copies of boilerplate.
3. **One umbrella chart that ranges over a `services` map** — a single template
   set generates the per-service Deployment/Service/HPA/PDB/NetworkPolicy.

## Decision

Ship a single umbrella chart `helm/desksync` whose templates iterate
`range $name, $svc := .Values.services`. Cross-cutting concerns are shared:

- **Config/secrets**: one ConfigMap (non-secret env) and one Secret (or an
  externally-managed `existingSecret`) injected into every pod via `envFrom`.
- **Images**: `<registry>/<repository>/<service>:<tag>`; `tag` defaults to the
  chart `appVersion` so an install pins a coherent set. Built once per service
  from the shared Dockerfile with a `SERVICE` build-arg.
- **Hardening (defaults)**: non-root, read-only root FS, dropped capabilities,
  `RuntimeDefault` seccomp, `automountServiceAccountToken: false`.
- **Resilience**: HPA (CPU/memory), PodDisruptionBudget, and topology spread per
  service. Liveness/readiness probes hit `/health` and `/ready`.
- **Schema migrations**: a pre-install/pre-upgrade Helm hook Job runs
  golang-migrate from an image that bakes in `backend/migrations`
  (`docker/Dockerfile.migrations`), so the DB is at the right version before the
  new pods roll out.
- **Networking**: a single Ingress routes the `public` services (gateway at `/`,
  signaling at `/api/v1/signaling`); internal services stay ClusterIP-only.
- **Observability (opt-in)**: `ServiceMonitor` + `PrometheusRule` for the
  Prometheus Operator, mirroring the static compose config in `monitoring/`.

CI/CD: `.github/workflows/cd.yml` builds and pushes all images (matrix,
including migrations) to GHCR, lints/renders/packages the chart, and runs a
guarded `helm upgrade --install` (only on version tags or explicit dispatch,
and only when a `KUBE_CONFIG` secret is present). CI additionally lints and
templates the chart on every PR.

## Consequences

- Adding a service is a few lines under `services:` — templates need no change.
- One chart version deploys a consistent image set; rollbacks are `helm rollback`.
- Migrations are ordered before rollout and are idempotent/rerunnable.
- The chart is offline-lintable/renderable (no subchart repos), keeping CI fast.
- Trade-off: the `range`-based templates are denser than per-service files; the
  `_helpers.tpl` and consistent labels keep them readable, and `helm template`
  in CI guards against regressions.
- Bundled Postgres/Redis are intentionally omitted; production uses managed
  datastores referenced through `secrets`/`config`.
