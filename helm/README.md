# Helm charts

The `desksync` umbrella chart deploys all nine Go microservices (gateway, auth,
device, session, pairing, signaling, relay, notification, monitoring) to
Kubernetes.

```
helm/desksync/
├── Chart.yaml
├── values.yaml
└── templates/
    ├── _helpers.tpl
    ├── configmap.yaml         shared non-secret env
    ├── secret.yaml            shared secrets (or reference an existing Secret)
    ├── serviceaccount.yaml
    ├── deployment.yaml        one Deployment per service (range over .Values.services)
    ├── service.yaml           one ClusterIP Service per service
    ├── hpa.yaml               HorizontalPodAutoscaler per service
    ├── pdb.yaml               PodDisruptionBudget per service
    ├── ingress.yaml           routes public services (gateway, signaling)
    ├── migrations-job.yaml    pre-install/pre-upgrade DB migration hook
    ├── servicemonitor.yaml    Prometheus Operator scraping (opt-in)
    ├── prometheusrule.yaml    RED-metric alerts (opt-in)
    ├── networkpolicy.yaml     default-deny ingress (opt-in)
    └── NOTES.txt
```

## Quick start

```bash
# Lint + render locally
helm lint helm/desksync
helm template desksync helm/desksync

# Install (demo values; override secrets for anything real)
helm upgrade --install desksync helm/desksync \
  --namespace desksync --create-namespace \
  --set global.image.tag=<image-tag>
```

## Key values

| Key | Purpose |
|-----|---------|
| `global.image.{registry,repository,tag}` | Image coordinates; per-service image is `<registry>/<repository>/<service>:<tag>`. Empty `tag` defaults to `Chart.appVersion`. |
| `config.*` | Non-secret env (log level, TTLs, ICE/TURN URLs) rendered into a ConfigMap. |
| `secrets.create` / `secrets.existingSecret` | Create a Secret from `secrets.data`, or reference one managed externally (recommended for production). |
| `autoscaling.*` | HPA min/max replicas and CPU/memory targets. |
| `podDisruptionBudget.*` | PDB `minAvailable`. |
| `ingress.*` | Ingress class, host, and TLS for the public services. |
| `migrations.enabled` | Run the golang-migrate Job (image bundles `backend/migrations`) before rollout. |
| `serviceMonitor.enabled` / `prometheusRule.enabled` | Prometheus Operator integration (needs the CRDs). |
| `networkPolicy.enabled` | Restrict pod ingress. |

Each service exposes `/health`, `/ready`, and `/metrics`, wired to liveness /
readiness probes and Prometheus scrape annotations. The migrations image is
built from [`docker/Dockerfile.migrations`](../docker/Dockerfile.migrations) and
published by the [CD workflow](../.github/workflows/cd.yml).

See [deployment design](../docs/design/deployment.md) for the full topology.
