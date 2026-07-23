# ============================================================================
# DeskSync — root Makefile
# Orchestrates the Go backend, Rust desktop agent, and Flutter mobile app.
# ============================================================================

SHELL := /bin/bash
.DEFAULT_GOAL := help

GO_SERVICES := gateway auth device session pairing signaling relay notification monitoring
BACKEND_DIR := backend
AGENT_DIR   := desktop-agent
MOBILE_DIR  := mobile

# Every Go module in the backend workspace (pkg + one per service).
GO_MODULES := pkg $(addprefix services/,$(GO_SERVICES))

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
	  awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# ---------------------------------------------------------------------------
# Backend (Go)
# ---------------------------------------------------------------------------
.PHONY: deps
deps: ## Download Go module dependencies
	cd $(BACKEND_DIR) && go work sync && go mod download all

.PHONY: build
build: ## Build all Go modules (service binaries emitted to each module's bin/)
	@set -e; \
	echo ">> build pkg"; (cd $(BACKEND_DIR)/pkg && go build ./...); \
	for s in $(GO_SERVICES); do \
	  echo ">> build services/$$s"; (cd $(BACKEND_DIR)/services/$$s && go build -o bin/ ./...); \
	done

.PHONY: vet
vet: ## Run go vet across every Go module
	@set -e; for m in $(GO_MODULES); do \
	  echo ">> vet $$m"; (cd $(BACKEND_DIR)/$$m && go vet ./...); \
	done

.PHONY: test
test: ## Run backend unit tests with race detector + coverage
	@set -e; for m in $(GO_MODULES); do \
	  echo ">> test $$m"; \
	  (cd $(BACKEND_DIR)/$$m && go test -race -covermode=atomic -coverprofile=coverage.out ./...); \
	done

.PHONY: lint
lint: ## Run golangci-lint per module (if installed)
	@for m in $(GO_MODULES); do \
	  echo ">> lint $$m"; \
	  (cd $(BACKEND_DIR)/$$m && golangci-lint run ./... || echo "golangci-lint not installed; skipping"); \
	done

.PHONY: run-%
run-%: ## Run a service, e.g. `make run-gateway`
	cd $(BACKEND_DIR) && go run ./services/$*/cmd

# ---------------------------------------------------------------------------
# Desktop agent (Rust)
# ---------------------------------------------------------------------------
.PHONY: agent-build
agent-build: ## Build the Rust desktop agent (no-op backends; portable)
	cd $(AGENT_DIR) && cargo build

.PHONY: agent-build-native
agent-build-native: ## Build the agent with real capture/input/clipboard backends
	cd $(AGENT_DIR) && cargo build -p desksync-agent --features native

.PHONY: agent-run-native
agent-run-native: ## Run the agent with native backends (needs OS permissions)
	cd $(AGENT_DIR) && cargo run -p desksync-agent --features native

.PHONY: agent-test
agent-test: ## Test the Rust desktop agent
	cd $(AGENT_DIR) && cargo test

.PHONY: agent-fmt
agent-fmt: ## Check Rust formatting
	cd $(AGENT_DIR) && cargo fmt --all --check

.PHONY: agent-clippy
agent-clippy: ## Run clippy on the desktop agent
	cd $(AGENT_DIR) && cargo clippy --all-targets -- -D warnings

# ---------------------------------------------------------------------------
# Mobile (Flutter)
# ---------------------------------------------------------------------------
.PHONY: mobile-deps
mobile-deps: ## Fetch Flutter packages
	cd $(MOBILE_DIR) && flutter pub get

.PHONY: mobile-analyze
mobile-analyze: ## Analyze the Flutter app
	cd $(MOBILE_DIR) && flutter analyze

.PHONY: mobile-test
mobile-test: ## Run Flutter tests
	cd $(MOBILE_DIR) && flutter test

# ---------------------------------------------------------------------------
# Infrastructure (Docker Compose)
# ---------------------------------------------------------------------------
.PHONY: dev-infra
dev-infra: ## Start postgres, redis, coturn locally
	docker compose -f docker/docker-compose.yml up -d postgres redis coturn

.PHONY: dev-infra-down
dev-infra-down: ## Stop local infrastructure
	docker compose -f docker/docker-compose.yml down

.PHONY: compose-validate
compose-validate: ## Validate the docker-compose file
	docker compose -f docker/docker-compose.yml config -q && echo "compose OK"

# ---------------------------------------------------------------------------
# Migrations
# ---------------------------------------------------------------------------
.PHONY: migrate-up
migrate-up: ## Apply DB migrations (requires golang-migrate)
	migrate -path $(BACKEND_DIR)/migrations -database "$$DATABASE_URL" up

.PHONY: migrate-down
migrate-down: ## Roll back the last migration
	migrate -path $(BACKEND_DIR)/migrations -database "$$DATABASE_URL" down 1

# ---------------------------------------------------------------------------
# Aggregate
# ---------------------------------------------------------------------------
.PHONY: ci
ci: build vet test ## Run the core CI checks locally

.PHONY: all
all: deps build test ## Deps + build + test
