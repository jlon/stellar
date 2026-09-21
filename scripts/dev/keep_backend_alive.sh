#!/bin/bash
# Stellar 后端保活：8081 不通则重启。
#
# 启动一律委托给 scripts/dev/start_backend.sh --dev，避免出现第二套启动参数：
# 早前版本直接 `stellar`（零配置模式）启动 release 二进制，既没有 --config、
# 也没有 STELLAR_ENV/端口覆盖，结果监听 9527 而非 8081，且在 production 语义下
# 会把 legacy admin 口令重置为随机值。
#
# 加到 crontab 每分钟跑一次：
#   * * * * * /mnt/data/stellar/scripts/dev/keep_backend_alive.sh >> /tmp/stellar-keepalive.log 2>&1
set -u
PORT="${PORT:-8081}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
START_SCRIPT="$SCRIPT_DIR/start_backend.sh"
LOG="/tmp/stellar-keepalive.log"

if curl -s -m 5 -o /dev/null "http://127.0.0.1:${PORT}/health"; then
  exit 0
fi

echo "[$(date '+%F %T')] backend unhealthy, restarting via start_backend.sh start --dev"
# 只杀监听该端口的 stellar 进程，避免误伤只发起连接的进程
PIDS=$(ss -tlnp 2>/dev/null | grep ":${PORT} " | grep -oP 'pid=\K[0-9]+' | sort -u)
for pid in $PIDS; do
  if ps -p "$pid" -o comm= 2>/dev/null | grep -q stellar; then
    kill "$pid" 2>/dev/null
  fi
done
sleep 2
"$START_SCRIPT" start --dev >> "$LOG" 2>&1
sleep 5
if curl -s -m 5 -o /dev/null "http://127.0.0.1:${PORT}/health"; then
  echo "[$(date '+%F %T')] restarted OK"
else
  echo "[$(date '+%F %T')] restart FAILED, see $LOG"
  exit 1
fi
