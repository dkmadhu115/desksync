# DeskSync — Developer Remote Workstation

Secure, end-to-end encrypted remote desktop that lets a developer fully control
their own laptop from a mobile phone — anywhere in the world — as long as both
devices are online and previously paired. **This is not an AI product**; it is a
low-latency, hardware-accelerated remote desktop built on WebRTC with a
zero-trust security model.

> If any device loses internet, no actions execute. When connectivity returns,
> the session reconnects automatically.

## Repository layout

```
DeskSync/
├── backend/            Go microservices (Fiber), shared pkg/, migrations, OpenAPI
│   ├── services/       gateway, auth, device, session, pairing,
│   │                   signaling, relay, notification, monitoring
│   ├── pkg/            shared libraries (config, logger, jwtauth, crypto, ...)
│   ├── migrations/     golang-migrate SQL (PostgreSQL)
│   └── api/            OpenAPI 3.1 contracts
├── desktop-agent/      Rust (Tokio) Cargo workspace — capture, input, transport
├── mobile/             Flutter app (Riverpod, Go Router, Dio, WebRTC)
├── docker/             Dockerfiles + docker-compose for local dev
├── helm/               Helm charts (populated in Phase 10)
├── terraform/          Cloud infrastructure (populated in Phase 10)
├── monitoring/         Prometheus, Grafana, Loki configuration
├── scripts/            Developer & CI helper scripts
└── docs/               Architecture, API, security, threat model, ADRs
```

## Technology

| Layer          | Stack |
|----------------|-------|
| Backend        | Go 1.25, Fiber, PostgreSQL, Redis, Coturn, JWT |
| Desktop agent  | Rust, Tokio, WebRTC, Rustls, AES-GCM, X25519, SQLite, Tauri (config UI) |
| Mobile         | Flutter, Riverpod, Go Router, Dio, flutter_webrtc, Firebase Messaging |
| Streaming      | WebRTC (VP9/H264/H265), adaptive bitrate, hardware encoding |
| Observability  | Prometheus, Grafana, Loki, OpenTelemetry |
| Delivery       | Docker, Kubernetes, Helm, GitHub Actions |

## Phased delivery

The system is built phase-by-phase; each phase is production-ready and reviewed
before the next begins. See [`PROJECT_SPEC.md`](PROJECT_SPEC.md) for the full
specification and [`docs/`](docs/) for design artifacts.

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Planning, architecture, folder structure, DB/API/security design, threat model, CI skeleton | ✅ done |
| 2 | Backend foundation (auth, JWT, DB, Redis, logging, config) | ✅ done |
| 3 | Rust desktop agent (capture, keyboard/mouse injection, clipboard) | ✅ current |
| 4 | Flutter mobile (auth, pairing, device list, viewer, touch controls) | pending |
| 5 | WebRTC (signaling, peer connection, streaming, adaptive bitrate) | pending |
| 6 | Device pairing (QR, trust, persistent pairing) | pending |
| 7 | Remote desktop (rendering, keyboard, mouse, clipboard) | pending |
| 8 | Developer features (VS Code, Cursor, Git, Docker, kubectl, SSH) | pending |
| 9 | Security hardening (certificates, replay protection, encryption) | pending |
| 10 | Production deployment (Docker, K8s, Helm, CI/CD, monitoring) | pending |

## Getting started (local dev)

```bash
cp .env.example .env          # fill in secrets
make deps                     # download Go modules
make dev-infra                # start postgres, redis, coturn via docker compose
make run-gateway              # boot the API gateway (stub in Phase 1)
```

See the [`Makefile`](Makefile) for all available targets.

## Documentation

- [Architecture & diagrams](docs/design/architecture.md)
- [Database design](docs/design/database.md)
- [API design](docs/design/api.md)
- [Security design](docs/design/security.md)
- [Threat model](docs/design/threat-model.md)
- [Architecture Decision Records](docs/adr/)

## License

Licensed under the terms in [`LICENSE`](LICENSE).
