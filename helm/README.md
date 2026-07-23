# Helm charts

Kubernetes Helm charts for DeskSync are implemented in **Phase 10 (Production
Deployment)**. The planned layout:

```
helm/
├── desksync/              umbrella chart
│   ├── Chart.yaml
│   ├── values.yaml
│   └── templates/         Deployments, Services, HPAs, Ingress, ServiceMonitors
└── charts/                per-service subcharts (gateway, auth, ...)
```

Each service already exposes `/health`, `/ready`, and `/metrics`, so the charts
will wire liveness/readiness probes and Prometheus scraping directly.
