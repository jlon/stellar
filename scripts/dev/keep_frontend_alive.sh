#!/bin/bash
# Stellar 前端 dev server 保活：4200 不通则重启。crontab：
#   * * * * * /mnt/data/stellar/scripts/dev/keep_frontend_alive.sh >> /tmp/stellar-keepalive.log 2>&1
set -u
PORT="${PORT:-4200}"
WORKDIR="/mnt/data/stellar/frontend"
LOG="/tmp/stellar-frontend.log"

if curl -s -m 5 -o /dev/null "http://127.0.0.1:${PORT}/"; then
  exit 0
fi

echo "[$(date '+%F %T')] frontend unhealthy, restarting..."
cd "$WORKDIR" || exit 1
setsid nohup npm start > "$LOG" 2>&1 &
echo "[$(date '+%F %T')] restart triggered (ng serve takes ~60s to compile)"
