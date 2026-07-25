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
compose-validate: ## Validate the docker-compose files
	docker compose -f docker/docker-compose.yml config -q && echo "compose OK"
	docker compose -f docker/docker-compose.observability.yml config -q && echo "observability compose OK"

.PHONY: obs-up
obs-up: ## Start the observability stack (Prometheus, Grafana, Loki, Promtail)
	docker compose -f docker/docker-compose.yml -f docker/docker-compose.observability.yml up -d prometheus grafana loki promtail

.PHONY: obs-down
obs-down: ## Stop the observability stack
	docker compose -f docker/docker-compose.observability.yml down

# ---------------------------------------------------------------------------
# Container images
# ---------------------------------------------------------------------------
REGISTRY   ?= ghcr.io
IMAGE_REPO ?= dkmadhu115/desksync
IMAGE_TAG  ?= dev

.PHONY: images
images: ## Build all service images + the migrations image locally
	@set -e; for s in $(GO_SERVICES); do \
	  echo ">> image $$s"; \
	  docker build -f docker/Dockerfile.service --build-arg SERVICE=$$s \
	    -t $(REGISTRY)/$(IMAGE_REPO)/$$s:$(IMAGE_TAG) backend; \
	done; \
	echo ">> image migrations"; \
	docker build -f docker/Dockerfile.migrations -t $(REGISTRY)/$(IMAGE_REPO)/migrations:$(IMAGE_TAG) .

.PHONY: image-%
image-%: ## Build a single service image, e.g. `make image-gateway`
	docker build -f docker/Dockerfile.service --build-arg SERVICE=$* \
	  -t $(REGISTRY)/$(IMAGE_REPO)/$*:$(IMAGE_TAG) backend

# ---------------------------------------------------------------------------
# Kubernetes / Helm
# ---------------------------------------------------------------------------
HELM_CHART   := helm/desksync
HELM_RELEASE ?= desksync
HELM_NS      ?= desksync

.PHONY: helm-lint
helm-lint: ## Lint the Helm chart
	helm lint $(HELM_CHART)

.PHONY: helm-template
helm-template: ## Render the chart with all optional toggles enabled
	helm template $(HELM_RELEASE) $(HELM_CHART) \
	  --set serviceMonitor.enabled=true \
	  --set prometheusRule.enabled=true \
	  --set networkPolicy.enabled=true

.PHONY: helm-package
helm-package: ## Package the chart into dist/
	helm package $(HELM_CHART) --destination dist/

.PHONY: k8s-deploy
k8s-deploy: ## Install/upgrade the release (override IMAGE_TAG/HELM_NS as needed)
	helm upgrade --install $(HELM_RELEASE) $(HELM_CHART) \
	  --namespace $(HELM_NS) --create-namespace \
	  --set global.image.tag=$(IMAGE_TAG) --wait --timeout 10m

.PHONY: k8s-uninstall
k8s-uninstall: ## Uninstall the release
	helm uninstall $(HELM_RELEASE) --namespace $(HELM_NS)

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
