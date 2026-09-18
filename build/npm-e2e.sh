#!/usr/bin/env bash

#
# Stellar - NPM End-to-end Test (local)
# Full customer journey in one shot:
#   pack -> npm install -g -> configure working dir -> start -> health/API smoke -> stop.
# Also smoke-tests the release tarball (unpack + run with its own config).
#
# Usage: bash build/npm-e2e.sh
#   PORT=18095 bash build/npm-e2e.sh    # override service port (default 18095)
#

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/build/dist"
PORT="${PORT:-18095}"
APP_DIR=$(mktemp -d)      # customer working dir (conf/ + data/)
PREFIX_DIR=$(mktemp -d)   # npm global prefix (simulates customer's node env)
PID=""

cleanup() {
    if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
        kill -TERM "$PID" 2>/dev/null || true
        sleep 1
        kill -KILL "$PID" 2>/dev/null || true
    fi
    rm -rf "$APP_DIR" "$PREFIX_DIR"
}
trap cleanup EXIT

# 1. Pack the npm tarball
bash "$PROJECT_ROOT/build/npm-package.sh" >/dev/null

# 2. Install as a customer would
VERSION=$(grep '^version' "$PROJECT_ROOT/backend/Cargo.toml" | head -1 | cut -d'"' -f2)
TGZ="$DIST_DIR/stellar-server-$VERSION.tgz"
[ -f "$TGZ" ] || { echo "FAIL: $TGZ missing"; exit 1; }
echo "[e2e] npm install -g stellar-server-$VERSION.tgz (prefix=$PREFIX_DIR)"
npm install -g --prefix "$PREFIX_DIR" "$TGZ" >/dev/null
STELLAR_BIN="$PREFIX_DIR/bin/stellar"
[ -x "$STELLAR_BIN" ] || { echo "FAIL: bin/stellar not installed"; exit 1; }
"$STELLAR_BIN" --version

# 3. Start the service from the customer working dir (zero-config data-dir mode)
echo "[e2e] starting stellar on 127.0.0.1:$PORT (cwd=$APP_DIR, data=$APP_DIR/data)"
cd "$APP_DIR"
nohup "$STELLAR_BIN" server "$APP_DIR/data" --server-host 127.0.0.1 --server-port "$PORT" >"$APP_DIR/stellar.log" 2>&1 &
PID=$!
cd "$PROJECT_ROOT"

# 5. Wait for /health, then log in with the one-time initial password
HEALTH=""
for _ in $(seq 1 30); do
    if curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then HEALTH=ok; break; fi
    sleep 1
done
if [ "$HEALTH" != ok ]; then
    echo "FAIL: /health not ready after 30s. Log tail:" >&2
    tail -20 "$APP_DIR/stellar.log" >&2
    exit 1
fi
echo "[e2e] /health OK"
PASSWORD=$(grep -oP "Created initial user 'admin' with password: \K\S+" "$APP_DIR/stellar.log" | tail -1)
[ -n "$PASSWORD" ] || { echo "FAIL: one-time password not found in startup log" >&2; tail -20 "$APP_DIR/stellar.log" >&2; exit 1; }
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/api/auth/login" \
    -H 'Content-Type: application/json' -d "{\"username\":\"admin\",\"password\":\"$PASSWORD\"}")
[ "$CODE" = 200 ] || { echo "FAIL: initial admin login returned $CODE" >&2; exit 1; }
echo "[e2e] initial admin login -> 200"
SWAGGER=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/swagger-ui/")
[ "$SWAGGER" = 200 ] && echo "[e2e] /swagger-ui/ -> 200" || echo "WARN: /swagger-ui/ -> $SWAGGER"

# 7. Tarball smoke test: unpack and run via the shipped stellar.sh (data-dir mode)
VERSION=$(grep '^version' "$PROJECT_ROOT/backend/Cargo.toml" | head -1 | cut -d'"' -f2)
TGZ_TAR="$DIST_DIR/stellar-$VERSION-linux-amd64-musl.tar.gz"
TGZ_PORT=$((PORT + 1))
if [ -f "$TGZ_TAR" ]; then
    TAR_DIR=$(mktemp -d)
    echo "[e2e] tarball smoke: $TGZ_TAR"
    tar -xzf "$TGZ_TAR" -C "$TAR_DIR"
    cd "$TAR_DIR/stellar-$VERSION"
    PORT=$TGZ_PORT ./bin/stellar.sh start
    cd "$PROJECT_ROOT"
    TAR_HEALTH=""
    for _ in $(seq 1 30); do
        if curl -sf "http://127.0.0.1:$TGZ_PORT/health" >/dev/null 2>&1; then TAR_HEALTH=ok; break; fi
        sleep 1
    done
    [ "$TAR_HEALTH" = ok ] || { echo "FAIL: tarball /health not ready" >&2; tail -20 "$TAR_DIR/stellar-$VERSION/data/logs/console.log" >&2; PORT=$TGZ_PORT "$TAR_DIR/stellar-$VERSION/bin/stellar.sh" stop || true; exit 1; }
    echo "[e2e] tarball /health OK (port $TGZ_PORT)"
    PORT=$TGZ_PORT "$TAR_DIR/stellar-$VERSION/bin/stellar.sh" stop
    rm -rf "$TAR_DIR"
fi

# 8. Graceful stop
kill -TERM "$PID"
sleep 1
kill -0 "$PID" 2>/dev/null && { echo "FAIL: process still running after TERM" >&2; exit 1; }
PID=""
echo "PASS: npm e2e OK (install -> run -> health -> API)"