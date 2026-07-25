# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

DeskSync is a secure, end-to-end encrypted **remote desktop** — control your own laptop from a phone. It is **not** an AI product. Three codebases in one repo:

- `backend/` — Go 1.25 microservices (Fiber) behind an API gateway
- `desktop-agent/` — Rust (Tokio) Cargo workspace; the headless daemon that runs on the laptop
- `mobile/` — Flutter app (the phone client)

The system was built phase-by-phase (10 phases, all complete). `PROJECT_SPEC.md` is the original spec; `README.md` has the phase table; design docs live in `docs/design/` and ADRs in `docs/adr/`.

## Commands

The root `Makefile` is the entry point for all three stacks — prefer it over raw `go`/`cargo`/`flutter` invocations. `make help` lists every target.

**Backend (Go):**
- `make deps` — `go work sync` + download modules (run first)
- `make build` / `make vet` / `make test` — across every module; `make test` runs with `-race` and coverage
- `make lint` — golangci-lint per module (skips if not installed)
- `make run-<service>` — e.g. `make run-gateway`, `make run-auth`
- `make ci` — build + vet + test (the local CI gate)

**Running a single Go test:** each service is its own Go module, so `cd` into it first. Because of the workspace, set `GOWORK=off` when running a module's tests in isolation (this is how CI runs integration tests):
```bash
cd backend/services/auth && GOWORK=off go test ./internal/service/... -run TestName -v
```

**Integration tests** are gated behind `DESKSYNC_INTEGRATION=1` and need Postgres + Redis (see `DATABASE_URL`/`REDIS_ADDR`). They match `-run Integration`.

**Desktop agent (Rust):**
- `make agent-build` / `make agent-test` — default build has **no-op platform backends** so it compiles and tests on headless CI (no display, no system libs)
- `make agent-build-native` / `make agent-run-native` — real screen-capture/input/clipboard backends via the `native` cargo feature; needs a real display + OS permissions
- `make agent-fmt` / `make agent-clippy` (clippy runs with `-D warnings`)

**Mobile (Flutter):** `make mobile-deps`, `make mobile-analyze`, `make mobile-test`

**Infra & deploy:** `make dev-infra` (postgres/redis/coturn via compose), `make obs-up` (Prometheus/Grafana/Loki), `make images`, `make helm-lint`/`make helm-template`, `make k8s-deploy IMAGE_TAG=...`, `make migrate-up` (needs `golang-migrate` + `DATABASE_URL`).

Local dev bootstrap: `cp .env.example .env` → fill secrets → `make deps` → `make dev-infra` → `make run-gateway`.

## Backend architecture

**Go workspace of independent modules.** `backend/go.work` unions `pkg/` and one module per service under `services/`. The nine services: `gateway auth device session pairing signaling relay notification monitoring`. Shared code lives in `pkg/` (imported as `github.com/desksync/backend/pkg/...`).

**Every service main.go is thin and identical in shape.** `pkg/service.Run(Spec, RegisterFunc)` is the canonical bootstrap: it loads config, builds the logger + metrics, constructs the Fiber app with standard ops endpoints (`/health`, `/ready`, `/metrics`) and middleware, registers domain routes, and handles graceful shutdown on SIGINT/SIGTERM. When adding a service or endpoint, follow the existing wiring — do not reinvent bootstrap.

**Clean Architecture per service** (see `services/auth` as the reference):
```
cmd/main.go              → wiring only: repo → service → transport, then service.Run
internal/domain/         → models, repository interfaces, domain errors
internal/repository/     → Postgres/Redis implementations of domain interfaces
internal/service/        → application logic (depends on domain interfaces)
internal/transport/      → Fiber HTTP handlers + DTOs
```
Dependencies point inward (transport → service → domain ← repository). Add new persistence behind a domain interface, not a concrete type.

**Config is env-driven and centralized** in `pkg/config` (`LoadBase`, `LoadPostgres`, `LoadRedis`, `LoadJWT`, `LoadOAuth`, signaling config, etc.). Services never read env vars directly. Prefer `DATABASE_URL` over discrete Postgres fields.

**Cross-service coupling to know about:** the session service *issues* signaling tickets and the signaling service *verifies* them using a shared `SIGNALING_TICKET_SECRET` (see `pkg/signalticket`). The gateway is the only public ingress; it proxies to internal services.

## Desktop agent architecture

Cargo workspace under `desktop-agent/crates/`, split so platform-specific code is isolated behind traits and the core runtime stays testable:
- `core` — runtime, config, identity, single-instance, autostart, subsystem lifecycle
- `crypto` — X25519 → HKDF-SHA256 → AES-256-GCM secure channel (pure Rust)
- `capture` / `input` — screen capture and keyboard/mouse/clipboard injection; **real backends are behind the `native` feature**, off by default (default = no-op stubs for CI)
- `transport` — WebRTC + WebSocket signaling (`tokio-tungstenite` + rustls, pure Rust)
- `backend` — REST client for enrollment/auth/device-registration/pairing
- `config-ui` — Tauri configuration UI (the only UI in the agent)

**Key convention:** anything requiring system libraries or a real display/OS permission must be gated behind a `native` cargo feature so the default build stays dependency-free and unit-testable on headless Linux CI. Pure-Rust crypto/transport primitives are chosen deliberately (rustls over system OpenSSL) for the same reason.

## Mobile architecture

Flutter app under `mobile/lib/`, feature-first + Clean Architecture. Each feature in `lib/features/<name>/` has `domain/`, `data/`, `application/` (Riverpod providers/notifiers), and `presentation/` layers. Features: `auth devices pairing viewer session signaling devtools security`. Shared infra in `lib/core/` (`network` = Dio, `storage` = secure storage/Hive, `config`, `util`); app shell + routing (Go Router) in `lib/app/`.

The mobile crypto (`cryptography` package: X25519/HKDF-SHA256/AES-256-GCM) is **wire-compatible with the Rust agent's `desksync-crypto` crate** — changes to the secure-channel format must be made on both sides in lockstep.

## Security model (do not weaken)

End-to-end encryption between phone and laptop; the server sees only public keys, never private keys. X25519 ECDH → HKDF → AES-256-GCM with replay protection/nonce validation, Ed25519 device certificates (`pkg/devicecert`), and TLS certificate pinning on the clients. JWT access tokens + rotating refresh tokens for API auth (`pkg/jwtauth`, `pkg/middleware`). When touching pairing, signaling, or the data channel, preserve these properties.

## CI

`.github/workflows/ci.yml` runs: backend build/vet/test (Go 1.25), backend integration tests (with Postgres + Redis service containers, `GOWORK=off`), and the Rust agent default build. `cd.yml` publishes images to GHCR and deploys via the `helm/desksync` umbrella chart. `make ci` reproduces the core backend gate locally.
