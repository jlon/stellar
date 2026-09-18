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

if [ ! -x "$DIST_DIR/bin/stellar" ]; then
    echo "Error: $DIST_DIR/bin/stellar not found or not executable. Run the build step first." >&2
    exit 1
fi

BINARY_IS_STATIC=false
if file "$DIST_DIR/bin/stellar" | grep -q 'statically linked'; then
    BINARY_IS_STATIC=true
fi

# Resolve package name. Static release artifacts are musl in this project; infer
# that default from the built binary so `make package` cannot mislabel it as glibc.
if [ -z "${BUILD_TARGET:-}" ]; then
    if "$BINARY_IS_STATIC"; then
        BUILD_TARGET="x86_64-unknown-linux-musl"
    else
        BUILD_TARGET="x86_64-unknown-linux-gnu"
    fi
fi
if [[ "$BUILD_TARGET" == *-musl ]] && ! "$BINARY_IS_STATIC"; then
    echo "Error: expected a statically linked musl binary for BUILD_TARGET=$BUILD_TARGET." >&2
    exit 1
fi
if [[ "$BUILD_TARGET" == *-gnu ]] && "$BINARY_IS_STATIC"; then
    echo "Error: refusing to label a static release binary as glibc." >&2
    exit 1
fi

if [ -n "$1" ]; then
    PACKAGE_NAME="$1"
else
    VERSION=$(grep '^version' "$PROJECT_ROOT/backend/Cargo.toml" | head -1 | cut -d'"' -f2)
    ARCH=$(uname -m | sed 's/x86_64/amd64/; s/aarch64/arm64/')
    SUFFIX=""
    case "$BUILD_TARGET" in
        *-musl) SUFFIX="-musl" ;;
    esac
    PACKAGE_NAME="stellar-$VERSION-linux-$ARCH$SUFFIX"
fi

echo -e "\033[1;33m[package]\033[0m Packaging $DIST_DIR -> ${PACKAGE_NAME}.tar.gz"

# Never ship runtime state: purge data/logs/lib leftovers (e.g. if a server was
# started from build/dist) and stale migration copies (migrations are embedded).
find "$DIST_DIR/data" "$DIST_DIR/logs" "$DIST_DIR/lib" -mindepth 1 -delete 2>/dev/null || true
rm -rf "$DIST_DIR/migrations"

# Ship the systemd unit inside the tarball so tar users can follow the
# deployment guide without cloning the source tree (deploy/systemd/).
mkdir -p "$DIST_DIR/deploy/systemd"
cp "$PROJECT_ROOT/deploy/systemd/stellar.service" "$DIST_DIR/deploy/systemd/" 2>/dev/null || true

cd "$DIST_DIR"
# Top-level directory carries the version (stellar-<ver>/) so that newer
# releases extract alongside - not on top of - older ones.
tar -czf "${PACKAGE_NAME}.tar.gz" --transform "s,^,stellar-${VERSION}/," bin conf lib data logs deploy

sha256sum "${PACKAGE_NAME}.tar.gz" > SHA256SUMS

# Verify package
echo "Package: ${PACKAGE_NAME}.tar.gz ($(du -h "${PACKAGE_NAME}.tar.gz" | cut -f1))"
tar -tzf "${PACKAGE_NAME}.tar.gz" | head -10
cat SHA256SUMS