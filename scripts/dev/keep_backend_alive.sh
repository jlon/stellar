#!/bin/bash
# Stellar 后端保活：8081 不通则重启。加到 crontab 每分钟跑一次：
#   * * * * * /mnt/data/stellar/scripts/dev/keep_backend_alive.sh >> /tmp/stellar-keepalive.log 2>&1
set -u
PORT="${PORT:-8081}"
BIN="/home/oppo/targets/release/stellar"
WORKDIR="/mnt/data/stellar/backend"
LOG="/tmp/stellar-8081.log"

if curl -s -m 5 -o /dev/null "http://127.0.0.1:${PORT}/health"; then
  exit 0
fi

echo "[$(date '+%F %T')] backend unhealthy, restarting..."
# 只杀监听该端口的 stellar 进程，避免误伤
PIDS=$(ss -tlnp 2>/dev/null | grep ":${PORT} " | grep -oP 'pid=\K[0-9]+' | sort -u)
for pid in $PIDS; do
  if ps -p "$pid" -o comm= 2>/dev/null | grep -q stellar; then
    kill "$pid" 2>/dev/null
  fi
done
sleep 2
cd "$WORKDIR" || exit 1
setsid nohup "$BIN" >> "$LOG" 2>&1 &
sleep 5
if curl -s -m 5 -o /dev/null "http://127.0.0.1:${PORT}/health"; then
  echo "[$(date '+%F %T')] restarted OK"
else
  echo "[$(date '+%F %T')] restart FAILED, see $LOG"
  exit 1
fi
