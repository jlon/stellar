.PHONY: help build build-static dev-backend dev-frontend docker-build docker-up docker-down clean

# Project paths
PROJECT_ROOT := $(shell pwd)
BACKEND_DIR := $(PROJECT_ROOT)/backend
FRONTEND_DIR := $(PROJECT_ROOT)/frontend
BUILD_DIR := $(PROJECT_ROOT)/build
DIST_DIR := $(BUILD_DIR)/dist

# Default target - show help
help:
	@echo "Stellar - Build Commands:"
	@echo ""
	@echo "Development (fast feedback, glibc, no packaging):"
	@echo "  make dev-backend      - Start backend via scripts/dev/start_backend.sh"
	@echo "  make dev-frontend     - Start Angular dev server via scripts/dev/start_frontend.sh"
	@echo ""
	@echo "Production builds (output: build/dist/, packaged as stellar-<ver>-linux-<arch>[-musl].tar.gz + SHA256SUMS):"
	@echo "  make build            - glibc release binary + embedded frontend + tarball"
	@echo "  make build-static     - fully static musl release binary (cargo zigbuild) + tarball"
	@echo "  make package          - (re)package existing build/dist without rebuilding"
	@echo ""
	@echo "Docker:"
	@echo "  make docker-build     - Build Docker image"
	@echo "  make docker-up        - Start Docker container (uses existing image)"
	@echo "  make docker-down      - Stop Docker container"
	@echo "  make clean            - Clean build artifacts"

# ---- Development environment (no binary packaging) ----
dev-backend:
	@bash scripts/dev/start_backend.sh

dev-frontend:
	@bash scripts/dev/start_frontend.sh

# ---- Production build helpers ----
# Frontend is required for embedding; SKIP_FRONTEND=1 reuses an existing frontend/dist.
build-frontend:
	@bash build/build-frontend.sh

# ---- Production: glibc release binary ----
build:
	@echo "Building Stellar (glibc release)"
	@echo "Step 1: Building frontend (required for embedding)..."
	@bash build/build-frontend.sh
	@echo ""
	@echo "Step 2: Running clippy checks on backend..."
	@cd $(BACKEND_DIR) && cargo clippy --release --all-targets -- --deny warnings --allow clippy::uninlined-format-args
	@echo "✓ Clippy checks passed!"
	@echo ""
	@echo "Step 3: Building backend (with embedded frontend)..."
	@bash build/build-backend.sh
	@echo ""
	@echo "Step 4: Packaging..."
	@bash build/package.sh

# ---- Production: fully static musl release binary ----
build-static:
	@echo "Building Stellar (static musl release)"
	@echo "Step 1: Building frontend (required for embedding)..."
	@bash build/build-frontend.sh
	@echo ""
	@echo "Step 2: Running clippy checks on backend..."
	@cd $(BACKEND_DIR) && cargo clippy --release --all-targets -- --deny warnings --allow clippy::uninlined-format-args
	@echo "✓ Clippy checks passed!"
	@echo ""
	@echo "Step 3: Building backend (BUILD_TARGET=x86_64-unknown-linux-musl)..."
	@BUILD_TARGET=x86_64-unknown-linux-musl bash build/build-backend.sh
	@echo ""
	@echo "Step 4: Packaging..."
	@BUILD_TARGET=x86_64-unknown-linux-musl bash build/package.sh

# Repackage existing dist (e.g. after a manual build) without rebuilding
package:
	@bash build/package.sh

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