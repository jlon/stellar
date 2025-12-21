use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::models::{
    CreateFunctionRequest, SystemFunction, SystemFunctionPreference, UpdateFunctionRequest,
    UpdateOrderRequest,
};
use crate::services::{ClusterService, MySQLClient, MySQLPoolManager};
use crate::utils::{vec_to_map, ApiError, ApiResult, StringExt};

#[derive(Clone)]
pub struct SystemFunctionService {
    db: Arc<SqlitePool>,
    mysql_pool_manager: Arc<MySQLPoolManager>,
    cluster_service: Arc<ClusterService>,
}

impl SystemFunctionService {
    pub fn new(
        db: Arc<SqlitePool>,
        mysql_pool_manager: Arc<MySQLPoolManager>,
        cluster_service: Arc<ClusterService>,
    ) -> Self {
        Self { db, mysql_pool_manager, cluster_service }
    }

    pub async fn get_functions(&self, cluster_id: i64) -> ApiResult<Vec<SystemFunction>> {
        tracing::debug!("Getting system functions for cluster_id: {}", cluster_id);

        let all_functions = sqlx::query_as::<_, SystemFunction>(
            "SELECT * FROM system_functions WHERE cluster_id IS NULL OR cluster_id = ?",
        )
        .bind(cluster_id)
        .fetch_all(&*self.db)
        .await?;

        tracing::debug!("Found {} function definitions", all_functions.len());

        let preferences = sqlx::query_as::<_, SystemFunctionPreference>(
            "SELECT * FROM system_function_preferences WHERE cluster_id = ?",
        )
        .bind(cluster_id)
        .fetch_all(&*self.db)
        .await?;

        tracing::debug!("Found {} preference settings", preferences.len());

        // 使用 vec_to_map 构建偏好设置映射
        let preference_map = vec_to_map(preferences, |p| p.function_id);

        // 使用 lambda 表达式合并函数和偏好设置
        let mut merged_functions: Vec<SystemFunction> = all_functions
            .into_iter()
            .map(|mut func| {
                if let Some(pref) = preference_map.get(&func.id) {
                    func.category_order = pref.category_order;
                    func.display_order = pref.display_order;
                    func.is_favorited = pref.is_favorited;
                }
                if func.cluster_id == 0 {
                    func.cluster_id = cluster_id;
                }
                func
            })
            .collect();
            
        // 使用 lambda 表达式排序
        merged_functions.sort_by(|a, b| {
            a.category_order
                .cmp(&b.category_order)
                .then_with(|| a.display_order.cmp(&b.display_order))
        });

        tracing::debug!(
            "Returning {} merged functions for cluster_id: {}",
            merged_functions.len(),
            cluster_id
        );
        Ok(merged_functions)
    }

    pub async fn create_function(
        &self,
        cluster_id: i64,
        req: CreateFunctionRequest,
        user_id: i64,
    ) -> ApiResult<SystemFunction> {
        tracing::info!(
            "Creating system function: {} for cluster_id: {} by user_id: {}",
            req.function_name,
            cluster_id,
            user_id
        );

        // 使用 StringExt trait 进行字符串清理和验证
        let category_name = req.category_name.trimmed();
        let function_name = req.function_name.trimmed();
        let description = req.description.trimmed();
        let sql_query = req.sql_query.trimmed();

        // 使用 lambda 表达式进行验证
        let validations = [
            (category_name.is_empty(), "Category name cannot be empty"),
            (function_name.is_empty(), "Function name cannot be empty"),
            (description.is_empty(), "Function description cannot be empty"),
            (sql_query.is_empty(), "SQL query cannot be empty"),
        ];
        
        if let Some((_, msg)) = validations.iter().find(|(is_empty, _)| *is_empty) {
            return Err(ApiError::validation_error(*msg));
        }

        self.validate_sql_safety(&sql_query)?;

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM system_functions WHERE cluster_id = ? AND category_name = ?",
        )
        .bind(cluster_id)
        .bind(&category_name)
        .fetch_one(&*self.db)
        .await?;

        if count >= 4 {
            return Err(ApiError::category_full(
                "This category already has 4 functions, cannot add more",
            ));
        }

        let max_order: Option<i32> = sqlx::query_scalar(
            "SELECT MAX(display_order) FROM system_functions WHERE cluster_id = ? AND category_name = ?"
        )
        .bind(cluster_id)
        .bind(&category_name)
        .fetch_optional(&*self.db)
        .await?;

        let display_order = max_order.unwrap_or(0) + 1;

        let max_category_order: Option<i32> = sqlx::query_scalar(
            "SELECT MAX(category_order) FROM system_functions WHERE cluster_id = ?",
        )
        .bind(cluster_id)
        .fetch_optional(&*self.db)
        .await?;

        let category_order = max_category_order.unwrap_or(0) + 1;

        let function_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO system_functions (
                cluster_id, category_name, function_name, description, sql_query,
                display_order, category_order, is_favorited, created_by
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?) RETURNING id",
        )
        .bind(cluster_id)
        .bind(&category_name)
        .bind(&function_name)
        .bind(&description)
        .bind(&sql_query)
        .bind(display_order)
        .bind(category_order)
        .bind(user_id)
        .fetch_one(&*self.db)
        .await?;

        let function =
            sqlx::query_as::<_, SystemFunction>("SELECT * FROM system_functions WHERE id = ?")
                .bind(function_id)
                .fetch_one(&*self.db)
                .await?;

        Ok(function)
    }

    pub async fn execute_function(
        &self,
        cluster_id: i64,
        function_id: i64,
    ) -> ApiResult<Vec<HashMap<String, Value>>> {
        let function = sqlx::query_as::<_, SystemFunction>(
            "SELECT * FROM system_functions WHERE id = ? AND cluster_id = ?",
        )
        .bind(function_id)
        .bind(cluster_id)
        .fetch_optional(&*self.db)
        .await?
        .ok_or_else(|| ApiError::not_found("Function not found or deleted"))?;

        sqlx::query("UPDATE system_functions SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(function_id)
            .execute(&*self.db)
            .await?;

        let cluster = self.cluster_service.get_cluster(cluster_id).await?;

        let pool = self.mysql_pool_manager.get_pool(&cluster).await?;
        let mysql_client = MySQLClient::from_pool(pool);

        let (columns, rows) = mysql_client.query_raw(&function.sql_query).await?;

        // 使用 lambda 表达式转换结果
        let result: Vec<HashMap<String, Value>> = rows
            .into_iter()
            .map(|row| {
                columns
                    .iter()
                    .zip(row.iter())
                    .map(|(col, val)| (col.clone(), Value::String(val.clone())))
                    .collect()
            })
            .collect();

        Ok(result)
    }

    pub async fn update_system_function_access_time(&self, function_name: &str) -> ApiResult<()> {
        sqlx::query(
            "UPDATE system_functions SET updated_at = CURRENT_TIMESTAMP WHERE function_name = ? AND cluster_id IS NULL"
        )
        .bind(function_name)
        .execute(&*self.db)
        .await?;

        Ok(())
    }

    pub async fn update_orders(&self, cluster_id: i64, req: UpdateOrderRequest) -> ApiResult<()> {
        let mut tx = self.db.begin().await?;

        for order in req.functions {
            sqlx::query(
                "INSERT INTO system_function_preferences (cluster_id, function_id, category_order, display_order, is_favorited, updated_at)
                 VALUES (?, ?, ?, ?, COALESCE((SELECT is_favorited FROM system_function_preferences WHERE cluster_id = ? AND function_id = ?), false), CURRENT_TIMESTAMP)
                 ON CONFLICT(cluster_id, function_id) DO UPDATE SET
                 category_order = excluded.category_order,
                 display_order = excluded.display_order,
                 updated_at = CURRENT_TIMESTAMP"
            )
            .bind(cluster_id)
            .bind(order.id)
            .bind(order.category_order)
            .bind(order.display_order)
            .bind(cluster_id)
            .bind(order.id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn toggle_favorite(
        &self,
        cluster_id: i64,
        function_id: i64,
    ) -> ApiResult<SystemFunction> {
        let current_favorited: Option<bool> = sqlx::query_scalar(
            "SELECT is_favorited FROM system_function_preferences WHERE cluster_id = ? AND function_id = ?"
        )
        .bind(cluster_id)
        .bind(function_id)
        .fetch_optional(&*self.db)
        .await?;

        let new_favorited = !current_favorited.unwrap_or(false);

        let (default_category_order, default_display_order): (i32, i32) = sqlx::query_as(
            "SELECT category_order, display_order FROM system_functions WHERE id = ?",
        )
        .bind(function_id)
        .fetch_one(&*self.db)
        .await?;

        sqlx::query(
            "INSERT INTO system_function_preferences (cluster_id, function_id, category_order, display_order, is_favorited, updated_at)
             VALUES (?, ?, 
                     COALESCE((SELECT category_order FROM system_function_preferences WHERE cluster_id = ? AND function_id = ?), ?),
                     COALESCE((SELECT display_order FROM system_function_preferences WHERE cluster_id = ? AND function_id = ?), ?),
                     ?, CURRENT_TIMESTAMP)
             ON CONFLICT(cluster_id, function_id) DO UPDATE SET
             is_favorited = excluded.is_favorited,
             updated_at = CURRENT_TIMESTAMP"
        )
        .bind(cluster_id)
        .bind(function_id)
        .bind(cluster_id)
        .bind(function_id)
        .bind(default_category_order)
        .bind(cluster_id)
        .bind(function_id)
        .bind(default_display_order)
        .bind(new_favorited)
        .execute(&*self.db)
        .await?;

        self.get_functions(cluster_id)
            .await?
            .into_iter()
            .find(|f| f.id == function_id)
            .ok_or_else(|| ApiError::not_found("Function not found or deleted"))
    }

    pub async fn update_function(
        &self,
        cluster_id: i64,
        function_id: i64,
        req: UpdateFunctionRequest,
    ) -> ApiResult<SystemFunction> {
        // 使用 StringExt trait 进行字符串清理
        let category_name = req.category_name.trimmed();
        let function_name = req.function_name.trimmed();
        let description = req.description.trimmed();
        let sql_query = req.sql_query.trimmed();

        // 使用 lambda 表达式进行验证
        let validations = [
            (category_name.is_empty(), "Category name cannot be empty"),
            (function_name.is_empty(), "Function name cannot be empty"),
            (description.is_empty(), "Function description cannot be empty"),
            (sql_query.is_empty(), "SQL query cannot be empty"),
        ];
        
        if let Some((_, msg)) = validations.iter().find(|(is_empty, _)| *is_empty) {
            return Err(ApiError::validation_error(*msg));
        }

        self.validate_sql_safety(&sql_query)?;

        sqlx::query(
            "UPDATE system_functions SET 
             category_name = ?, function_name = ?, description = ?, sql_query = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ? AND cluster_id = ?"
        )
        .bind(category_name)
        .bind(function_name)
        .bind(description)
        .bind(sql_query)
        .bind(function_id)
        .bind(cluster_id)
        .execute(&*self.db)
        .await?;

        self.get_functions(cluster_id)
            .await?
            .into_iter()
            .find(|f| f.id == function_id)
            .ok_or_else(|| ApiError::not_found("Function not found or deleted"))
    }

    pub async fn delete_function(&self, cluster_id: i64, function_id: i64) -> ApiResult<()> {
        let result = sqlx::query("DELETE FROM system_functions WHERE id = ? AND cluster_id = ?")
            .bind(function_id)
            .bind(cluster_id)
            .execute(&*self.db)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::not_found("Function not found or deleted"));
        }

        Ok(())
    }

    fn validate_sql_safety(&self, sql: &str) -> ApiResult<()> {
        let trimmed_sql = sql.trim().to_uppercase();

        if !trimmed_sql.starts_with("SELECT") && !trimmed_sql.starts_with("SHOW") {
            return Err(ApiError::invalid_sql("Only SELECT and SHOW type SQL queries are allowed"));
        }

        // 使用 lambda 表达式检查危险关键字
        let dangerous_keywords = [
            "DROP", "DELETE", "UPDATE", "INSERT", "ALTER", "CREATE", "TRUNCATE", "EXEC", "EXECUTE",
            "CALL", "GRANT", "REVOKE", "COMMIT", "ROLLBACK",
        ];

        let found_keyword = dangerous_keywords.iter().find(|&keyword| {
            trimmed_sql.contains(&format!(" {}", keyword))
                || trimmed_sql.contains(&format!("{} ", keyword))
        });

        if let Some(keyword) = found_keyword {
            return Err(ApiError::sql_safety_violation(format!(
                "SQL查询包含不允许的关键字：{}",
                keyword
            )));
        }

        Ok(())
    }

    pub async fn delete_category(&self, category_name: &str) -> ApiResult<()> {
        let system_categories = [
            "集群信息",
            "数据库管理",
            "事务管理",
            "任务管理",
            "元数据管理",
            "存储管理",
            "作业管理",
        ];

        if system_categories.contains(&category_name) {
            return Err(ApiError::invalid_data("不能删除系统默认分类"));
        }

        sqlx::query(
            "DELETE FROM system_functions WHERE category_name = ? AND cluster_id IS NOT NULL",
        )
        .bind(category_name)
        .execute(&*self.db)
        .await?;

        sqlx::query(
            "DELETE FROM system_function_preferences WHERE function_id IN (
                SELECT id FROM system_functions WHERE category_name = ? AND cluster_id IS NOT NULL
            )",
        )
        .bind(category_name)
        .execute(&*self.db)
        .await?;

        Ok(())
    }
}
