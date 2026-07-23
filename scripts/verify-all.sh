#!/usr/bin/env bash
# Run every Phase 1 quality gate locally: backend, desktop agent, mobile,
# OpenAPI, and docker-compose validation. Mirrors the GitHub Actions CI jobs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Make locally-installed toolchains discoverable.
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin"

echo "==> Backend (Go): build, vet, test"
make build vet test

echo "==> Desktop agent (Rust): fmt, clippy, test"
( cd desktop-agent && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test --all )

echo "==> Mobile (Flutter): analyze, test"
( cd mobile && flutter pub get && flutter analyze && flutter test )

echo "==> OpenAPI lint"
npx --yes @redocly/cli@latest lint backend/api/openapi.yaml

echo "==> docker compose config"
docker compose -f docker/docker-compose.yml config -q && echo "compose OK"

echo "All Phase 1 checks passed."
