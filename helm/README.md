# Helm charts

The `desksync` umbrella chart is the **single, self-contained way to run
DeskSync**. It deploys all nine Go microservices (gateway, auth, device,
session, pairing, signaling, relay, notification, monitoring) **and** the
in-cluster infrastructure they depend on: PostgreSQL (StatefulSet + PVC), Redis,
and a coturn TURN/STUN relay. Any infra piece can be disabled to use a managed
equivalent.

```
helm/desksync/
├── Chart.yaml
├── values.yaml               defaults (production-shaped)
├── values-vps.yaml           single-node k3s override (LoadBalancer, local images)
└── templates/
    ├── _helpers.tpl          image-ref builder + derived DATABASE_URL/REDIS_ADDR
    ├── configmap.yaml         shared non-secret env
    ├── secret.yaml            shared secrets (derives DATABASE_URL/REDIS_ADDR)
    ├── serviceaccount.yaml
    ├── deployment.yaml        one Deployment per service (range over .Values.services)
    ├── service.yaml           one Service per service (ClusterIP / LoadBalancer)
    ├── infra-postgres.yaml    PostgreSQL StatefulSet + Service + Secret + PVC
    ├── infra-redis.yaml       Redis Deployment + Service
    ├── infra-coturn.yaml      coturn Deployment + Service (hostNetwork)
    ├── hpa.yaml               HorizontalPodAutoscaler per service
    ├── pdb.yaml               PodDisruptionBudget per service
    ├── ingress.yaml           routes public services (gateway, signaling)
    ├── migrations-job.yaml    revision-scoped DB migration Job (waits for postgres)
    ├── servicemonitor.yaml    Prometheus Operator scraping (opt-in)
    ├── prometheusrule.yaml    RED-metric alerts (opt-in)
    ├── networkpolicy.yaml     default-deny ingress (opt-in)
    └── NOTES.txt
```

## Quick start (managed cluster + registry)

```bash
helm lint helm/desksync
helm template desksync helm/desksync

# Install (demo values; override secrets for anything real)
helm upgrade --install desksync helm/desksync \
  --namespace desksync --create-namespace \
  --set global.image.tag=<image-tag>
```

## Single-node k3s (bare VPS, images built on the node)

`values-vps.yaml` runs everything on one k3s node: images imported into
containerd (no registry, `pullPolicy: Never`), gateway/signaling exposed on the
node IP via k3s ServiceLB, ingress/HPA/PDB off. Secrets are injected at install
time so they never live in a file:

```bash
# On the node: build + import images into k3s containerd
for s in gateway auth device session pairing signaling relay notification monitoring; do
  docker build -f docker/Dockerfile.service --build-arg SERVICE=$s -t desksync/$s:local backend
  docker save desksync/$s:local | k3s ctr images import -
done
docker build -f docker/Dockerfile.migrations -t desksync/migrations:local .
docker save desksync/migrations:local | k3s ctr images import -

# Install with generated secrets
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml
PG=$(openssl rand -hex 24); JA=$(openssl rand -hex 32); JR=$(openssl rand -hex 32)
SG=$(openssl rand -hex 32); TN=$(openssl rand -hex 32)
helm upgrade --install desksync helm/desksync -n desksync --create-namespace \
  -f helm/desksync/values-vps.yaml \
  --set-string postgres.auth.password=$PG \
  --set-string secrets.data.JWT_ACCESS_SECRET=$JA \
  --set-string secrets.data.JWT_REFRESH_SECRET=$JR \
  --set-string secrets.data.SIGNALING_TICKET_SECRET=$SG \
  --set-string secrets.data.TURN_STATIC_AUTH_SECRET=$TN
```

## Key values

| Key | Purpose |
|-----|---------|
| `global.image.{registry,repository,tag}` | Image coordinates; per-service image is `<registry>/<repository>/<service>:<tag>`. Empty `registry`/`repository` segments are omitted (supports images imported directly into containerd). Empty `tag` defaults to `Chart.appVersion`. |
| `global.image.pullPolicy` | Pull policy for the app images (`Never` for node-local images). |
| `config.*` | Non-secret env (log level, TTLs, ICE/TURN URLs) rendered into a ConfigMap. |
| `secrets.create` / `secrets.existingSecret` | Create a Secret from `secrets.data`, or reference one managed externally (recommended for production). `DATABASE_URL`/`REDIS_ADDR` are derived from the in-cluster infra when enabled. |
| `postgres.*` | In-cluster PostgreSQL (image, `auth.{username,password,database}`, `storage.{size,storageClassName}`). Set `enabled: false` for a managed DB. |
| `redis.*` | In-cluster Redis. Set `enabled: false` for managed Redis + `secrets.data.REDIS_ADDR`. |
| `coturn.*` | In-cluster TURN/STUN relay (`hostNetwork`, `externalIP`, `extraArgs`). |
| `services.<name>.serviceType` | `ClusterIP` (default), `NodePort`, or `LoadBalancer` to expose a service on the node. |
| `autoscaling.*` | HPA min/max replicas and CPU/memory targets. |
| `podDisruptionBudget.*` | PDB `minAvailable`. |
| `ingress.*` | Ingress class, host, and TLS for the public services. |
| `migrations.enabled` | Run the golang-migrate Job (image bundles `backend/migrations`); waits for in-cluster postgres before applying. |
| `serviceMonitor.enabled` / `prometheusRule.enabled` | Prometheus Operator integration (needs the CRDs). |
| `networkPolicy.enabled` | Restrict pod ingress. |

Each service exposes `/health`, `/ready`, and `/metrics`, wired to liveness /
readiness probes and Prometheus scrape annotations. The migrations image is
built from [`docker/Dockerfile.migrations`](../docker/Dockerfile.migrations) and
published by the [CD workflow](../.github/workflows/cd.yml).

See [deployment design](../docs/design/deployment.md) for the full topology.
