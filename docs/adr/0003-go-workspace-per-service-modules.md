# 3. Backend uses a Go workspace with per-service modules

- Status: Accepted
- Date: 2026-07-23

## Context

The backend comprises nine services plus shared libraries. We need independent
deployability and clear ownership boundaries, while sharing common code
(config, logging, HTTP bootstrap, metrics, errors) without copy-paste.

Options considered:

1. **Single Go module** for the whole backend with services as packages.
2. **One module per service** joined by a Go workspace (`go.work`), with a
   shared `pkg` module.
3. Separate repositories per service (polyrepo).

## Decision

Use option 2: a `go.work` workspace with one module per service under
`services/` and a shared `github.com/desksync/backend/pkg` module. Service
modules depend on `pkg` via a `replace` directive so they build both inside the
workspace and standalone (e.g. in per-service Docker builds and CI).

## Consequences

- Each service can be versioned, built, and containerized independently.
- Shared behavior lives in `pkg` (`config`, `logger`, `httpx`, `observability`,
  `errors`, `service`) — DRY and consistent operability.
- Build/test tooling iterates per module (see the root `Makefile`) because a
  non-module directory cannot be targeted with a single `go build ./...`.
- Slightly more `go.mod` files to manage; accepted for the isolation benefits.
