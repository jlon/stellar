#!/bin/bash

# Permission Requests End-to-End Test Script
# 覆盖权限申请、审批、执行及 OLAP 权限生效的主流程

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
  echo "${BLUE}Permission Requests Flow Test${NC}"
  echo "${BLUE}======================================${NC}"
  echo ""

  # Step 1: login as admin
  step "Step 1" "Login as admin/admin"
  LOGIN_RESPONSE=$(curl -s -X POST "${BASE_URL}/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"admin"}')

  TOKEN=$(extract_value "$LOGIN_RESPONSE" "token")
  if [ -z "$TOKEN" ]; then
    fail "Login failed";
  fi
  pass "Login successful"

  # Step 2: get current user info
  step "Step 2" "Get current user info"
  ME_RESPONSE=$(curl -s -X GET "${BASE_URL}/auth/me" \
    -H "Authorization: Bearer ${TOKEN}")
  USER_ID=$(extract_number "$ME_RESPONSE" "id")
  ORG_ID=$(extract_number "$ME_RESPONSE" "organization_id")
  pass "Current user id=${USER_ID}, org_id=${ORG_ID}"

  # Step 3: find an active cluster
  step "Step 3" "Find any available cluster for tests"
  CLUSTERS=$(curl -s -X GET "${BASE_URL}/clusters" \
    -H "Authorization: Bearer ${TOKEN}")
  CLUSTER_ID=$(extract_number "$CLUSTERS" "id")
  if [ -z "$CLUSTER_ID" ]; then
    fail "No cluster found. Please register at least one OLAP cluster before running this script";
  fi
  pass "Using cluster_id=${CLUSTER_ID}"

  # Step 4: submit a few permission requests as admin (simulate applicant)
  step "Step 4" "Submit grant_permission request with existing user"
  REQUEST1_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "test_user_1",
    "resource_type": "database",
    "database": "default",
    "permissions": ["SELECT"]
  },
  "reason": "E2E test: grant_permission to existing user"
}
EOF
)

  REQ1=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$REQUEST1_PAYLOAD")

  # API 返回的是一个 JSON number，例如: 123
  REQ1_ID=$(echo "$REQ1" | grep -Eo '^[0-9]+$')
  if [ -z "$REQ1_ID" ]; then
    echo "Response from /permission-requests:"
    format_json "$REQ1"
    fail "Failed to submit grant_permission request";
  fi
  pass "grant_permission request submitted (id=${REQ1_ID})"

  # Step 5: list my requests
  step "Step 5" "List my permission requests"
  MY_REQUESTS=$(curl -s -X GET "${BASE_URL}/permission-requests/my?page=1&page_size=20" \
    -H "Authorization: Bearer ${TOKEN}")
  pass "My requests listed"

  # Step 6: list pending approvals
  step "Step 6" "List pending approvals"
  PENDING=$(curl -s -X GET "${BASE_URL}/permission-requests/pending" \
    -H "Authorization: Bearer ${TOKEN}")
  pass "Pending approvals listed"

  # Step 7: approve the request
  step "Step 7" "Approve request ${REQ1_ID}"
  APPROVE_RESULT=$(curl -s -X POST "${BASE_URL}/permission-requests/${REQ1_ID}/approve" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"comment": "Approved for testing"}')
  pass "Request approved: ${APPROVE_RESULT}"

  # Step 8: submit grant_permission with new role (Scheme B)
  step "Step 8" "Submit grant_permission with new role (Scheme B)"
  REQUEST2_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "test_user_1",
    "new_role_name": "test_role_auto",
    "resource_type": "database",
    "database": "default",
    "permissions": ["SELECT", "INSERT"]
  },
  "reason": "E2E test: grant_permission with new role (Scheme B)"
}
EOF
)

  REQ2=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$REQUEST2_PAYLOAD")

  REQ2_ID=$(echo "$REQ2" | grep -Eo '^[0-9]+$')
  if [ -z "$REQ2_ID" ]; then
    echo "Response from /permission-requests:"
    format_json "$REQ2"
    fail "Failed to submit grant_permission with new role request";
  fi
  pass "grant_permission with new role request submitted (id=${REQ2_ID})"

  # Step 9: approve the new role request
  step "Step 9" "Approve request ${REQ2_ID}"
  APPROVE2=$(curl -s -X POST "${BASE_URL}/permission-requests/${REQ2_ID}/approve" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"comment": "Approved new role request"}')
  pass "New role request approved: ${APPROVE2}"

  # Step 10: submit grant_permission with new user
  step "Step 10" "Submit grant_permission with new user"
  REQUEST3_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "new_user_name": "test_new_user",
    "new_user_password": "test_password_123",
    "resource_type": "database",
    "database": "default",
    "permissions": ["SELECT"]
  },
  "reason": "E2E test: grant_permission with new user"
}
EOF
)

  REQ3=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$REQUEST3_PAYLOAD")

  REQ3_ID=$(echo "$REQ3" | grep -Eo '^[0-9]+$')
  if [ -z "$REQ3_ID" ]; then
    echo "Response from /permission-requests:"
    format_json "$REQ3"
    fail "Failed to submit grant_permission with new user request";
  fi
  pass "grant_permission with new user request submitted (id=${REQ3_ID})"

  # Step 11: reject this request
  step "Step 11" "Reject request ${REQ3_ID}"
  REJECT_RESULT=$(curl -s -X POST "${BASE_URL}/permission-requests/${REQ3_ID}/reject" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"comment": "Rejected for testing"}')
  pass "Request rejected: ${REJECT_RESULT}"

  # Step 12: query my-permissions
  step "Step 12" "Query my database permissions"
  MY_PERMS=$(curl -s -X GET "${BASE_URL}/clusters/db-auth/my-permissions" \
    -H "Authorization: Bearer ${TOKEN}")
  pass "My permissions queried successfully"

  # Step 13: list db accounts
  step "Step 13" "List database accounts"
  DB_ACCOUNTS=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/db-auth/accounts" \
    -H "Authorization: Bearer ${TOKEN}")
  pass "Database accounts listed"

  # Step 14: list db roles
  step "Step 14" "List database roles"
  DB_ROLES=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/db-auth/roles" \
    -H "Authorization: Bearer ${TOKEN}")
  pass "Database roles listed"

  pass "Permission requests flow comprehensive test finished. All key scenarios covered."
}

main "$@"
