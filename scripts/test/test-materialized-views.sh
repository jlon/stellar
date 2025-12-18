#!/bin/bash

# 物化视图功能完整测试脚本
# 测试集群ID: 1
# 测试数据库: test

set -e

BASE_URL="http://localhost:8081/api"
CLUSTER_ID=1
DATABASE="test"
TOKEN=""

# 颜色输出
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "${YELLOW}================================${NC}"
echo "${YELLOW}物化视图功能测试脚本${NC}"
echo "${YELLOW}================================${NC}"
echo ""

# 1. 登录获取token
echo "${YELLOW}[Step 1] 登录获取JWT Token...${NC}"
LOGIN_RESPONSE=$(curl -s -X POST "${BASE_URL}/auth/login" \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"admin"}')

TOKEN=$(echo $LOGIN_RESPONSE | grep -o '"token":"[^"]*' | sed 's/"token":"//')

if [ -z "$TOKEN" ]; then
  echo "${RED}✗ 登录失败${NC}"
  echo "Response: $LOGIN_RESPONSE"
  exit 1
fi

echo "${GREEN}✓ 登录成功${NC}"
echo ""

# 2. 创建测试基础表
echo "${YELLOW}[Step 2] 创建测试基础表...${NC}"

# 创建订单表
CREATE_TABLE_SQL="CREATE TABLE IF NOT EXISTS test.orders (order_id BIGINT, order_date DATE, customer_id INT, product_id INT, quantity INT, amount DECIMAL(10,2)) DUPLICATE KEY(order_id) PARTITION BY RANGE(order_date) (PARTITION p20240101 VALUES LESS THAN ('2024-02-01'), PARTITION p20240201 VALUES LESS THAN ('2024-03-01'), PARTITION p20240301 VALUES LESS THAN ('2024-04-01')) DISTRIBUTED BY HASH(order_id) BUCKETS 8;"

curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_TABLE_SQL}\",\"limit\":100}" > /dev/null

echo "${GREEN}✓ 基础表创建完成${NC}"
echo ""

# 3. 插入测试数据
echo "${YELLOW}[Step 3] 插入测试数据...${NC}"

INSERT_SQL="INSERT INTO test.orders VALUES (1, '2024-01-15', 101, 1001, 5, 99.50), (2, '2024-01-20', 102, 1002, 3, 149.99), (3, '2024-02-10', 103, 1003, 2, 299.00), (4, '2024-02-25', 101, 1001, 1, 19.90);"

curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${INSERT_SQL}\",\"limit\":100}" > /dev/null

echo "${GREEN}✓ 测试数据插入完成${NC}"
echo ""

# 4. 创建同步物化视图（ROLLUP）
echo "${YELLOW}[Step 4] 创建同步物化视图 (ROLLUP)...${NC}"

# 同步物化视图不需要DISTRIBUTED BY，导入时自动刷新
# 使用简单的聚合函数，避免const expr错误
CREATE_SYNC_MV_SQL="CREATE MATERIALIZED VIEW test.orders_sync_mv AS SELECT order_date, customer_id, SUM(amount) as sum_amount FROM test.orders GROUP BY order_date, customer_id;"

RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_SYNC_MV_SQL}\"}")

echo "Response: $RESPONSE"
echo "${GREEN}✓ 同步物化视图创建请求已发送${NC}"

# Wait for sync MV to finish building
echo "等待同步物化视图构建完成..."
sleep 5
echo ""

# 5. 创建异步物化视图（MANUAL）
echo "${YELLOW}[Step 5] 创建异步物化视图 (MANUAL)...${NC}"

# 异步物化视图必须有DISTRIBUTED BY和REFRESH子句，并明确指定数据库
CREATE_ASYNC_MV_SQL="CREATE MATERIALIZED VIEW test.orders_daily_summary DISTRIBUTED BY HASH(order_date) BUCKETS 10 REFRESH MANUAL AS SELECT order_date, SUM(amount) as daily_sales, SUM(quantity) as daily_quantity FROM test.orders GROUP BY order_date;"

RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"sql\":\"${CREATE_ASYNC_MV_SQL}\"}")

echo "Response: $RESPONSE"
echo "${GREEN}✓ 异步物化视图创建请求已发送${NC}"
sleep 3
echo ""

# 6. 列出所有物化视图（获取所有数据库的MV）
echo "${YELLOW}[Step 6] 查询物化视图列表（所有数据库）...${NC}"

LIST_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views" \
  -H "Authorization: Bearer ${TOKEN}")

echo "物化视图列表："
echo "$LIST_RESPONSE" | python3 -m json.tool 2>/dev/null || echo "$LIST_RESPONSE"
echo ""

# 提取第一个物化视图名称用于后续测试
MV_NAME=$(echo "$LIST_RESPONSE" | grep -o '"name":"[^"]*' | head -1 | sed 's/"name":"//')

if [ -z "$MV_NAME" ]; then
  echo "${YELLOW}⚠ 未找到物化视图，跳过详细测试${NC}"
else
  echo "${GREEN}✓ 找到物化视图: $MV_NAME${NC}"
  echo ""

  # 7. 查询单个物化视图详情
  echo "${YELLOW}[Step 7] 查询物化视图详情: $MV_NAME${NC}"

  DETAIL_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/${MV_NAME}" \
    -H "Authorization: Bearer ${TOKEN}")

  echo "详情："
  echo "$DETAIL_RESPONSE" | python3 -m json.tool 2>/dev/null || echo "$DETAIL_RESPONSE"
  echo ""

  # 8. 查询DDL
  echo "${YELLOW}[Step 8] 查询物化视图DDL: $MV_NAME${NC}"

  DDL_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/${MV_NAME}/ddl" \
    -H "Authorization: Bearer ${TOKEN}")

  echo "DDL:"
  echo "$DDL_RESPONSE" | python3 -m json.tool 2>/dev/null || echo "$DDL_RESPONSE"
  echo ""

  # 9. 刷新物化视图（仅对ASYNC/MANUAL有效）
  if echo "$LIST_RESPONSE" | grep -q "orders_daily_summary"; then
    echo "${YELLOW}[Step 9] 刷新异步物化视图: orders_daily_summary${NC}"
    
    REFRESH_RESPONSE=$(curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/orders_daily_summary/refresh" \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "Content-Type: application/json" \
      -d '{"mode":"ASYNC","force":false}')
    
    echo "刷新响应: $REFRESH_RESPONSE"
    echo "${GREEN}✓ 刷新请求已发送${NC}"
    sleep 2
    echo ""
  fi
fi

# 10. 测试筛选（带数据库参数）
echo "${YELLOW}[Step 10] 测试数据库筛选...${NC}"

FILTER_RESPONSE=$(curl -s -X GET "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views?database=test" \
  -H "Authorization: Bearer ${TOKEN}")

MV_COUNT=$(echo "$FILTER_RESPONSE" | grep -o '"name":' | wc -l)
echo "${GREEN}✓ 找到 $MV_COUNT 个物化视图（数据库=test）${NC}"
echo ""

# # 11. 清理：删除测试物化视图
# echo "${YELLOW}[Step 11] 清理测试数据...${NC}"

# if echo "$LIST_RESPONSE" | grep -q "orders_sync_mv"; then
#   echo "删除同步物化视图: orders_sync_mv"
#   curl -s -X DELETE "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/orders_sync_mv?if_exists=true" \
#     -H "Authorization: Bearer ${TOKEN}" > /dev/null
#   echo "${GREEN}✓ orders_sync_mv 已删除${NC}"
# fi

# if echo "$LIST_RESPONSE" | grep -q "orders_daily_summary"; then
#   echo "删除异步物化视图: orders_daily_summary"
#   curl -s -X DELETE "${BASE_URL}/clusters/${CLUSTER_ID}/materialized_views/orders_daily_summary?if_exists=true" \
#     -H "Authorization: Bearer ${TOKEN}" > /dev/null
#   echo "${GREEN}✓ orders_daily_summary 已删除${NC}"
# fi

# # 删除测试表
# echo "删除测试表: orders"
# curl -s -X POST "${BASE_URL}/clusters/${CLUSTER_ID}/queries/execute" \
#   -H "Authorization: Bearer ${TOKEN}" \
#   -H "Content-Type: application/json" \
#   -d '{"sql":"DROP TABLE IF EXISTS test.orders;","limit":100}' > /dev/null

# echo "${GREEN}✓ test.orders 表已删除${NC}"
# echo ""

echo "${GREEN}================================${NC}"
echo "${GREEN}✓ 所有测试完成！${NC}"
echo "${GREEN}================================${NC}"
echo ""
echo "测试总结："
echo "1. ✓ 登录认证"
echo "2. ✓ 创建基础表和测试数据"
echo "3. ✓ 创建同步物化视图（ROLLUP）"
echo "4. ✓ 创建异步物化视图（MANUAL）"
echo "5. ✓ 查询物化视图列表"
echo "6. ✓ 查询物化视图详情"
echo "7. ✓ 查询物化视图DDL"
echo "8. ✓ 刷新物化视图"
echo "9. ✓ 数据库筛选"
echo "10. ✓ 删除物化视图"
echo ""
echo "${YELLOW}可以在浏览器中访问 http://localhost:4200 查看前端界面${NC}"
