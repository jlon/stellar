#!/bin/bash

# Test script to diagnose db-auth permission issues
# Usage: ./test-db-auth-permission.sh

set -e

BASE_URL="http://localhost:8081"

echo "========================================="
echo "Testing DB-Auth Permission Issue"
echo "========================================="

# Step 1: Login as admin
echo ""
echo "Step 1: Logging in as admin..."
LOGIN_RESPONSE=$(curl -s -X POST "${BASE_URL}/api/auth/login" \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"admin"}')

echo "Login Response:"
echo "$LOGIN_RESPONSE" | jq . 2>/dev/null || echo "$LOGIN_RESPONSE"

TOKEN=$(echo "$LOGIN_RESPONSE" | jq -r '.data.token' 2>/dev/null)

if [ -z "$TOKEN" ] || [ "$TOKEN" = "null" ]; then
  echo "❌ Failed to get token"
  exit 1
fi

echo "✅ Token obtained: ${TOKEN:0:20}..."

# Step 2: Get user info
echo ""
echo "Step 2: Getting user info..."
curl -s -X GET "${BASE_URL}/api/auth/me" \
  -H "Authorization: Bearer ${TOKEN}" | jq .

# Step 3: Test permission-requests APIs (should work)
echo ""
echo "Step 3: Testing permission-requests/my API..."
curl -s -X GET "${BASE_URL}/api/permission-requests/my?page=1&page_size=10" \
  -H "Authorization: Bearer ${TOKEN}" | jq '.'

# Step 4: Get active cluster
echo ""
echo "Step 4: Getting active cluster..."
CLUSTER_RESPONSE=$(curl -s -X GET "${BASE_URL}/api/clusters/active" \
  -H "Authorization: Bearer ${TOKEN}")

echo "$CLUSTER_RESPONSE" | jq .

CLUSTER_ID=$(echo "$CLUSTER_RESPONSE" | jq -r '.data.id' 2>/dev/null)
if [ -z "$CLUSTER_ID" ] || [ "$CLUSTER_ID" = "null" ]; then
  echo "❌ Failed to get active cluster"
  exit 1
fi

echo "✅ Cluster ID: $CLUSTER_ID"

# Step 5: Test old db-auth API with cluster ID
echo ""
echo "Step 5: Testing old db-auth API (with cluster ID)..."
echo "Request: GET /api/clusters/${CLUSTER_ID}/db-auth/accounts"
curl -s -X GET "${BASE_URL}/api/clusters/${CLUSTER_ID}/db-auth/accounts" \
  -H "Authorization: Bearer ${TOKEN}" \
  -w "\nHTTP Status: %{http_code}\n" | jq . 2>/dev/null || echo "(error parsing JSON - likely HTML error response)"

# Step 6: Test new db-auth API without cluster ID
echo ""
echo "Step 6: Testing new db-auth API (without cluster ID)..."
echo "Request: GET /api/clusters/db-auth/accounts"
curl -s -X GET "${BASE_URL}/api/clusters/db-auth/accounts" \
  -H "Authorization: Bearer ${TOKEN}" \
  -w "\nHTTP Status: %{http_code}\n" | jq . 2>/dev/null || echo "(error parsing JSON - likely HTML error response)"

# Step 7: Test db-auth roles
echo ""
echo "Step 7: Testing db-auth roles API..."
echo "Request: GET /api/clusters/${CLUSTER_ID}/db-auth/roles"
curl -s -X GET "${BASE_URL}/api/clusters/${CLUSTER_ID}/db-auth/roles" \
  -H "Authorization: Bearer ${TOKEN}" \
  -w "\nHTTP Status: %{http_code}\n" | jq . 2>/dev/null || echo "(error parsing JSON - likely HTML error response)"

echo ""
echo "========================================="
echo "Test Complete"
echo "========================================="
