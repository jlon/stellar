#!/bin/bash

# Register two clusters: StarRocks and Doris
# Usage: ./register-clusters.sh

set -e

BASE_URL="http://localhost:8081/api"
TOKEN=""

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

format_json() {
  echo "$1" | python3 -m json.tool 2>/dev/null || echo "$1"
}

extract_value() {
  echo "$1" | grep -o "\"$2\":\"[^\"]*" | sed "s/\"$2\":\"//" | head -1
}

extract_number() {
  echo "$1" | grep -o "\"$2\":[0-9]*" | sed "s/\"$2\"://" | head -1
}

step() {
  echo "${YELLOW}[$1] $2${NC}"
}

pass() {
  echo "${GREEN}✓ $1${NC}"
}

fail() {
  echo "${RED}✗ $1${NC}"
  exit 1
}

main() {
  echo "${BLUE}======================================${NC}"
  echo "${BLUE}Register Clusters${NC}"
  echo "${BLUE}======================================${NC}"
  echo ""

  # Step 1: login as admin
  step "Step 1" "Login as admin/admin"
  LOGIN_RESPONSE=$(curl -s -X POST "${BASE_URL}/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"admin"}')

  TOKEN=$(extract_value "$LOGIN_RESPONSE" "token")
  if [ -z "$TOKEN" ]; then
    fail "Login failed. Is the backend running on ${BASE_URL}?";
  fi
  pass "Login successful"

  # Step 2: Register StarRocks cluster
  step "Step 2" "Register StarRocks cluster: cloud-commons"
  STARROCKS_PAYLOAD=$(cat <<EOF
{
  "name": "cloud-commons",
  "description": "StarRocks cluster",
  "fe_host": "10.212.160.235",
  "fe_http_port": 8030,
  "fe_query_port": 9030,
  "username": "starrocks",
  "password": "MY!vTN5d3la(",
  "enable_ssl": false,
  "connection_timeout": 10,
  "cluster_type": "starrocks",
  "deployment_mode": "shared_nothing"
}
EOF
)

  STARROCKS_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$STARROCKS_PAYLOAD")

  STARROCKS_ID=$(extract_number "$STARROCKS_RESPONSE" "id")
  if [ -z "$STARROCKS_ID" ]; then
    echo "Response from /clusters:"
    format_json "$STARROCKS_RESPONSE"
    fail "Failed to register StarRocks cluster";
  fi
  pass "StarRocks cluster registered (id=${STARROCKS_ID})"

  # Step 3: Register Doris cluster
  step "Step 3" "Register Doris cluster: my-doris"
  DORIS_PAYLOAD=$(cat <<EOF
{
  "name": "my-doris",
  "description": "Doris cluster",
  "fe_host": "10.119.43.216",
  "fe_http_port": 8030,
  "fe_query_port": 9030,
  "username": "root",
  "password": "",
  "enable_ssl": false,
  "connection_timeout": 10,
  "cluster_type": "doris",
  "deployment_mode": "shared_nothing"
}
EOF
)

  DORIS_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$DORIS_PAYLOAD")

  DORIS_ID=$(extract_number "$DORIS_RESPONSE" "id")
  if [ -z "$DORIS_ID" ]; then
    echo "Response from /clusters:"
    format_json "$DORIS_RESPONSE"
    fail "Failed to register Doris cluster";
  fi
  pass "Doris cluster registered (id=${DORIS_ID})"

  # Step 4: List all clusters
  step "Step 4" "List all registered clusters"
  CLUSTERS=$(curl -s -X GET "${BASE_URL}/clusters" \
    -H "Authorization: Bearer ${TOKEN}")
  pass "Clusters listed successfully"
  format_json "$CLUSTERS"

  pass "Cluster registration completed successfully!"
  echo ""
  echo "${GREEN}Registered clusters:${NC}"
  echo "  - StarRocks: cloud-commons (id=${STARROCKS_ID})"
  echo "  - Doris: my-doris (id=${DORIS_ID})"
}

main "$@"

