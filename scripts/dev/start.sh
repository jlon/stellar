#!/bin/bash

#
# Stellar - Development Environment (One-Click Start)
# 开发环境一键启动（同时启动前后端）
#

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$PROJECT_ROOT"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║   Stellar - Dev Environment   ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
echo ""

# Cleanup function
cleanup() {
    echo ""
    echo -e "${YELLOW}Stopping services...${NC}"
    if [ -n "$BACKEND_PID" ]; then
        kill $BACKEND_PID 2>/dev/null || true
        echo -e "${GREEN}✓ Backend stopped${NC}"
    fi
    if [ -n "$FRONTEND_PID" ]; then
        kill $FRONTEND_PID 2>/dev/null || true
        echo -e "${GREEN}✓ Frontend stopped${NC}"
    fi
    exit 0
}

trap cleanup SIGINT SIGTERM

# Check environment
echo -e "${YELLOW}[1/3]${NC} Checking environment..."
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}✗ Rust/Cargo not found. Please install Rust.${NC}"
    exit 1
fi
if ! command -v npm &> /dev/null; then
    echo -e "${RED}✗ Node.js/npm not found. Please install Node.js.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Environment OK${NC}"
echo ""

# Start backend
echo -e "${YELLOW}[2/3]${NC} Starting backend..."
bash scripts/dev/start_backend.sh &
BACKEND_PID=$!
sleep 3

# Check backend health
if curl -s http://0.0.0.0:8081/health > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Backend running (PID: $BACKEND_PID)${NC}"
else
    echo -e "${YELLOW}⚠ Backend starting... (PID: $BACKEND_PID)${NC}"
fi
echo ""

# Start frontend
echo -e "${YELLOW}[3/3]${NC} Starting frontend..."
echo -e "${BLUE}════════════════════════════════════════${NC}"
echo -e ""
echo -e "${GREEN}Development environment started!${NC}"
echo -e ""
echo -e "${BLUE}Access URLs:${NC}"
echo -e "  Frontend: ${GREEN}http://localhost:4200${NC}"
echo -e "  Backend:  ${GREEN}http://0.0.0.0:8081${NC}"
echo -e "  API Docs: ${GREEN}http://0.0.0.0:8081/api-docs${NC}"
echo -e ""
echo -e "${YELLOW}Press Ctrl+C to stop all services${NC}"
echo -e "${BLUE}════════════════════════════════════════${NC}"
echo ""

# Run frontend in foreground
cd frontend
npm start &
FRONTEND_PID=$!

# Wait for frontend
wait $FRONTEND_PID
