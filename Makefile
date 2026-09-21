.PHONY: help build build-static dev-backend dev-frontend docker-build docker-up docker-down clean

# Project paths
PROJECT_ROOT := $(shell pwd)
BACKEND_DIR := $(PROJECT_ROOT)/backend
FRONTEND_DIR := $(PROJECT_ROOT)/frontend
BUILD_DIR := $(PROJECT_ROOT)/build
DIST_DIR := $(BUILD_DIR)/dist

# Rust toolchain for non-interactive shells (idempotent)
export PATH := $(HOME)/.cargo/bin:$(PATH)

# Default target - show help
help:
	@echo "Stellar - Build Commands:"
	@echo ""
	@echo "Development (fast feedback, glibc, no packaging):"
	@echo "  make dev-backend      - Start backend via scripts/dev/start_backend.sh start --dev"
	@echo "  make dev-frontend     - Start Angular dev server via scripts/dev/start_frontend.sh"
	@echo ""
	@echo "Production build (output: build/dist/):"
	@echo "  make build            - ONE-SHOT release: prod frontend + musl static binary"
	@echo "                          + tar.gz/SHA256SUMS + npm tarball + Debian package"
	@echo "  make build-static     - Alias of make build (kept for compatibility)"
	@echo "  make package          - (re)package existing build/dist without rebuilding"
	@echo "  make npm-package      - wrap existing dist/bin/stellar into stellar-server-<ver>.tgz"
	@echo "  make deb-package      - wrap existing dist/bin/stellar into stellar-server-<ver>-amd64.deb"
	@echo "  make npm-e2e          - local e2e: pack -> npm install -> run -> health/API smoke"
	@echo ""
	@echo "Docker:"
	@echo "  make docker-build     - Build Docker image"
	@echo "  make docker-up        - Start Docker container (uses existing image)"
	@echo "  make docker-down      - Stop Docker container"
	@echo "  make clean            - Clean build artifacts"

# ---- Development environment (no binary packaging) ----
dev-backend:
	@bash scripts/dev/start_backend.sh start --dev

dev-frontend:
	@bash scripts/dev/start_frontend.sh

# ---- Production build helpers ----
# Frontend is required for embedding; SKIP_FRONTEND=1 reuses an existing frontend/dist.
build-frontend:
	@bash build/build-frontend.sh

# ---- Production: fully static musl release binary ----
# `make build` is the official command. Keep `build-static` for existing scripts.
# One-shot packaging: frontend (prod, slimmed) -> clippy -> musl static binary
# -> tar.gz + SHA256SUMS -> npm tarball + sha256.
build: build-static

build-static:
	@echo "Building Stellar (static musl release)"
	@if [ "$${SKIP_FRONTEND:-0}" = "1" ]; then \
		echo "Step 1: Reusing existing production frontend..."; \
	else \
		echo "Step 1: Building frontend (required for embedding)..."; \
		bash build/build-frontend.sh; \
	fi
	@echo ""
	@echo "Step 2: Running clippy checks on backend..."
	@cd $(BACKEND_DIR) && cargo clippy --locked --release --all-targets -- --deny warnings --allow clippy::uninlined-format-args
	@echo "✓ Clippy checks passed!"
	@echo ""
	@echo "Step 3: Building backend (BUILD_TARGET=x86_64-unknown-linux-musl)..."
	@BUILD_TARGET=x86_64-unknown-linux-musl bash build/build-backend.sh
	@echo ""
	@echo "Step 4: Packaging tar.gz + SHA256SUMS..."
	@BUILD_TARGET=x86_64-unknown-linux-musl bash build/package.sh
	@echo ""
	@echo "Step 5: Packaging npm tarball + sha256..."
	@bash build/npm-package.sh
	@echo ""
	@echo "Step 6: Packaging Debian .deb + sha256..."
	@bash build/deb-package.sh

# Repackage existing dist (e.g. after a manual build) without rebuilding
package:
	@bash build/package.sh

# Package existing dist into an npm tarball (stellar-server-<version>.tgz)
# Official release: run `make build` first so the tarball wraps the musl static binary.
npm-package:
	@bash build/npm-package.sh

# Package existing dist into a Debian .deb package (stellar-server-<ver>-amd64.deb)
# Official release: run `make build` first so the .deb wraps the musl static binary.
deb-package:
	@bash build/deb-package.sh

# Local npm end-to-end test: pack -> install -> run -> health/API smoke
npm-e2e:
	@bash build/npm-e2e.sh

# Build Docker image
docker-build:
	@echo "Building Docker image..."
	@docker build -f deploy/docker/Dockerfile -t stellar:latest .

# Start Docker container without rebuild (use existing image)
docker-up:
	@echo "Starting Docker container (using existing image)..."
	@cd deploy/docker && docker compose up -d

# Stop Docker container
docker-down:
	@echo "Stopping Docker container..."
	@cd deploy/docker && docker compose down

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@rm -rf $(BUILD_DIR)
	@cd $(BACKEND_DIR) && cargo clean
	@cd $(FRONTEND_DIR) && rm -rf dist node_modules/.cache
	@echo "Clean complete!"
