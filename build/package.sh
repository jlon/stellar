#!/usr/bin/env bash

#
# Stellar - Build Package Script
# Packages build/dist into a versioned tarball + SHA256SUMS.
# Shared by `make build` / `make build-static` and the release CI workflow.
#
# Usage: bash build/package.sh [OUTPUT_NAME_PREFIX]
#   e.g. bash build/package.sh                 # reads version from Cargo.toml
#        bash build/package.sh stellar-1.0.0-linux-amd64-musl
#

set -e

# Get project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/build/dist"

if [ ! -d "$DIST_DIR/bin" ]; then
    echo "Error: $DIST_DIR/bin not found. Run the build step first (make build / make build-static)." >&2
    exit 1
fi

# Resolve package name
if [ -n "$1" ]; then
    PACKAGE_NAME="$1"
else
    VERSION=$(grep '^version' "$PROJECT_ROOT/backend/Cargo.toml" | head -1 | cut -d'"' -f2)
    export BUILD_TARGET="${BUILD_TARGET:-x86_64-unknown-linux-gnu}"
    ARCH=$(uname -m | sed 's/x86_64/amd64/; s/aarch64/arm64/')
    SUFFIX=""
    case "$BUILD_TARGET" in
        *-musl) SUFFIX="-musl" ;;
    esac
    PACKAGE_NAME="stellar-$VERSION-linux-$ARCH$SUFFIX"
fi

echo -e "\033[1;33m[package]\033[0m Packaging $DIST_DIR -> ${PACKAGE_NAME}.tar.gz"

cd "$DIST_DIR"
tar -czf "${PACKAGE_NAME}.tar.gz" --transform 's,^,stellar/,' bin conf lib data logs

sha256sum "${PACKAGE_NAME}.tar.gz" > SHA256SUMS

# Verify package
echo "Package: ${PACKAGE_NAME}.tar.gz ($(du -h "${PACKAGE_NAME}.tar.gz" | cut -f1))"
tar -tzf "${PACKAGE_NAME}.tar.gz" | head -10
cat SHA256SUMS