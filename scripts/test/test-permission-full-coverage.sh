#!/bin/bash

# Permission Management Full Coverage Test Script
# ================================================
# 
# 测试场景说明：
# 1. 用户申请 OLAP 数据库权限
# 2. 管理员审批权限申请
# 3. 用户查看自己申请的权限状态
# 4. 验证权限申请的各种类型（grant_role, grant_permission, revoke_permission）
#
# 重要概念区分：
# - Stellar 系统用户：登录 Stellar 平台的用户（存储在 Stellar SQLite 数据库）
# - OLAP 数据库账户：StarRocks/Doris 数据库中的账户（存储在 OLAP 引擎中）
# - 本功能管理的是 OLAP 数据库账户的权限，不是 Stellar 系统用户
#
# 注意：
# - target_user 字段是 OLAP 数据库账户名（如 olap_analyst），不是 Stellar 用户
# - 这些账户存在于 StarRocks/Doris 数据库中，不会出现在 Stellar 系统管理-用户管理中

set -e

BASE_URL="http://localhost:8081/api"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Token
TOKEN=""

# Test counters
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

format_json() {
  echo "$1" | python3 -m json.tool 2>/dev/null || echo "$1"
}

extract_value() {
  echo "$1" | grep -o "\"$2\":\"[^\"]*" | sed "s/\"$2\":\"//" | head -1
}

extract_number() {
  echo "$1" | grep -o "\"$2\":[0-9]*" | sed "s/\"$2\"://" | head -1
}

section() {
  echo ""
  echo "${CYAN}========================================${NC}"
  echo "${CYAN}$1${NC}"
  echo "${CYAN}========================================${NC}"
}

step() {
  echo "${YELLOW}[$1] $2${NC}"
}

pass() {
  PASSED_TESTS=$((PASSED_TESTS + 1))
  TOTAL_TESTS=$((TOTAL_TESTS + 1))
  echo "${GREEN}✓ PASS: $1${NC}"
}

fail() {
  FAILED_TESTS=$((FAILED_TESTS + 1))
  TOTAL_TESTS=$((TOTAL_TESTS + 1))
  echo "${RED}✗ FAIL: $1${NC}"
  if [ -n "$2" ]; then
    echo "${RED}  Details: $2${NC}"
  fi
}

check_response() {
  local response="$1"
  local expected="$2"
  local test_name="$3"
  
  if echo "$response" | grep -q "$expected"; then
    pass "$test_name"
    return 0
  else
    fail "$test_name" "Expected '$expected' in response"
    echo "  Response: $response"
    return 1
  fi
}

check_not_empty() {
  local value="$1"
  local test_name="$2"
  
  if [ -n "$value" ] && [ "$value" != "null" ]; then
    pass "$test_name"
    return 0
  else
    fail "$test_name" "Value is empty or null"
    return 1
  fi
}

main() {
  echo "${BLUE}======================================${NC}"
  echo "${BLUE}Permission Management Full Coverage Test${NC}"
  echo "${BLUE}======================================${NC}"
  echo ""
  echo "测试目标: OLAP 数据库权限管理（非 Stellar 系统用户管理）"
  echo "测试地址: $BASE_URL"
  echo ""
  echo "${CYAN}重要说明:${NC}"
  echo "- target_user 是 OLAP 数据库账户（如 olap_analyst）"
  echo "- 这些账户存在于 StarRocks/Doris 中，不是 Stellar 系统用户"
  echo "- 不会出现在 Stellar 系统管理-用户管理中"
  echo ""

  # ============================================
  section "1. 用户登录"
  # ============================================

  step "1.1" "Admin 用户登录"
  LOGIN_RESPONSE=$(curl -s -X POST "${BASE_URL}/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"admin"}')

  TOKEN=$(extract_value "$LOGIN_RESPONSE" "token")
  check_not_empty "$TOKEN" "登录成功获取 token"

  # 获取用户信息
  step "1.2" "获取用户信息"
  USER_INFO=$(curl -s -X GET "${BASE_URL}/auth/me" \
    -H "Authorization: Bearer ${TOKEN}")
  
  USER_ID=$(extract_number "$USER_INFO" "id")
  check_not_empty "$USER_ID" "获取用户 ID (id=${USER_ID})"

  # ============================================
  section "2. 获取集群信息"
  # ============================================

  step "2.1" "获取可用集群"
  CLUSTERS=$(curl -s -X GET "${BASE_URL}/clusters" \
    -H "Authorization: Bearer ${TOKEN}")
  
  CLUSTER_ID=$(extract_number "$CLUSTERS" "id")
  check_not_empty "$CLUSTER_ID" "获取可用集群 ID (cluster_id=${CLUSTER_ID})"

  # ============================================
  section "3. 查询 OLAP 数据库信息"
  # ============================================

  step "3.1" "查询 OLAP 数据库账户列表"
  DB_ACCOUNTS=$(curl -s -X GET "${BASE_URL}/clusters/db-auth/accounts" \
    -H "Authorization: Bearer ${TOKEN}")
  
  if echo "$DB_ACCOUNTS" | grep -qE '^\[|account_name|name'; then
    pass "查询 OLAP 数据库账户列表"
    echo "  说明: 这些是 StarRocks/Doris 中的数据库账户，不是 Stellar 系统用户"
  else
    fail "查询 OLAP 数据库账户列表" "Response: $DB_ACCOUNTS"
  fi

  step "3.2" "查询 OLAP 数据库角色列表"
  DB_ROLES=$(curl -s -X GET "${BASE_URL}/clusters/db-auth/roles" \
    -H "Authorization: Bearer ${TOKEN}")
  
  if echo "$DB_ROLES" | grep -qE '^\[|role_name|name'; then
    pass "查询 OLAP 数据库角色列表"
    echo "  说明: 这些是 StarRocks/Doris 中的数据库角色"
  else
    fail "查询 OLAP 数据库角色列表" "Response: $DB_ROLES"
  fi

  step "3.3" "查询我的 OLAP 数据库权限"
  MY_PERMS=$(curl -s -X GET "${BASE_URL}/clusters/db-auth/my-permissions" \
    -H "Authorization: Bearer ${TOKEN}")
  
  if echo "$MY_PERMS" | grep -qE '^\[|privilege_type'; then
    pass "查询我的 OLAP 数据库权限"
  else
    fail "查询我的 OLAP 数据库权限" "Response: $MY_PERMS"
  fi

  # ============================================
  section "4. 提交权限申请（为 OLAP 数据库账户申请权限）"
  # ============================================

  # 4.1 Submit grant_permission request
  step "4.1" "提交 grant_permission 申请（为 OLAP 账户 olap_analyst 申请权限）"
  REQUEST1_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "olap_analyst",
    "resource_type": "database",
    "database": "default",
    "permissions": ["SELECT", "INSERT"]
  },
  "reason": "需要查询和导入数据到 default 数据库进行数据分析"
}
EOF
)

  REQ1=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$REQUEST1_PAYLOAD")

  REQ1_ID=$(echo "$REQ1" | grep -Eo '^[0-9]+$')
  check_not_empty "$REQ1_ID" "提交 grant_permission 申请成功 (id=${REQ1_ID})"
  echo "  说明: target_user=olap_analyst 是 OLAP 数据库账户，不是 Stellar 用户"

  # 4.2 Submit grant_role request
  step "4.2" "提交 grant_role 申请（为 OLAP 账户授予角色）"
  REQUEST2_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_role",
  "request_details": {
    "action": "grant_role",
    "target_user": "olap_analyst",
    "target_role": "db_admin"
  },
  "reason": "需要 db_admin 角色来管理数据库对象"
}
EOF
)

  REQ2=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$REQUEST2_PAYLOAD")

  REQ2_ID=$(echo "$REQ2" | grep -Eo '^[0-9]+$')
  check_not_empty "$REQ2_ID" "提交 grant_role 申请成功 (id=${REQ2_ID})"

  # 4.3 Submit revoke_permission request
  step "4.3" "提交 revoke_permission 申请（撤销 OLAP 账户权限）"
  REQUEST3_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "revoke_permission",
  "request_details": {
    "action": "revoke_permission",
    "target_user": "olap_analyst",
    "resource_type": "database",
    "database": "default",
    "permissions": ["INSERT"]
  },
  "reason": "不再需要 INSERT 权限，申请撤销"
}
EOF
)

  REQ3=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$REQUEST3_PAYLOAD")

  REQ3_ID=$(echo "$REQ3" | grep -Eo '^[0-9]+$')
  check_not_empty "$REQ3_ID" "提交 revoke_permission 申请成功 (id=${REQ3_ID})"

  # ============================================
  section "5. 查看我的申请列表"
  # ============================================

  step "5.1" "查看我的申请列表"
  MY_REQUESTS=$(curl -s -X GET "${BASE_URL}/permission-requests/my?page=1&page_size=20" \
    -H "Authorization: Bearer ${TOKEN}")
  
  if echo "$MY_REQUESTS" | grep -q "\"id\":${REQ1_ID}"; then
    pass "能看到自己提交的申请"
  else
    fail "能看到自己提交的申请" "Request ID ${REQ1_ID} not found"
  fi

  step "5.2" "验证申请状态为 pending"
  if echo "$MY_REQUESTS" | grep -q '"status":"pending"'; then
    pass "申请状态为 pending，等待审批"
  else
    fail "申请状态为 pending"
  fi

  # ============================================
  section "6. 查看待审批列表"
  # ============================================

  step "6.1" "获取待审批申请列表"
  PENDING=$(curl -s -X GET "${BASE_URL}/permission-requests/pending" \
    -H "Authorization: Bearer ${TOKEN}")
  
  if echo "$PENDING" | grep -q "\"id\":${REQ1_ID}"; then
    pass "能看到待审批申请"
    PENDING_COUNT=$(echo "$PENDING" | grep -o '"id"' | wc -l)
    echo "  待审批申请数量: ${PENDING_COUNT}"
  else
    fail "能看到待审批申请"
  fi

  # ============================================
  section "7. 审批权限申请"
  # ============================================

  # 7.1 Approve grant_permission request
  step "7.1" "批准 grant_permission 申请 (id=${REQ1_ID})"
  APPROVE1=$(curl -s -X POST "${BASE_URL}/permission-requests/${REQ1_ID}/approve" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"comment": "批准，符合业务需求"}')
  
  check_response "$APPROVE1" "approved" "批准 grant_permission 申请"

  # 7.2 Approve grant_role request
  step "7.2" "批准 grant_role 申请 (id=${REQ2_ID})"
  APPROVE2=$(curl -s -X POST "${BASE_URL}/permission-requests/${REQ2_ID}/approve" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"comment": "批准角色授予"}')
  
  check_response "$APPROVE2" "approved" "批准 grant_role 申请"

  # 7.3 Reject revoke_permission request
  step "7.3" "拒绝 revoke_permission 申请 (id=${REQ3_ID})"
  REJECT1=$(curl -s -X POST "${BASE_URL}/permission-requests/${REQ3_ID}/reject" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"comment": "拒绝，该权限仍然需要用于日常工作"}')
  
  check_response "$REJECT1" "rejected" "拒绝 revoke_permission 申请"

  # ============================================
  section "8. 验证审批结果"
  # ============================================

  step "8.1" "查看申请状态变更"
  MY_REQUESTS_AFTER=$(curl -s -X GET "${BASE_URL}/permission-requests/my?page=1&page_size=50" \
    -H "Authorization: Bearer ${TOKEN}")
  
  # Check for approved status
  if echo "$MY_REQUESTS_AFTER" | grep -q '"status":"approved"\|"status":"completed"'; then
    pass "能看到已批准的申请"
  else
    fail "能看到已批准的申请"
  fi

  step "8.2" "能看到已拒绝的申请"
  if echo "$MY_REQUESTS_AFTER" | grep -q '"status":"rejected"'; then
    pass "能看到已拒绝的申请"
  else
    fail "能看到已拒绝的申请"
  fi

  # ============================================
  section "9. 验证待审批列表已更新"
  # ============================================

  step "9.1" "再次查看待审批列表"
  PENDING_AFTER=$(curl -s -X GET "${BASE_URL}/permission-requests/pending" \
    -H "Authorization: Bearer ${TOKEN}")
  
  # 之前提交的3个申请都已处理，不应该再出现在待审批列表
  if echo "$PENDING_AFTER" | grep -q "\"id\":${REQ1_ID}"; then
    fail "已处理的申请不应出现在待审批列表"
  else
    pass "已处理的申请已从待审批列表移除"
  fi

  # ============================================
  section "10. 测试结果汇总"
  # ============================================

  echo ""
  echo "${BLUE}======================================${NC}"
  echo "${BLUE}测试结果汇总${NC}"
  echo "${BLUE}======================================${NC}"
  echo ""
  echo "总测试数: ${TOTAL_TESTS}"
  echo "${GREEN}通过: ${PASSED_TESTS}${NC}"
  echo "${RED}失败: ${FAILED_TESTS}${NC}"
  echo ""
  echo "${CYAN}重要说明:${NC}"
  echo "- 本测试验证的是 OLAP 数据库权限管理功能"
  echo "- target_user (如 olap_analyst) 是 StarRocks/Doris 数据库账户"
  echo "- 不是 Stellar 系统用户，不会出现在系统管理-用户管理中"
  echo "- 审批后的权限会在 OLAP 数据库中生效（执行 GRANT/REVOKE SQL）"
  echo ""

  if [ $FAILED_TESTS -eq 0 ]; then
    echo "${GREEN}✓ 所有测试通过！${NC}"
    exit 0
  else
    echo "${RED}✗ 有 ${FAILED_TESTS} 个测试失败${NC}"
    exit 1
  fi
}

main "$@"
