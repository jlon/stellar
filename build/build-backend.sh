#!/usr/bin/env bash

#
# Stellar - Backend Build Script
# Builds the Rust backend and outputs to build/dist/
# Backend embeds frontend assets directly from frontend/dist/
#

set -e

# --- 守卫：只允许嵌入生产前端，避免 dev 构建（含 sourcemap）使二进制翻倍 ---
FRONTEND_DIST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../frontend/dist" 2>/dev/null && pwd)" || {
    echo "错误: frontend/dist 不存在。请先执行 bash build/build-frontend.sh。" >&2
    exit 1
}
if [ -z "$(ls -A "$FRONTEND_DIST_DIR" 2>/dev/null)" ]; then
    echo "错误: frontend/dist 为空，无法嵌入前端。请先执行 bash build/build-frontend.sh。" >&2
    exit 1
fi
MAP_COUNT=$(find "$FRONTEND_DIST_DIR" -name '*.map' 2>/dev/null | wc -l)
FRONTEND_DIST_MB=$(du -sm "$FRONTEND_DIST_DIR" | cut -f1)
if [ "$MAP_COUNT" -gt 0 ] || [ "$FRONTEND_DIST_MB" -gt 20 ]; then
    echo "错误: frontend/dist 看起来是开发构建（${FRONTEND_DIST_MB}MB，${MAP_COUNT} 个 sourcemap）。" >&2
    echo "      生产构建请执行: bash build/build-frontend.sh" >&2
    echo "      否则嵌入资源会从约 7MB 膨胀到 70MB+，二进制体积翻倍。" >&2
    exit 1
fi
echo "[guard] frontend/dist 检查通过: ${FRONTEND_DIST_MB}MB，无 sourcemap"


# Get project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BACKEND_DIR="$PROJECT_ROOT/backend"
BUILD_DIR="$PROJECT_ROOT/build"
DIST_DIR="$BUILD_DIR/dist"

# Target selection: BUILD_TARGET override, default native glibc.
#   glibc:  x86_64-unknown-linux-gnu (dev / quick builds)
#   static: x86_64-unknown-linux-musl via cargo zigbuild (release artifact)
BUILD_TARGET="${BUILD_TARGET:-x86_64-unknown-linux-gnu}"
case "$BUILD_TARGET" in
    *-musl) BUILD_CMD=(cargo zigbuild) ;;
    *)      BUILD_CMD=(cargo build) ;;
esac

# Non-interactive shells miss ~/.cargo/bin; make it available (idempotent).
export PATH="$HOME/.cargo/bin:$PATH"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Building Stellar Backend${NC}"
echo -e "${GREEN}Target: $BUILD_TARGET${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""

# Verify toolchain for musl (requires zig + cargo-zigbuild)
if [[ "$BUILD_TARGET" == *-musl ]]; then
    # Fall back to well-known zig install locations when not on PATH (e.g. /opt/zig)
    if ! command -v zig >/dev/null; then
        for ZIG_DIR in /opt/zig /usr/local/zig /usr/lib/zig; do
            if [ -x "$ZIG_DIR/zig" ]; then
                export PATH="$ZIG_DIR:$PATH"
                break
            fi
        done
    fi
    if ! command -v zig >/dev/null; then
        echo -e "${RED}Error: zig not found. Install it (e.g. apt install zig or from ziglang.org) or add it to PATH.${NC}" >&2
        exit 1
    fi
    if ! command -v cargo-zigbuild >/dev/null; then
        echo -e "${RED}Error: cargo-zigbuild not found. Run: cargo install cargo-zigbuild${NC}" >&2
        exit 1
    fi
fi


# Create dist directories
echo -e "${YELLOW}[0/4]${NC} Creating build directories..."
mkdir -p "$DIST_DIR/bin"
mkdir -p "$DIST_DIR/conf"
mkdir -p "$DIST_DIR/lib"
mkdir -p "$DIST_DIR/data"
mkdir -p "$DIST_DIR/logs"

# Clean old backend artifacts
echo -e "${YELLOW}[0/4]${NC} Cleaning old backend artifacts..."
rm -f "$DIST_DIR/bin/"*
rm -f "$DIST_DIR/conf/"*
rm -f "$DIST_DIR/lib/"*

# Build backend
echo -e "${YELLOW}[1/4]${NC} Compiling Rust backend (release, $BUILD_TARGET)..."
cd "$BACKEND_DIR"
# Cargo 的 target-dir 可来自环境变量或 .cargo/config.toml；让 Cargo 报告实际目录，禁止拷贝旧产物。
CARGO_BUILD_DIR="$(cargo metadata --no-deps --format-version=1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
if [ -z "$CARGO_BUILD_DIR" ]; then
    echo "Error: unable to determine Cargo target directory." >&2
    exit 1
fi
"${BUILD_CMD[@]}" --locked --release --target "$BUILD_TARGET" --bin stellar

# Copy the binary cargo just built. CARGO_TARGET_DIR is commonly set by CI/local toolchains.
echo -e "${YELLOW}[2/4]${NC} Copying backend binary..."
ARTIFACT_PATH="$CARGO_BUILD_DIR/$BUILD_TARGET/release/stellar"
if [ ! -x "$ARTIFACT_PATH" ]; then
    echo "Error: expected build artifact not found: $ARTIFACT_PATH" >&2
    exit 1
fi
cp "$ARTIFACT_PATH" "$DIST_DIR/bin/"

# Create production configuration file
echo -e "${YELLOW}[3/4]${NC} Creating production configuration file..."
cat > "$DIST_DIR/conf/config.toml" << 'EOF'
[server]
host = "0.0.0.0"
port = 9527

[database]
url = "sqlite://data/stellar.db"

[auth]
# APP_JWT_SECRET takes precedence. The release script supplies a data directory,
# so an empty value is generated once and persisted as data/.jwt-secret.
jwt_secret = ""
jwt_expires_in = "24h"

[logging]
level = "info,stellar_backend=debug"
file = "data/logs/stellar.log"


# Metrics collector configuration
[metrics]
interval_secs = "30s"    
retention_days = "7d"    
enabled = true

# Audit log configuration
[audit]
database = "starrocks_audit_db__"
table = "starrocks_audit_tbl__"
EOF
echo "Created production config.toml"

# Migrations are embedded into the binary at compile time (sqlx::migrate!,
# one static Migrator per backend under backend/migrations/<backend>/);
# no runtime migration files are shipped.
echo -e "${YELLOW}[4/4]${NC} Database migrations are embedded in the binary (no runtime files shipped)"

# Create enhanced start script for backend
cat > "$DIST_DIR/bin/stellar.sh" << 'EOF'
#!/bin/bash

# Stellar Backend - 生产环境管理脚本

set -e

# 配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY_PATH="$SCRIPT_DIR/stellar"
DATA_DIR="$DIST_ROOT/data"
LOG_DIR="$DATA_DIR/logs"
LOG_FILE="$LOG_DIR/console.log"
PID_FILE="$DIST_ROOT/stellar.pid"

# 颜色输出
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 显示帮助信息
show_help() {
    echo -e "${GREEN}Stellar Backend 生产环境管理脚本${NC}"
    echo ""
    echo "用法: $0 {start|stop|restart|status|logs|help}"
    echo ""
    echo "命令:"
    echo "  start   - 启动后端服务"
    echo "  stop    - 停止后端服务"
    echo "  restart - 重启后端服务"
    echo "  status  - 查看服务状态"
    echo "  logs    - 查看实时日志"
    echo "  help    - 显示此帮助信息"
    echo ""
}

# 检查服务是否运行
is_running() {
    if [ -f "$PID_FILE" ]; then
        local pid=$(cat "$PID_FILE")
        if ps -p "$pid" > /dev/null 2>&1; then
            return 0
        else
            rm -f "$PID_FILE"
            return 1
        fi
    fi
    return 1
}

# 获取服务PID
get_pid() {
    if [ -f "$PID_FILE" ]; then
        cat "$PID_FILE"
    else
        echo ""
    fi
}

# 启动服务
start_service() {
    if is_running; then
        echo -e "${YELLOW}[WARNING]${NC} 后端服务已在运行 (PID: $(get_pid))"
        return 0
    fi

    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}Stellar Backend 启动脚本${NC}"
    echo -e "${GREEN}========================================${NC}"
    echo ""

    # 检查二进制文件
    if [ ! -f "$BINARY_PATH" ]; then
        echo -e "${RED}[ERROR]${NC} 二进制文件不存在: $BINARY_PATH"
        exit 1
    fi

    # 创建必要的目录
    echo -e "${YELLOW}[INFO]${NC} 创建必要的目录..."
    mkdir -p "$LOG_DIR"
    HOST="${HOST:-0.0.0.0}"
    PORT="${PORT:-9527}"

    echo -e "${GREEN}[CONFIG]${NC} 配置信息:"
    echo "  - 二进制文件: $BINARY_PATH"
    echo "  - 数据目录: $DATA_DIR"
    echo "  - 日志目录: $LOG_DIR"
    echo "  - 监听地址: $HOST:$PORT"
    echo ""

    # 配置文件提供业务配置；数据目录仅保存 SQLite、日志和生成的 JWT 密钥。
    echo -e "${GREEN}[START]${NC} 启动后端服务..."
    (
        cd "$DIST_ROOT"
        exec nohup "$BINARY_PATH" server "$DATA_DIR" --config "$DIST_ROOT/conf/config.toml" --server-host "$HOST" --server-port "$PORT"
    ) > "$LOG_FILE" 2>&1 &
    BACKEND_PID=$!
    echo $BACKEND_PID > "$PID_FILE"

    # 等待启动
    sleep 3

    # 检查进程
    if ps -p $BACKEND_PID > /dev/null; then
        echo -e "${GREEN}[SUCCESS]${NC} 后端启动成功!"
        echo "  - PID: $BACKEND_PID"
        echo "  - 健康检查: http://$HOST:$PORT/health"
        echo "  - Web UI: http://$HOST:$PORT"
echo ""

        # 测试健康检查
        if curl -s "http://$HOST:$PORT/health" > /dev/null 2>&1; then
            echo -e "${GREEN}[健康检查]${NC} ✅ Backend运行正常"
        else
            echo -e "${YELLOW}[警告]${NC} Backend已启动但健康检查失败，请查看日志"
        fi
    else
        echo -e "${RED}[ERROR]${NC} 后端启动失败，最近 10 行日志:"
        echo "  ----------------------------------------"
        tail -n 10 "$LOG_FILE" 2>/dev/null || echo "  (无日志输出: $LOG_FILE)"
        echo "  ----------------------------------------"
        echo -e "${YELLOW}完整日志:${NC} tail -f $LOG_FILE"
        rm -f "$PID_FILE"
        exit 1
    fi

    echo ""
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}后端已启动${NC}"
    echo -e "${GREEN}========================================${NC}"
    echo ""
}

# 停止服务
stop_service() {
    if ! is_running; then
        echo -e "${YELLOW}[WARNING]${NC} 后端服务未运行"
        return 0
    fi

    local pid=$(get_pid)
    echo -e "${YELLOW}[INFO]${NC} 停止后端服务 (PID: $pid)..."
    
    # 优雅停止
    kill -TERM "$pid" 2>/dev/null || true
    sleep 2
    
    # 检查是否还在运行
    if ps -p "$pid" > /dev/null 2>&1; then
        echo -e "${YELLOW}[INFO]${NC} 强制停止后端服务..."
        kill -KILL "$pid" 2>/dev/null || true
        sleep 1
    fi
    
    # 清理PID文件
    rm -f "$PID_FILE"
    
    if ps -p "$pid" > /dev/null 2>&1; then
        echo -e "${RED}[ERROR]${NC} 无法停止后端服务"
        exit 1
    else
        echo -e "${GREEN}[SUCCESS]${NC} 后端服务已停止"
    fi
}

# 重启服务
restart_service() {
    echo -e "${BLUE}[INFO]${NC} 重启后端服务..."
    stop_service
    sleep 1
    start_service
}

# 查看服务状态
show_status() {
    if is_running; then
        local pid=$(get_pid)
        echo -e "${GREEN}[STATUS]${NC} 后端服务正在运行"
        echo "  - PID: $pid"
        echo "  - 二进制文件: $BINARY_PATH"
        echo "  - 日志文件: $LOG_FILE"
        echo "  - 数据目录: $DATA_DIR"
        echo "  - 健康检查: http://${HOST:-0.0.0.0}:${PORT:-9527}/health"
        echo "  - Web UI: http://${HOST:-0.0.0.0}:${PORT:-9527}"
        
        # 测试健康检查
        if curl -s "http://${HOST:-0.0.0.0}:${PORT:-9527}/health" > /dev/null 2>&1; then
            echo -e "  - 健康状态: ${GREEN}✅ 正常${NC}"
        else
            echo -e "  - 健康状态: ${RED}❌ 异常${NC}"
        fi
    else
        echo -e "${RED}[STATUS]${NC} 后端服务未运行"
    fi
}

# 查看日志
show_logs() {
    if [ ! -f "$LOG_FILE" ]; then
        echo -e "${YELLOW}[WARNING]${NC} 日志文件不存在: $LOG_FILE"
        return 1
    fi
    
    echo -e "${BLUE}[INFO]${NC} 显示实时日志 (按 Ctrl+C 退出)..."
    tail -f "$LOG_FILE"
}

# 主函数
main() {
    case "${1:-start}" in
        start)
            start_service
            ;;
        stop)
            stop_service
            ;;
        restart)
            restart_service
            ;;
        status)
            show_status
            ;;
        logs)
            show_logs
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            echo -e "${RED}[ERROR]${NC} 未知命令: $1"
            echo ""
            show_help
            exit 1
            ;;
    esac
}

# 执行主函数
main "$@"
EOF

chmod +x "$DIST_DIR/bin/stellar.sh"

echo ""
echo -e "${GREEN}✓ Backend build complete!${NC}"
echo -e "  Binary: $DIST_DIR/bin/stellar"
echo -e "  Startup script: $DIST_DIR/bin/stellar.sh"
echo -e "  Config file: $DIST_DIR/conf/config.toml"
echo -e "  Note: Frontend assets are embedded directly from frontend/dist/"
