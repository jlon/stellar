#!/usr/bin/env bash

#
# Stellar - Debian Package Script
# Wraps build/dist/bin/stellar (musl static binary) into a .deb package:
#   /opt/stellar/{bin,conf,data,logs} + /usr/bin/stellar symlink + systemd unit.
# Layout mirrors the Docker/tarball deployment; no code changes needed.
#
# Usage: bash build/deb-package.sh
#   Requires build/dist/bin/stellar (run make build first)
#

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/build/dist"

if [ ! -x "$DIST_DIR/bin/stellar" ]; then
    echo "Error: $DIST_DIR/bin/stellar not found or not executable. Run make build first." >&2
    exit 1
fi
if ! file "$DIST_DIR/bin/stellar" | grep -q 'statically linked'; then
    echo "Error: Debian packages must contain the static musl binary. Run make build first." >&2
    exit 1
fi
if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "Error: dpkg-deb not found (install dpkg-dev on Debian/Ubuntu)." >&2
    exit 1
fi

VERSION=$(grep '^version' "$PROJECT_ROOT/backend/Cargo.toml" | head -1 | cut -d'"' -f2)
PACKAGE_NAME="stellar-server-$VERSION-amd64"

echo -e "\033[1;33m[deb-package]\033[0m Packaging $DIST_DIR/bin/stellar -> ${PACKAGE_NAME}.deb"

ROOT_DIR=$(mktemp -d)
trap 'rm -rf "$ROOT_DIR"' EXIT

# ---- Filesystem layout (mirrors Docker/tarball: cwd-relative conf/data) ----
mkdir -p "$ROOT_DIR/DEBIAN"
mkdir -p "$ROOT_DIR/opt/stellar/bin" "$ROOT_DIR/opt/stellar/conf" \
         "$ROOT_DIR/opt/stellar/data" "$ROOT_DIR/opt/stellar/logs"
mkdir -p "$ROOT_DIR/usr/bin"
mkdir -p "$ROOT_DIR/lib/systemd/system"

cp "$DIST_DIR/bin/stellar" "$ROOT_DIR/opt/stellar/bin/stellar"
chmod 755 "$ROOT_DIR/opt/stellar/bin/stellar"
cp "$DIST_DIR/conf/config.toml" "$ROOT_DIR/opt/stellar/conf/config.toml"
ln -s /opt/stellar/bin/stellar "$ROOT_DIR/usr/bin/stellar"

# ---- DEBIAN metadata ----
cat > "$ROOT_DIR/DEBIAN/control" <<EOF
Package: stellar-server
Version: $VERSION
Section: admin
Priority: optional
Architecture: amd64
Maintainer: Stellar Team <support@stellar.example>
Description: Stellar - enterprise OLAP cluster management platform
 StarRocks & Apache Doris unified management. Fully static single binary;
 no runtime dependencies.
EOF

# config.toml is user-editable; keep it on upgrades
echo "/opt/stellar/conf/config.toml" > "$ROOT_DIR/DEBIAN/conffiles"

cat > "$ROOT_DIR/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
# Dedicated system user
if ! getent passwd stellar >/dev/null; then
    useradd --system --home /opt/stellar --shell /usr/sbin/nologin stellar
fi
chown -R stellar:stellar /opt/stellar/data /opt/stellar/logs
chmod 750 /opt/stellar/data /opt/stellar/logs
# Expose the systemd unit
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload
    systemctl enable stellar >/dev/null 2>&1 || true
fi
exit 0
EOF
chmod 755 "$ROOT_DIR/DEBIAN/postinst"

# ---- systemd unit ----
cat > "$ROOT_DIR/lib/systemd/system/stellar.service" <<EOF
[Unit]
Description=Stellar - OLAP Cluster Management Platform
After=network.target

[Service]
Type=simple
User=stellar
Group=stellar
WorkingDirectory=/opt/stellar
ExecStart=/opt/stellar/bin/stellar server /opt/stellar/data
Restart=on-failure
RestartSec=3
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
EOF

# ---- Build the .deb ----
cd "$ROOT_DIR"
dpkg-deb --build --root-owner-group . "$DIST_DIR/$PACKAGE_NAME.deb" >/dev/null

cd "$DIST_DIR"
sha256sum "$PACKAGE_NAME.deb" > "$PACKAGE_NAME.deb.sha256"

echo "Package: $PACKAGE_NAME.deb ($(du -h "$PACKAGE_NAME.deb" | cut -f1))"
dpkg-deb --info "$PACKAGE_NAME.deb" 2>/dev/null | grep -E "Package|Version|Architecture|Installed-Size" | head -4
echo "Contents:"
dpkg-deb --contents "$PACKAGE_NAME.deb" | awk '{print $6}' | grep -v "^/$" | head -12
cat "$PACKAGE_NAME.deb.sha256"