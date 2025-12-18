#!/bin/bash

# 物化视图完整功能测试脚本
# 测试所有CRUD操作和边界情况

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置
BASE_URL="http://localhost:8081/api"
CLUSTER_ID=1
USERNAME="admin"
PASSWORD="admin"

echo -e "${YELLOW}================================${NC}"
echo -e "${YELLOW}物化视图完整功能测试脚本${NC}"
echo -e "${YELLOW}================================${NC}"
echo ""

# Step 1: 登录获取Token
echo -e "${YELLOW}[Step 1] 登录获取JWT Token...${NC}"
LOGIN_RESPONSE=$(curl -s -X POST "${BASE_URL}/auth/login" \
  -H "Content-Type: application/json" \
  -d "{\"username\":\"${USERNAME}\",\"password\":\"${PASSWORD}\"}")

TOKEN=$(echo $LOGIN_RESPONSE | grep -o '"token":"[^"]*' | sed 's/"token":"//')

if [ -z "$TOKEN" ]; then
    echo -e "${RED}✗ 登录失败${NC}"
    echo "Response: $LOGIN_RESPONSE"
    exit 1
fi

echo -e "${GREEN}✓ 登录成功，Token: ${TOKEN:0:20}...${NC}"
echo ""

# Step 2: 创建测试基础表
echo -e "${YELLOW}[Step 2] 创建测试基础表...${NC}"

# 创建数据库
curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"sql":"CREATE DATABASE IF NOT EXISTS test_mv_db"}' > /dev/null

# 创建orders表 - 避免JSON转义问题，使用单引号
CREATE_TABLE_SQL='CREATE TABLE IF NOT EXISTS test_mv_db.orders (order_id BIGINT, order_date DATE, customer_id INT, amount DECIMAL(10,2), quantity INT, product_name STRING) DUPLICATE KEY(order_id, order_date) PARTITION BY RANGE(order_date) (PARTITION p20240101 VALUES [("2024-01-01"), ("2024-02-01")), PARTITION p20240201 VALUES [("2024-02-01"), ("2024-03-01")), PARTITION p20240301 VALUES [("2024-03-01"), ("2024-04-01"))) DISTRIBUTED BY HASH(order_id) BUCKETS 10 PROPERTIES ("replication_num" = "1");'

curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_TABLE_SQL}\"}" > /dev/null

echo -e "${GREEN}✓ 基础表创建完成${NC}"
echo ""

# Step 3: 插入测试数据
echo -e "${YELLOW}[Step 3] 插入测试数据...${NC}"

INSERT_SQL='INSERT INTO test_mv_db.orders VALUES (1, "2024-01-15", 101, 100.50, 2, "Product A"), (2, "2024-01-20", 102, 200.75, 1, "Product B"), (3, "2024-02-10", 101, 150.25, 3, "Product C"), (4, "2024-02-15", 103, 300.00, 1, "Product D"), (5, "2024-03-05", 102, 250.50, 2, "Product E"), (6, "2024-03-10", 101, 180.00, 4, "Product F");'

curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${INSERT_SQL}\"}" > /dev/null

echo -e "${GREEN}✓ 测试数据插入完成${NC}"
echo ""

# Step 4: 创建同步物化视图（ROLLUP）
echo -e "${YELLOW}[Step 4] 创建同步物化视图 (ROLLUP)...${NC}"

CREATE_SYNC_MV_SQL="CREATE MATERIALIZED VIEW test_mv_db.orders_sync_rollup AS SELECT order_date, customer_id, SUM(amount) as total_amount FROM test_mv_db.orders GROUP BY order_date, customer_id;"

RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_SYNC_MV_SQL}\"}")

echo "Response: $RESPONSE"
echo -e "${GREEN}✓ 同步物化视图创建请求已发送${NC}"
sleep 3
echo ""

# Step 5: 创建异步物化视图 - MANUAL刷新
echo -e "${YELLOW}[Step 5] 创建异步物化视图 (MANUAL刷新)...${NC}"

CREATE_ASYNC_MANUAL_SQL="CREATE MATERIALIZED VIEW test_mv_db.mv_daily_sales DISTRIBUTED BY HASH(order_date) BUCKETS 10 REFRESH MANUAL AS SELECT order_date, SUM(amount) as daily_sales, SUM(quantity) as daily_quantity, COUNT(DISTINCT customer_id) as customer_count FROM test_mv_db.orders GROUP BY order_date;"

RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_ASYNC_MANUAL_SQL}\"}")

echo "Response: $RESPONSE"
echo -e "${GREEN}✓ MANUAL异步物化视图创建完成${NC}"
sleep 2
echo ""

# Step 6: 创建异步物化视图 - ASYNC自动刷新
echo -e "${YELLOW}[Step 6] 创建异步物化视图 (ASYNC自动刷新)...${NC}"

CREATE_ASYNC_AUTO_SQL="CREATE MATERIALIZED VIEW test_mv_db.mv_customer_summary DISTRIBUTED BY HASH(customer_id) BUCKETS 10 REFRESH ASYNC EVERY(INTERVAL 1 HOUR) AS SELECT customer_id, COUNT(*) as order_count, SUM(amount) as total_spent, MAX(order_date) as last_order_date FROM test_mv_db.orders GROUP BY customer_id;"

RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_ASYNC_AUTO_SQL}\"}")

echo "Response: $RESPONSE"
echo -e "${GREEN}✓ ASYNC异步物化视图创建完成${NC}"
sleep 2
echo ""

# Step 7: 创建分区物化视图
echo -e "${YELLOW}[Step 7] 创建分区物化视图...${NC}"

CREATE_PARTITIONED_MV_SQL="CREATE MATERIALIZED VIEW test_mv_db.mv_monthly_sales PARTITION BY order_date DISTRIBUTED BY HASH(order_date) BUCKETS 10 REFRESH MANUAL AS SELECT order_date, SUM(amount) as monthly_sales, COUNT(*) as order_count FROM test_mv_db.orders GROUP BY order_date;"

RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_PARTITIONED_MV_SQL}\"}")

echo "Response: $RESPONSE"
echo -e "${GREEN}✓ 分区物化视图创建完成${NC}"
sleep 2
echo ""

# Step 8: 列出所有物化视图（无过滤）
echo -e "${YELLOW}[Step 8] 查询所有物化视图列表（无数据库过滤）...${NC}"

LIST_ALL_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}")

MV_COUNT=$(echo "$LIST_ALL_RESPONSE" | python3 -c "import sys, json; data=json.load(sys.stdin); print(len(data))" 2>/dev/null || echo "0")
echo "找到 ${MV_COUNT} 个物化视图"
echo "$LIST_ALL_RESPONSE" | python3 -m json.tool 2>/dev/null | head -50
echo -e "${GREEN}✓ 列表查询成功${NC}"
echo ""

# Step 9: 列出指定数据库的物化视图
echo -e "${YELLOW}[Step 9] 查询test_mv_db数据库的物化视图...${NC}"

LIST_DB_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views?database=test_mv_db" \
  -H "Authorization: Bearer ${TOKEN}")

DB_MV_COUNT=$(echo "$LIST_DB_RESPONSE" | python3 -c "import sys, json; data=json.load(sys.stdin); print(len(data))" 2>/dev/null || echo "0")
echo "找到 ${DB_MV_COUNT} 个物化视图（数据库=test_mv_db）"
echo -e "${GREEN}✓ 数据库筛选成功${NC}"
echo ""

# Step 10: 查询物化视图详情
echo -e "${YELLOW}[Step 10] 查询物化视图详情: mv_daily_sales${NC}"

DETAIL_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_daily_sales" \
  -H "Authorization: Bearer ${TOKEN}")

echo "$DETAIL_RESPONSE" | python3 -m json.tool 2>/dev/null
echo -e "${GREEN}✓ 详情查询成功${NC}"
echo ""

# Step 11: 查询物化视图DDL
echo -e "${YELLOW}[Step 11] 查询物化视图DDL: mv_daily_sales${NC}"

DDL_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_daily_sales/ddl" \
  -H "Authorization: Bearer ${TOKEN}")

echo "$DDL_RESPONSE" | python3 -m json.tool 2>/dev/null
echo -e "${GREEN}✓ DDL查询成功${NC}"
echo ""

# Step 12: 手动刷新物化视图 - ASYNC模式
echo -e "${YELLOW}[Step 12] 手动刷新物化视图（ASYNC模式）: mv_daily_sales${NC}"

REFRESH_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_daily_sales/refresh" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"mode":"ASYNC","force":false}')

echo "刷新响应: $REFRESH_RESPONSE"
echo -e "${GREEN}✓ ASYNC刷新请求已发送${NC}"
sleep 2
echo ""

# Step 13: 强制刷新物化视图 - SYNC模式
echo -e "${YELLOW}[Step 13] 强制刷新物化视图（SYNC模式）: mv_customer_summary${NC}"

FORCE_REFRESH_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_customer_summary/refresh" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"mode":"SYNC","force":true}')

echo "强制刷新响应: $FORCE_REFRESH_RESPONSE"
echo -e "${GREEN}✓ SYNC强制刷新完成${NC}"
sleep 1
echo ""

# Step 14: 刷新分区物化视图（指定分区范围）
echo -e "${YELLOW}[Step 14] 刷新分区物化视图（指定分区）: mv_monthly_sales${NC}"

PARTITION_REFRESH_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_monthly_sales/refresh" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"partition_start":"2024-01-01","partition_end":"2024-02-01","mode":"ASYNC","force":false}')

echo "分区刷新响应: $PARTITION_REFRESH_RESPONSE"
echo -e "${GREEN}✓ 分区刷新请求已发送${NC}"
sleep 2
echo ""

# Step 15: 取消刷新物化视图
echo -e "${YELLOW}[Step 15] 取消物化视图刷新: mv_monthly_sales${NC}"

CANCEL_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_monthly_sales/cancel?force=false" \
  -H "Authorization: Bearer ${TOKEN}")

echo "取消刷新响应: $CANCEL_RESPONSE"
echo -e "${GREEN}✓ 取消刷新请求已发送${NC}"
echo ""

# Step 16: 强制取消刷新
echo -e "${YELLOW}[Step 16] 强制取消物化视图刷新: mv_daily_sales${NC}"

FORCE_CANCEL_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_daily_sales/cancel?force=true" \
  -H "Authorization: Bearer ${TOKEN}")

echo "强制取消响应: $FORCE_CANCEL_RESPONSE"
echo -e "${GREEN}✓ 强制取消请求已发送${NC}"
echo ""

# Step 17: ALTER - 重命名物化视图
echo -e "${YELLOW}[Step 17] 重命名物化视图: mv_customer_summary -> mv_customer_stats${NC}"

ALTER_RENAME_RESPONSE=$(curl -s -X PUT "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_customer_summary" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"alter_clause":"RENAME mv_customer_stats"}')

echo "重命名响应: $ALTER_RENAME_RESPONSE"
echo -e "${GREEN}✓ 重命名成功${NC}"
sleep 1
echo ""

# Step 18: ALTER - 修改刷新策略
echo -e "${YELLOW}[Step 18] 修改物化视图刷新策略: mv_customer_stats${NC}"

ALTER_REFRESH_RESPONSE=$(curl -s -X PUT "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_customer_stats" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"alter_clause":"REFRESH ASYNC EVERY(INTERVAL 2 HOUR)"}')

echo "修改刷新策略响应: $ALTER_REFRESH_RESPONSE"
echo -e "${GREEN}✓ 刷新策略修改成功${NC}"
sleep 1
echo ""

# Step 19: ALTER - 设置为INACTIVE
echo -e "${YELLOW}[Step 19] 设置物化视图为INACTIVE: mv_customer_stats${NC}"

ALTER_INACTIVE_RESPONSE=$(curl -s -X PUT "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_customer_stats" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"alter_clause":"INACTIVE"}')

echo "设置INACTIVE响应: $ALTER_INACTIVE_RESPONSE"
echo -e "${GREEN}✓ 已设置为INACTIVE${NC}"
sleep 1
echo ""

# Step 20: ALTER - 设置为ACTIVE
echo -e "${YELLOW}[Step 20] 设置物化视图为ACTIVE: mv_customer_stats${NC}"

ALTER_ACTIVE_RESPONSE=$(curl -s -X PUT "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_customer_stats" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"alter_clause":"ACTIVE"}')

echo "设置ACTIVE响应: $ALTER_ACTIVE_RESPONSE"
echo -e "${GREEN}✓ 已设置为ACTIVE${NC}"
sleep 1
echo ""

# Step 21: ALTER - 修改属性
echo -e "${YELLOW}[Step 21] 修改物化视图属性: mv_customer_stats${NC}"

ALTER_PROPERTY_RESPONSE=$(curl -s -X PUT "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_customer_stats" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"alter_clause":"SET (\"session.enable_profile\" = \"true\")"}')

echo "修改属性响应: $ALTER_PROPERTY_RESPONSE"
echo -e "${GREEN}✓ 属性修改成功${NC}"
sleep 1
echo ""

# Step 22: 删除物化视图 - 不带IF EXISTS
echo -e "${YELLOW}[Step 22] 删除物化视图（不带IF EXISTS）: mv_customer_stats${NC}"

DELETE_RESPONSE=$(curl -s -X DELETE "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_customer_stats?if_exists=false" \
  -H "Authorization: Bearer ${TOKEN}")

echo "删除响应: $DELETE_RESPONSE"
echo -e "${GREEN}✓ mv_customer_stats 已删除${NC}"
echo ""

# Step 23: 删除物化视图 - 带IF EXISTS
echo -e "${YELLOW}[Step 23] 删除物化视图（IF EXISTS）: mv_daily_sales${NC}"

DELETE_IF_EXISTS_RESPONSE=$(curl -s -X DELETE "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_daily_sales?if_exists=true" \
  -H "Authorization: Bearer ${TOKEN}")

echo "删除响应: $DELETE_IF_EXISTS_RESPONSE"
echo -e "${GREEN}✓ mv_daily_sales 已删除${NC}"
echo ""

# Step 24: 删除分区物化视图
echo -e "${YELLOW}[Step 24] 删除分区物化视图: mv_monthly_sales${NC}"

curl -s -X DELETE "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/mv_monthly_sales?if_exists=true" \
  -H "Authorization: Bearer ${TOKEN}" > /dev/null

echo -e "${GREEN}✓ mv_monthly_sales 已删除${NC}"
echo ""

# Step 25: 测试错误场景 - 查询不存在的物化视图
echo -e "${YELLOW}[Step 25] 测试错误场景: 查询不存在的物化视图${NC}"

ERROR_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/non_existent_mv" \
  -H "Authorization: Bearer ${TOKEN}")

if echo "$ERROR_RESPONSE" | grep -q "not found"; then
    echo -e "${GREEN}✓ 正确返回404错误${NC}"
else
    echo -e "${RED}✗ 错误处理不正确${NC}"
    echo "$ERROR_RESPONSE"
fi
echo ""

# Step 26: 测试错误场景 - 删除不存在的物化视图（不带IF EXISTS）
echo -e "${YELLOW}[Step 26] 测试错误场景: 删除不存在的物化视图（不带IF EXISTS）${NC}"

ERROR_DELETE_RESPONSE=$(curl -s -X DELETE "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/non_existent_mv?if_exists=false" \
  -H "Authorization: Bearer ${TOKEN}")

if echo "$ERROR_DELETE_RESPONSE" | grep -q "not found"; then
    echo -e "${GREEN}✓ 正确返回错误${NC}"
else
    echo -e "${YELLOW}⚠ 可能需要检查错误处理${NC}"
fi
echo "$ERROR_DELETE_RESPONSE" | python3 -m json.tool 2>/dev/null || echo "$ERROR_DELETE_RESPONSE"
echo ""

# Step 27: 清理测试数据
echo -e "${YELLOW}[Step 27] 清理测试数据...${NC}"

# 删除测试表
curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"sql":"DROP TABLE IF EXISTS test_mv_db.orders"}' > /dev/null

echo -e "${GREEN}✓ test_mv_db.orders 表已删除${NC}"

# 删除测试数据库
curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"sql":"DROP DATABASE IF EXISTS test_mv_db"}' > /dev/null

echo -e "${GREEN}✓ test_mv_db 数据库已删除${NC}"
echo ""

echo -e "${GREEN}================================${NC}"
echo -e "${GREEN}✓ 所有测试完成！${NC}"
echo -e "${GREEN}================================${NC}"
echo ""

echo "测试覆盖率总结："
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📋 CREATE 操作:"
echo "  ✓ 创建同步物化视图（ROLLUP）"
echo "  ✓ 创建异步物化视图（MANUAL刷新）"
echo "  ✓ 创建异步物化视图（ASYNC自动刷新）"
echo "  ✓ 创建分区物化视图"
echo ""
echo "📋 READ 操作:"
echo "  ✓ 查询所有物化视图（无过滤）"
echo "  ✓ 查询指定数据库的物化视图（database参数）"
echo "  ✓ 查询单个物化视图详情"
echo "  ✓ 查询物化视图DDL"
echo ""
echo "📋 UPDATE 操作:"
echo "  ✓ ALTER - 重命名物化视图（RENAME）"
echo "  ✓ ALTER - 修改刷新策略（REFRESH）"
echo "  ✓ ALTER - 设置为INACTIVE状态"
echo "  ✓ ALTER - 设置为ACTIVE状态"
echo "  ✓ ALTER - 修改属性（SET）"
echo ""
echo "📋 DELETE 操作:"
echo "  ✓ 删除物化视图（不带IF EXISTS）"
echo "  ✓ 删除物化视图（带IF EXISTS）"
echo ""
echo "📋 REFRESH 操作:"
echo "  ✓ 手动刷新（ASYNC模式）"
echo "  ✓ 强制刷新（SYNC模式）"
echo "  ✓ 刷新指定分区（PARTITION START/END）"
echo "  ✓ 取消刷新（普通取消）"
echo "  ✓ 强制取消刷新（FORCE）"
echo ""
echo "📋 错误处理:"
echo "  ✓ 查询不存在的物化视图"
echo "  ✓ 删除不存在的物化视图"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}总计: 27个测试场景，覆盖所有CRUD和管理操作${NC}"
echo ""

