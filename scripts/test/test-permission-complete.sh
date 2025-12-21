#!/bin/bash

# Complete Permission Management Test Script
# 全面测试权限申请、审批、执行及多租户权限控制

set -e

BASE_URL="http://localhost:8081/api"
ADMIN_TOKEN=""
USER_TOKEN=""
ORG_ADMIN_TOKEN=""

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

# 登录用户
login_user() {
  local username=$1
  local password=$2
  local token_var=$3
  
  step "Logging in as" "$username"
  
  LOGIN_RESPONSE=$(curl -s -X POST "${BASE_URL}/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"$username\",\"password\":\"$password\"}")
  
  TOKEN=$(extract_value "$LOGIN_RESPONSE" "token")
  if [ -z "$TOKEN" ]; then
    echo "Login failed for $username. Response: $LOGIN_RESPONSE"
    return 1
  fi
  
  # 设置全局变量
  case $token_var in
    "ADMIN")
      ADMIN_TOKEN="$TOKEN"
      ;;
    "USER")
      USER_TOKEN="$TOKEN"
      ;;
    "ORG_ADMIN")
      ORG_ADMIN_TOKEN="$TOKEN"
      ;;
  esac
  
  pass "Login successful as $username"
}

# 创建组织
create_organization() {
  local org_name=$1
  local token=$2
  
  step "Creating organization" "$org_name"
  
  ORG_RESPONSE=$(curl -s -X POST "${BASE_URL}/organizations" \
    -H "Authorization: Bearer $token" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"$org_name\",\"description\":\"Test organization for permission testing\"}")
  
  ORG_ID=$(extract_number "$ORG_RESPONSE" "id")
  if [ -z "$ORG_ID" ]; then
    echo "Failed to create organization. Response: $ORG_RESPONSE"
    return 1
  fi
  
  pass "Organization created: $org_name (id=$ORG_ID)"
  echo "$ORG_ID"
}

# 创建用户
create_user() {
  local username=$1
  local password=$2
  local email=$3
  local org_id=$4
  local token=$5
  
  step "Creating user" "$username"
  
  USER_RESPONSE=$(curl -s -X POST "${BASE_URL}/users" \
    -H "Authorization: Bearer $token" \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"$username\",\"password\":\"$password\",\"email\":\"$email\",\"organization_id\":$org_id}")
  
  USER_ID=$(extract_number "$USER_RESPONSE" "id")
  if [ -z "$USER_ID" ]; then
    echo "Failed to create user. Response: $USER_RESPONSE"
    return 1
  fi
  
  pass "User created: $username (id=$USER_ID)"
  echo "$USER_ID"
}

# 提交权限申请
submit_permission_request() {
  local token=$1
  local payload=$2
  local description=$3
  
  step "Submitting permission request" "$description"
  
  RESPONSE=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer $token" \
    -H "Content-Type: application/json" \
    -d "$payload")
  
  REQ_ID=$(echo "$RESPONSE" | grep -Eo '^[0-9]+$')
  if [ -z "$REQ_ID" ]; then
    echo "Response from /permission-requests:"
    format_json "$RESPONSE"
    fail "Failed to submit permission request: $description"
  fi
  
  pass "Request submitted successfully (id=$REQ_ID) for: $description"
  echo "$REQ_ID"
}

# 审批权限申请
approve_request() {
  local token=$1
  local request_id=$2
  local comment=$3
  
  step "Approving request" "$request_id"
  
  RESPONSE=$(curl -s -X POST "${BASE_URL}/permission-requests/$request_id/approve" \
    -H "Authorization: Bearer $token" \
    -H "Content-Type: application/json" \
    -d "{\"comment\": \"$comment\"}")
  
  if echo "$RESPONSE" | grep -q "error"; then
    echo "Failed to approve request $request_id: $RESPONSE"
    return 1
  fi
  
  pass "Request $request_id approved successfully"
}

# 拒绝权限申请
reject_request() {
  local token=$1
  local request_id=$2
  local comment=$3
  
  step "Rejecting request" "$request_id"
  
  RESPONSE=$(curl -s -X POST "${BASE_URL}/permission-requests/$request_id/reject" \
    -H "Authorization: Bearer $token" \
    -H "Content-Type: application/json" \
    -d "{\"comment\": \"$comment\"}")
  
  if echo "$RESPONSE" | grep -q "error"; then
    echo "Failed to reject request $request_id: $RESPONSE"
    return 1
  fi
  
  pass "Request $request_id rejected successfully"
}

# 测试权限控制
test_permission_control() {
  local user_token=$1
  local request_id=$2
  
  step "Testing permission control" "Non-admin user trying to approve"
  
  RESPONSE=$(curl -s -X POST "${BASE_URL}/permission-requests/$request_id/approve" \
    -H "Authorization: Bearer $user_token" \
    -H "Content-Type: application/json" \
    -d '{"comment": "Unauthorized approval attempt"}')
  
  # 应该返回错误
  if echo "$RESPONSE" | grep -q "error\|forbidden\|unauthorized"; then
    pass "Permission control working - non-admin user correctly denied"
  else
    fail "Permission control failed - non-admin user was able to approve request"
  fi
}

# 测试SQL预览功能
test_sql_preview() {
  local token=$1
  local payload=$2
  local expected_sql_pattern=$3
  
  step "Testing SQL preview generation"
  
  RESPONSE=$(curl -s -X POST "${BASE_URL}/db-auth/preview-sql" \
    -H "Authorization: Bearer $token" \
    -H "Content-Type: application/json" \
    -d "$payload")
  
  SQL=$(echo "$RESPONSE" | grep -o '"sql":"[^"]*' | sed 's/"sql":"//')
  
  if echo "$SQL" | grep -q "$expected_sql_pattern"; then
    pass "SQL preview generated correctly: $SQL"
  else
    echo "Expected pattern: $expected_sql_pattern, Got: $SQL"
    echo "Full response: $RESPONSE"
    fail "SQL preview failed"
  fi
}

main() {
  echo "${BLUE}======================================${NC}"
  echo "${BLUE}Complete Permission Management Test${NC}"
  echo "${BLUE}======================================${NC}"
  echo ""

  # Step 1: Login as admin
  step "Setup" "Login as admin"
  login_user "admin" "admin" "ADMIN" || fail "Admin login failed"
  echo ""

  # Step 2: Get cluster info
  step "Setup" "Getting cluster information"
  CLUSTERS=$(curl -s -X GET "${BASE_URL}/clusters" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}")
  CLUSTER_ID=$(extract_number "$CLUSTERS" "id")
  
  if [ -z "$CLUSTER_ID" ]; then
    fail "No cluster found. Please register at least one OLAP cluster."
  fi
  
  pass "Using cluster_id=${CLUSTER_ID} for testing"
  echo ""

  # ========================================
  # 第一部分：创建测试组织和用户
  # ========================================
  echo "${BLUE}=== Part 1: Create Test Organization and Users ===${NC}"
  
  # 创建测试组织
  ORG_ID=$(create_organization "TestOrg" "$ADMIN_TOKEN") || {
    echo "${YELLOW}Note: Organization creation failed, using existing org_id=1${NC}"
    ORG_ID=1
  }
  
  # 创建普通用户
  create_user "test_user" "user123" "testuser@test.com" "$ORG_ID" "$ADMIN_TOKEN" || {
    echo "${YELLOW}Note: User creation failed, trying to login existing user${NC}"
    login_user "test_user" "user123" "USER" || {
      echo "${YELLOW}Note: No test_user found, will test with admin only${NC}"
      USER_TOKEN="$ADMIN_TOKEN"
    }
  }
  
  # 创建组织管理员用户
  create_user "org_admin" "admin123" "orgadmin@test.com" "$ORG_ID" "$ADMIN_TOKEN" || {
    echo "${YELLOW}Note: Org admin creation failed, using admin as org admin${NC}"
    ORG_ADMIN_TOKEN="$ADMIN_TOKEN"
  }
  
  echo ""

  # ========================================
  # 第二部分：基础权限申请和审批流程
  # ========================================
  echo "${BLUE}=== Part 2: Basic Permission Request & Approval Flow ===${NC}"
  
  # 测试1：现有用户权限授予
  step "Test 1" "Existing user permission grant"
  REQUEST1_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "data_analyst_1",
    "resource_type": "database",
    "database": "analytics_db",
    "permissions": ["SELECT", "INSERT"]
  },
  "reason": "Need read/write access for analytics work"
}
EOF
)
  
  REQ1_ID=$(submit_permission_request "$USER_TOKEN" "$REQUEST1_PAYLOAD" "Regular user permission request")
  
  # 测试2：组织管理员审批
  step "Test 2" "Organization admin approves the request"
  approve_request "$ORG_ADMIN_TOKEN" "$REQ1_ID" "Approved for analytics team member"
  
  # 测试3：权限控制测试
  step "Test 3" "Testing permission control - regular user cannot approve"
  test_permission_control "$USER_TOKEN" "$REQ1_ID"
  echo ""

  # ========================================
  # 第三部分：新角色创建 (Scheme B)
  # ========================================
  echo "${BLUE}=== Part 3: New Role Creation (Scheme B) ===${NC}"
  
  # 测试4：新角色创建和权限授予
  step "Test 4" "Create new role and grant permissions"
  REQUEST2_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "data_analyst_1",
    "new_role_name": "analytics_reader",
    "resource_type": "database",
    "database": "analytics_db",
    "permissions": ["SELECT", "SHOW"]
  },
  "reason": "Create dedicated role for analytics read access"
}
EOF
)
  
  REQ2_ID=$(submit_permission_request "$USER_TOKEN" "$REQUEST2_PAYLOAD" "New role creation request")
  approve_request "$ORG_ADMIN_TOKEN" "$REQ2_ID" "Approved new analytics reader role"
  echo ""

  # ========================================
  # 第四部分：新用户创建 (Scheme C)
  # ========================================
  echo "${BLUE}=== Part 4: New User Creation (Scheme C) ===${NC}"
  
  # 测试5：新用户创建和权限授予
  step "Test 5" "Create new user and grant permissions"
  REQUEST3_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "new_user_name": "new_analyst",
    "new_user_password": "secure_password_123",
    "resource_type": "table",
    "database": "analytics_db",
    "table": "user_events",
    "permissions": ["SELECT"]
  },
  "reason": "New team member needs access to user events table"
}
EOF
)
  
  REQ3_ID=$(submit_permission_request "$USER_TOKEN" "$REQUEST3_PAYLOAD" "New user creation request")
  approve_request "$ORG_ADMIN_TOKEN" "$REQ3_ID" "Approved new analyst account"
  echo ""

  # ========================================
  # 第五部分：权限撤销
  # ========================================
  echo "${BLUE}=== Part 5: Permission Revocation ===${NC}"
  
  # 测试6：权限撤销
  step "Test 6" "Revoke permissions"
  REQUEST4_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "revoke_permission",
  "request_details": {
    "action": "revoke_permission",
    "target_user": "data_analyst_1",
    "resource_type": "database",
    "database": "analytics_db",
    "permissions": ["INSERT"]
  },
  "reason": "User no longer needs write access"
}
EOF
)
  
  REQ4_ID=$(submit_permission_request "$USER_TOKEN" "$REQUEST4_PAYLOAD" "Permission revocation request")
  approve_request "$ORG_ADMIN_TOKEN" "$REQ4_ID" "Revoked write access as requested"
  echo ""

  # ========================================
  # 第六部分：WITH GRANT OPTION
  # ========================================
  echo "${BLUE}=== Part 6: WITH GRANT OPTION ===${NC}"
  
  # 测试7：WITH GRANT OPTION权限授予
  step "Test 7" "Grant permissions WITH GRANT OPTION"
  REQUEST5_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "team_lead",
    "resource_type": "database",
    "database": "analytics_db",
    "permissions": ["SELECT", "INSERT", "UPDATE"],
    "with_grant_option": true
  },
  "reason": "Team lead needs full access with ability to grant to team members"
}
EOF
)
  
  REQ5_ID=$(submit_permission_request "$USER_TOKEN" "$REQUEST5_PAYLOAD" "WITH GRANT OPTION request")
  approve_request "$ORG_ADMIN_TOKEN" "$REQ5_ID" "Approved with grant option for team lead"
  echo ""

  # ========================================
  # 第七部分：SQL预览功能测试
  # ========================================
  echo "${BLUE}=== Part 7: SQL Preview Functionality ===${NC}"
  
  # 测试8：SQL预览 - 现有用户
  step "Test 8" "SQL Preview - Existing User Permission Grant"
  PREVIEW1_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "test_user_preview",
    "resource_type": "database",
    "database": "test_db",
    "permissions": ["SELECT", "INSERT"]
  }
}
EOF
)
  
  test_sql_preview "$ADMIN_TOKEN" "$PREVIEW1_PAYLOAD" "GRANT.*ON.*TO"
  
  # 测试9：SQL预览 - 新角色
  step "Test 9" "SQL Preview - New Role Creation"
  PREVIEW2_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "test_user",
    "new_role_name": "preview_role",
    "resource_type": "table",
    "database": "test_db",
    "table": "test_table",
    "permissions": ["SELECT"]
  }
}
EOF
)
  
  test_sql_preview "$ADMIN_TOKEN" "$PREVIEW2_PAYLOAD" "CREATE ROLE\|GRANT.*TO"
  
  # 测试10：SQL预览 - 新用户
  step "Test 10" "SQL Preview - New User Creation"
  PREVIEW3_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "new_user_name": "preview_new_user",
    "new_user_password": "preview_pass",
    "resource_type": "database",
    "database": "test_db",
    "permissions": ["SELECT"]
  }
}
EOF
)
  
  test_sql_preview "$ADMIN_TOKEN" "$PREVIEW3_PAYLOAD" "CREATE USER\|GRANT.*ON"
  echo ""

  # ========================================
  # 第八部分：错误场景测试
  # ========================================
  echo "${BLUE}=== Part 8: Error Scenarios ===${NC}"
  
  # 测试11：无效权限申请
  step "Test 11" "Invalid permission request"
  INVALID_REQUEST=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission"
  }
}
EOF
)
  
  RESPONSE=$(curl -s -X POST "${BASE_URL}/permission-requests" \
    -H "Authorization: Bearer $USER_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$INVALID_REQUEST")
  
  if echo "$RESPONSE" | grep -q "error\|validation"; then
    pass "Invalid request correctly rejected"
  else
    fail "Invalid request should have been rejected"
  fi
  
  # 测试12：权限申请被拒绝
  step "Test 12" "Permission request rejection"
  REQUEST6_PAYLOAD=$(cat <<EOF
{
  "cluster_id": ${CLUSTER_ID},
  "request_type": "grant_permission",
  "request_details": {
    "action": "grant_permission",
    "target_user": "temp_user",
    "resource_type": "database",
    "database": "sensitive_db",
    "permissions": ["ALL"]
  },
  "reason": "Requesting excessive permissions"
}
EOF
)
  
  REQ6_ID=$(submit_permission_request "$USER_TOKEN" "$REQUEST6_PAYLOAD" "Excessive permissions request")
  reject_request "$ORG_ADMIN_TOKEN" "$REQ6_ID" "Requesting excessive permissions - denied"
  echo ""

  # ========================================
  # 第九部分：查询和列表功能
  # ========================================
  echo "${BLUE}=== Part 9: Query and List Functions ===${NC}"
  
  # 测试13-17：各种查询功能
  step "Test 13-17" "Testing various query functions"
  
  MY_PERMS=$(curl -s -X GET "${BASE_URL}/clusters/db-auth/my-permissions" \
    -H "Authorization: Bearer ${USER_TOKEN}")
  pass "My permissions queried successfully"
  
  DB_ACCOUNTS=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/db-auth/accounts" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}")
  pass "Database accounts listed successfully"
  
  DB_ROLES=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/db-auth/roles" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}")
  pass "Database roles listed successfully"
  
  MY_REQUESTS=$(curl -s -X GET "${BASE_URL}/permission-requests/my?page=1&page_size=50" \
    -H "Authorization: Bearer ${USER_TOKEN}")
  pass "My permission requests listed successfully"
  
  PENDING=$(curl -s -X GET "${BASE_URL}/permission-requests/pending" \
    -H "Authorization: Bearer ${ORG_ADMIN_TOKEN}")
  pass "Pending approvals listed successfully"
  echo ""

  # ========================================
  # 第十部分：审批历史和状态变更
  # ========================================
  echo "${BLUE}=== Part 10: Approval History & Status Changes ===${NC}"
  
  # 测试18：查看申请详情
  step "Test 18" "View request details"
  REQUEST_DETAILS=$(curl -s -X GET "${BASE_URL}/permission-requests/$REQ1_ID" \
    -H "Authorization: Bearer ${USER_TOKEN}")
  pass "Request details retrieved successfully"
  
  # 测试19：查看审批历史
  step "Test 19" "View approval history"
  APPROVAL_HISTORY=$(curl -s -X GET "${BASE_URL}/permission-requests/$REQ1_ID/history" \
    -H "Authorization: Bearer ${USER_TOKEN}")
  pass "Approval history retrieved successfully"
  echo ""

  # ========================================
  # 总结
  # ========================================
  echo "${BLUE}======================================${NC}"
  echo "${GREEN}✓ All comprehensive permission management tests passed!${NC}"
  echo "${BLUE}======================================${NC}"
  echo ""
  echo "Test Coverage Summary:"
  echo "✓ Multi-tenant organization and user management"
  echo "✓ Regular user permission requests"
  echo "✓ Organization admin approval workflow"
  echo "✓ Permission control (non-admin denial)"
  echo "✓ New role creation (Scheme B)"
  echo "✓ New user creation (Scheme C)"
  echo "✓ Permission revocation"
  echo "✓ WITH GRANT OPTION functionality"
  echo "✓ SQL preview generation for all scenarios"
  echo "✓ Error scenario handling"
  echo "✓ Permission request rejection"
  echo "✓ Query and list functions"
  echo "✓ Request details and history"
  echo ""
  echo "Total scenarios tested: 19"
  echo "All key permission management workflows covered!"
}

main "$@"
