#!/usr/bin/env bash

#
# Stellar - NPM Package Script
# Wraps build/dist/bin/stellar (single binary) into an npm package
# stellar-server-<version>.tgz + SHA256 checksum, output to build/dist.
# npm is only used here as a packager/installer; the binary itself does
# NOT require a Node runtime on the customer machine.
#
# Usage: bash build/npm-package.sh
#   Requires build/dist/bin/stellar from make build
#

set -e

# Get project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/build/dist"

if [ ! -x "$DIST_DIR/bin/stellar" ]; then
    echo "Error: $DIST_DIR/bin/stellar not found or not executable. Run make build first." >&2
    exit 1
fi
if ! file "$DIST_DIR/bin/stellar" | grep -q 'statically linked'; then
    echo "Error: npm packages must contain the static musl binary. Run make build first." >&2
    exit 1
fi

if ! command -v npm >/dev/null 2>&1; then
    echo "Error: npm not found. Needed only to pack the tarball; customers do not need Node." >&2
    exit 1
fi

# Resolve version from Cargo.toml (single source of truth)
VERSION=$(grep '^version' "$PROJECT_ROOT/backend/Cargo.toml" | head -1 | cut -d'"' -f2)
PACKAGE_NAME="stellar-server-$VERSION"

echo -e "\033[1;33m[npm-package]\033[0m Packaging $DIST_DIR/bin/stellar -> ${PACKAGE_NAME}.tgz"

# Build the package in a temp dir, keep the repo clean
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$TMP_DIR/bin"
cp "$DIST_DIR/bin/stellar" "$TMP_DIR/bin/stellar"
chmod 755 "$TMP_DIR/bin/stellar"

cat > "$TMP_DIR/package.json" <<EOF
{
  "name": "stellar-server",
  "version": "$VERSION",
  "description": "Stellar - enterprise OLAP cluster management platform (StarRocks & Apache Doris)",
  "license": "Apache-2.0",
  "bin": {
    "stellar": "bin/stellar"
  },
  "files": [
    "bin/stellar"
  ],
  "os": [
    "linux"
  ]
}
EOF

cd "$TMP_DIR"
npm pack --pack-destination "$DIST_DIR" >/dev/null

# Checksum (kept separate from package.sh's SHA256SUMS to avoid overwriting it)
cd "$DIST_DIR"
sha256sum "$PACKAGE_NAME.tgz" > "$PACKAGE_NAME.tgz.sha256"

# Verify package
echo "Package: $PACKAGE_NAME.tgz ($(du -h "$PACKAGE_NAME.tgz" | cut -f1))"
tar -tzf "$PACKAGE_NAME.tgz" | head -5
cat "$PACKAGE_NAME.tgz.sha256"