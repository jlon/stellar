# syntax=docker/dockerfile:1.7

ARG NODE_VERSION=22
ARG RUST_VERSION=1.90.0
ARG ZIG_VERSION=0.13.0
ARG ZIG_AMD64_SHA256=d45312e61ebcc48032b77bc4cf7fd6915c11fa16e4aad116b66c9468211230ea
ARG ZIG_ARM64_SHA256=041ac42323837eb5624068acd8b00cd5777dac4cf91179e8dad7a7e90dd0c556
ARG CARGO_ZIGBUILD_VERSION=0.23.4

# Build the frontend through the same script used by release packages so the
# embedded assets (including gzip compression) are identical across channels.
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-bookworm-slim AS frontend-builder
WORKDIR /app
COPY frontend/package.json frontend/package-lock.json frontend/.npmrc ./frontend/
RUN cd frontend && npm ci
COPY frontend/ ./frontend/
COPY build/build-frontend.sh ./build/build-frontend.sh
RUN SKIP_NPM_INSTALL=1 bash ./build/build-frontend.sh

# Build on Buildx's native builder and use Zig to cross-compile the requested
# target. This avoids QEMU-emulating a full Rust build for the arm64 image.
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-bookworm AS backend-builder
ARG BUILDARCH
ARG TARGETARCH
ARG ZIG_VERSION
ARG ZIG_AMD64_SHA256
ARG ZIG_ARM64_SHA256
ARG CARGO_ZIGBUILD_VERSION
RUN case "$BUILDARCH" in \
        amd64) ZIG_ARCH=x86_64; ZIG_SHA256="$ZIG_AMD64_SHA256" ;; \
        arm64) ZIG_ARCH=aarch64; ZIG_SHA256="$ZIG_ARM64_SHA256" ;;  \
        *) echo "Unsupported build architecture: $BUILDARCH" >&2; exit 1 ;; \
    esac; \
    case "$TARGETARCH" in \
        amd64) RUST_TARGET=x86_64-unknown-linux-musl ;; \
        arm64) RUST_TARGET=aarch64-unknown-linux-musl ;; \
        *) echo "Unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl file xz-utils; \
    rm -rf /var/lib/apt/lists/*; \
    curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}.tar.xz" -o /tmp/zig.tar.xz; \
    echo "$ZIG_SHA256  /tmp/zig.tar.xz" | sha256sum --check; \
    tar -xf /tmp/zig.tar.xz -C /opt; \
    ln -s "/opt/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}/zig" /usr/local/bin/zig; \
    rm /tmp/zig.tar.xz; \
    rustup target add "$RUST_TARGET"; \
    cargo install cargo-zigbuild --version "$CARGO_ZIGBUILD_VERSION" --locked

WORKDIR /app
COPY backend/ ./backend/
COPY --from=frontend-builder /app/frontend/dist ./frontend/dist
WORKDIR /app/backend
RUN --mount=type=cache,id=cargo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-git-${TARGETARCH},target=/usr/local/cargo/git \
    --mount=type=cache,id=cargo-target-${TARGETARCH},target=/app/backend/target \
    case "$TARGETARCH" in \
        amd64) RUST_TARGET=x86_64-unknown-linux-musl ;; \
        arm64) RUST_TARGET=aarch64-unknown-linux-musl ;; \
        *) echo "Unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    cargo zigbuild --locked --release --target "$RUST_TARGET" --bin stellar; \
    install -Dm755 "target/${RUST_TARGET}/release/stellar" /out/stellar; \
    file /out/stellar; \
    file /out/stellar | grep -q 'statically linked'

# Debian remains the runtime base for CA roots and curl-based HEALTHCHECK; the
# application binary itself is fully static and has no glibc runtime dependency.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin starrocks

WORKDIR /app
RUN mkdir -p bin conf lib data logs
COPY --from=backend-builder --chown=starrocks:starrocks /out/stellar ./bin/stellar
COPY --chmod=755 deploy/docker/entrypoint.sh /usr/local/bin/stellar-entrypoint
RUN printf '%s\n' \
    '[server]' \
    'host = "0.0.0.0"' \
    'port = 9527' \
    '' \
    '[database]' \
    'url = "sqlite:///data/stellar.db"' \
    '' \
    '[auth]' \
    '# APP_JWT_SECRET takes precedence; `server /data --config ...` generates .jwt-secret.' \
    'jwt_secret = ""' \
    'jwt_expires_in = "24h"' \
    '' \
    '[logging]' \
    'level = "info,stellar_backend=debug"' \
    'file = "/data/logs/stellar.log"' \
    '' \
    '[metrics]' \
    'interval_secs = "30s"' \
    'retention_days = "7d"' \
    'enabled = true' \
    '' \
    '[audit]' \
    'database = "starrocks_audit_db__"' \
    'table = "starrocks_audit_tbl__"' \
    > ./conf/config.toml \
    && chown -R starrocks:starrocks /app

# Entrypoint normalizes /data ownership then drops to this user via setpriv.
EXPOSE 9527
# Everything (SQLite, .jwt-secret, logs) lives in one volume; the entrypoint
# defaults to `stellar server /data --config /app/conf/config.toml`.
VOLUME "/data"
HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD curl -f http://localhost:9527/health || exit 1
ENTRYPOINT ["/usr/local/bin/stellar-entrypoint"]
