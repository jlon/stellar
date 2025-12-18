#!/bin/bash

#
# Stellar - Production Service Management Script
# 生产环境服务管理脚本
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BINARY_NAME="stellar"
BINARY_PATH="$DIST_ROOT/bin/$BINARY_NAME"
CONFIG_PATH="$DIST_ROOT/conf/config.toml"
PID_FILE="$DIST_ROOT/stellar.pid"
LOG_FILE="$DIST_ROOT/logs/stellar.log"

# Ensure required directories exist
mkdir -p "$DIST_ROOT/logs"
mkdir -p "$DIST_ROOT/data"

# Function to check if service is running
is_running() {
    if [ -f "$PID_FILE" ]; then
        local pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            return 0
        else
            # PID file exists but process is dead
            rm -f "$PID_FILE"
            return 1
        fi
    fi
    return 1
}

# Function to start the service
start_service() {
    if is_running; then
        echo -e "${YELLOW}Service is already running (PID: $(cat $PID_FILE))${NC}"
        return 0
    fi

    if [ ! -f "$BINARY_PATH" ]; then
        echo -e "${RED}Error: Binary not found at $BINARY_PATH${NC}"
        echo "Please run 'make build' first"
        exit 1
    fi

    if [ ! -f "$CONFIG_PATH" ]; then
        echo -e "${YELLOW}Warning: Config file not found at $CONFIG_PATH${NC}"
        echo "Using default configuration"
    fi

    echo -e "${BLUE}Starting Stellar...${NC}"
    
    # Start the service in background
    cd "$DIST_ROOT"
    nohup "$BINARY_PATH" > "$LOG_FILE" 2>&1 &
    local pid=$!
    
    # Save PID
    echo $pid > "$PID_FILE"
    
    # Wait a moment and check if it's still running
    sleep 2
    if is_running; then
        echo -e "${GREEN}✓ Service started successfully (PID: $pid)${NC}"
        echo -e "${BLUE}Logs: $LOG_FILE${NC}"
        echo -e "${BLUE}Config: $CONFIG_PATH${NC}"
        echo ""
        echo -e "${GREEN}Access URLs:${NC}"
        echo -e "  Web UI: ${GREEN}http://localhost:8080${NC}"
        echo -e "  API Docs: ${GREEN}http://localhost:8080/api-docs${NC}"
        echo -e "  Health: ${GREEN}http://localhost:8080/health${NC}"
    else
        echo -e "${RED}✗ Failed to start service${NC}"
        echo -e "${YELLOW}Check logs: $LOG_FILE${NC}"
        rm -f "$PID_FILE"
        exit 1
    fi
}

# Function to stop the service
stop_service() {
    if ! is_running; then
        echo -e "${YELLOW}Service is not running${NC}"
        return 0
    fi

    local pid=$(cat "$PID_FILE")
    echo -e "${BLUE}Stopping Stellar (PID: $pid)...${NC}"
    
    # Send SIGTERM for graceful shutdown
    kill -TERM "$pid" 2>/dev/null || true
    
    # Wait for graceful shutdown (max 10 seconds)
    local count=0
    while [ $count -lt 10 ]; do
        if ! kill -0 "$pid" 2>/dev/null; then
            break
        fi
        sleep 1
        count=$((count + 1))
    done
    
    # Force kill if still running
    if kill -0 "$pid" 2>/dev/null; then
        echo -e "${YELLOW}Force stopping service...${NC}"
        kill -KILL "$pid" 2>/dev/null || true
    fi
    
    rm -f "$PID_FILE"
    echo -e "${GREEN}✓ Service stopped${NC}"
}

# Function to restart the service
restart_service() {
    echo -e "${BLUE}Restarting Stellar...${NC}"
    stop_service
    sleep 1
    start_service
}

# Function to show service status
show_status() {
    if is_running; then
        local pid=$(cat "$PID_FILE")
        echo -e "${GREEN}✓ Service is running (PID: $pid)${NC}"
        echo -e "${BLUE}Config: $CONFIG_PATH${NC}"
        echo -e "${BLUE}Logs: $LOG_FILE${NC}"
        echo -e "${BLUE}Web UI: http://localhost:8080${NC}"
        
        # Show recent log entries
        if [ -f "$LOG_FILE" ]; then
            echo ""
            echo -e "${BLUE}Recent logs:${NC}"
            tail -n 5 "$LOG_FILE" | sed 's/^/  /'
        fi
    else
        echo -e "${RED}✗ Service is not running${NC}"
        if [ -f "$LOG_FILE" ]; then
            echo -e "${YELLOW}Last log entries:${NC}"
            tail -n 5 "$LOG_FILE" | sed 's/^/  /'
        fi
    fi
}

# Function to show logs
show_logs() {
    if [ -f "$LOG_FILE" ]; then
        tail -f "$LOG_FILE"
    else
        echo -e "${YELLOW}Log file not found: $LOG_FILE${NC}"
        exit 1
    fi
}

# Main script logic
case "${1:-}" in
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
    *)
        echo -e "${BLUE}Stellar Service Manager${NC}"
        echo ""
        echo "Usage: $0 {start|stop|restart|status|logs}"
        echo ""
        echo "Commands:"
        echo "  start   - Start the service"
        echo "  stop    - Stop the service"
        echo "  restart - Restart the service"
        echo "  status  - Show service status"
        echo "  logs    - Show live logs"
        echo ""
        echo "Files:"
        echo "  Binary: $BINARY_PATH"
        echo "  Config: $CONFIG_PATH"
        echo "  Logs:   $LOG_FILE"
        echo "  PID:    $PID_FILE"
        exit 1
        ;;
esac
