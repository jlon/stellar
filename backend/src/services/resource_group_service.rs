use anyhow::Result;
use mysql_async::Pool;

use crate::models::{
    Classifier, ClassifierRequest, CreateResourceGroupRequest, ResourceGroup,
    ResourceGroupUsage, ResourceUsageAnalysis, UpdateResourceGroupRequest, UserConcurrency,
    UserCpuUsage, UserMemoryUsage,
};
use crate::services::mysql_client::MySQLClient;

pub struct ResourceGroupService;

impl ResourceGroupService {
    pub async fn list_resource_groups(pool: &Pool) -> Result<Vec<ResourceGroup>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = "SHOW RESOURCE GROUPS ALL";
        let (_, rows, _) = session.execute(sql).await?;

        let mut groups = Vec::new();
        for row in rows {
            let name = row.get(0).unwrap_or(&String::new()).clone();
            let id: i64 = row.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let cpu_weight: Option<i32> = row.get(2).and_then(|s| s.parse().ok());
            let exclusive_cpu_cores: Option<i32> = row.get(3).and_then(|s| s.parse().ok());
            let mem_limit: Option<String> = row.get(4).map(|s| s.clone());
            let big_query_cpu_second_limit: Option<i64> = row.get(5).and_then(|s| s.parse().ok());
            let big_query_scan_rows_limit: Option<i64> = row.get(6).and_then(|s| s.parse().ok());
            let big_query_mem_limit: Option<String> = row.get(7).map(|s| s.clone());
            let concurrency_limit: Option<i32> = row.get(8).and_then(|s| s.parse().ok());
            let spill_mem_limit_threshold: Option<String> = row.get(9).map(|s| s.clone());
            let classifiers_str: Option<String> = row.get(10).map(|s| s.clone());

            let classifiers = Self::parse_classifiers(&classifiers_str.unwrap_or_default())?;

            groups.push(ResourceGroup {
                name,
                id,
                cpu_weight,
                exclusive_cpu_cores,
                mem_limit,
                big_query_cpu_second_limit,
                big_query_scan_rows_limit,
                big_query_mem_limit,
                concurrency_limit,
                spill_mem_limit_threshold,
                classifiers,
            });
        }

        Ok(groups)
    }

    pub async fn get_resource_group(pool: &Pool, name: &str) -> Result<ResourceGroup> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = format!("SHOW RESOURCE GROUP {}", Self::quote_identifier(name));
        let (_, rows, _) = session.execute(&sql).await?;
        
        let row = rows.into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("Resource group not found"))?;

        let name = row.get(0).unwrap_or(&String::new()).clone();
        let id: i64 = row.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let cpu_weight: Option<i32> = row.get(2).and_then(|s| s.parse().ok());
        let exclusive_cpu_cores: Option<i32> = row.get(3).and_then(|s| s.parse().ok());
        let mem_limit: Option<String> = row.get(4).map(|s| s.clone());
        let big_query_cpu_second_limit: Option<i64> = row.get(5).and_then(|s| s.parse().ok());
        let big_query_scan_rows_limit: Option<i64> = row.get(6).and_then(|s| s.parse().ok());
        let big_query_mem_limit: Option<String> = row.get(7).map(|s| s.clone());
        let concurrency_limit: Option<i32> = row.get(8).and_then(|s| s.parse().ok());
        let spill_mem_limit_threshold: Option<String> = row.get(9).map(|s| s.clone());
        let classifiers_str: Option<String> = row.get(10).map(|s| s.clone());

        let classifiers = Self::parse_classifiers(&classifiers_str.unwrap_or_default())?;

        Ok(ResourceGroup {
            name,
            id,
            cpu_weight,
            exclusive_cpu_cores,
            mem_limit,
            big_query_cpu_second_limit,
            big_query_scan_rows_limit,
            big_query_mem_limit,
            concurrency_limit,
            spill_mem_limit_threshold,
            classifiers,
        })
    }

    pub async fn create_resource_group(
        pool: &Pool,
        req: CreateResourceGroupRequest,
    ) -> Result<()> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = Self::build_create_sql(&req)?;
        session.execute(&sql).await?;
        Ok(())
    }

    pub async fn update_resource_group(
        pool: &Pool,
        name: &str,
        req: UpdateResourceGroupRequest,
    ) -> Result<()> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = Self::build_alter_sql(name, &req)?;
        session.execute(&sql).await?;
        Ok(())
    }

    pub async fn delete_resource_group(pool: &Pool, name: &str) -> Result<()> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = format!("DROP RESOURCE GROUP {}", Self::quote_identifier(name));
        session.execute(&sql).await?;
        Ok(())
    }

    pub async fn get_resource_group_usage(pool: &Pool) -> Result<Vec<ResourceGroupUsage>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = "SHOW USAGE RESOURCE GROUPS";
        let (_, rows, _) = session.execute(sql).await?;

        let mut usages = Vec::new();
        for row in rows {
            let id: i64 = row.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
            let backend = row.get(1).unwrap_or(&String::new()).clone();
            let be_in_use_cpu_cores: f64 = row.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let be_in_use_mem_bytes: i64 = row.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let be_running_queries: i32 = row.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);

            usages.push(ResourceGroupUsage {
                id,
                backend,
                be_in_use_cpu_cores,
                be_in_use_mem_bytes,
                be_running_queries,
            });
        }

        Ok(usages)
    }

    pub async fn analyze_resource_usage(
        pool: &Pool,
        days: u32,
    ) -> Result<ResourceUsageAnalysis> {
        let cpu_analysis = Self::analyze_cpu_usage(pool, days).await?;
        let memory_analysis = Self::analyze_memory_usage(pool, days).await?;
        let concurrency_analysis = Self::analyze_concurrency(pool, days).await?;

        Ok(ResourceUsageAnalysis {
            cpu_analysis,
            memory_analysis,
            concurrency_analysis,
        })
    }

    async fn analyze_cpu_usage(pool: &Pool, days: u32) -> Result<Vec<UserCpuUsage>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        let sql = format!(
            r#"
            SELECT 
                user,
                SUM(cpuCostNs) / 1e9 AS total_cpu_seconds,
                (SUM(cpuCostNs) / (
                    SELECT SUM(cpuCostNs) 
                    FROM starrocks_audit_db__.starrocks_audit_tbl__ 
                    WHERE timestamp >= DATE_SUB(NOW(), INTERVAL {} DAY)
                )) * 100 AS cpu_usage_percentage
            FROM starrocks_audit_db__.starrocks_audit_tbl__
            WHERE timestamp >= DATE_SUB(NOW(), INTERVAL {} DAY)
              AND state IN ('EOF', 'OK')
            GROUP BY user
            ORDER BY total_cpu_seconds DESC
            LIMIT 50
            "#,
            days, days
        );

        let (_, rows, _) = session.execute(&sql).await?;

        let mut results = Vec::new();
        for row in rows {
            let user = row.get(0).unwrap_or(&String::new()).clone();
            let total_cpu_seconds: f64 = row.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let cpu_usage_percentage: f64 = row.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);

            let suggested_cpu_weight = (cpu_usage_percentage * 100.0).round() as i32;
            let suggested_exclusive_cores = (cpu_usage_percentage * 64.0 / 100.0).round() as i32;

            results.push(UserCpuUsage {
                user,
                total_cpu_seconds,
                cpu_usage_percentage,
                suggested_cpu_weight: suggested_cpu_weight.max(1).min(100),
                suggested_exclusive_cores: suggested_exclusive_cores.max(0),
            });
        }

        Ok(results)
    }

    async fn analyze_memory_usage(pool: &Pool, days: u32) -> Result<Vec<UserMemoryUsage>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = format!(
            r#"
            SELECT 
                user,
                MAX(memCostBytes) / 1024 / 1024 AS max_mem_mb
            FROM starrocks_audit_db__.starrocks_audit_tbl__
            WHERE timestamp >= DATE_SUB(NOW(), INTERVAL {} DAY)
              AND state IN ('EOF', 'OK')
            GROUP BY user
            ORDER BY max_mem_mb DESC
            LIMIT 50
            "#,
            days
        );

        let (_, rows, _) = session.execute(&sql).await?;

        let mut results = Vec::new();
        for row in rows {
            let user = row.get(0).unwrap_or(&String::new()).clone();
            let max_mem_mb: f64 = row.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);

            let suggested_mem_limit = format!("{}%", ((max_mem_mb / 1024.0) * 1.2).round() as i32);
            let suggested_big_query_mem_limit =
                format!("{}GB", (max_mem_mb / 1024.0 * 1.5).round() as i32);

            results.push(UserMemoryUsage {
                user,
                max_mem_mb,
                suggested_mem_limit,
                suggested_big_query_mem_limit,
            });
        }

        Ok(results)
    }

    async fn analyze_concurrency(pool: &Pool, days: u32) -> Result<Vec<UserConcurrency>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        
        let sql = format!(
            r#"
            WITH UserConcurrency AS (
                SELECT 
                    user,
                    DATE_FORMAT(timestamp, '%Y-%m-%d %H:%i') AS minute_bucket,
                    COUNT(*) AS query_concurrency
                FROM starrocks_audit_db__.starrocks_audit_tbl__
                WHERE state IN ('EOF', 'OK')
                  AND timestamp >= DATE_SUB(NOW(), INTERVAL {} DAY)
                  AND LOWER(stmt) LIKE '%select%'
                GROUP BY user, minute_bucket
                HAVING query_concurrency > 1
            )
            SELECT 
                user,
                minute_bucket,
                query_concurrency / 60.0 AS query_concurrency_per_second
            FROM (
                SELECT 
                    user,
                    minute_bucket,
                    query_concurrency,
                    ROW_NUMBER() OVER (
                        PARTITION BY user
                        ORDER BY query_concurrency DESC
                    ) AS rn
                FROM UserConcurrency
            ) ranked
            WHERE rn = 1
            ORDER BY query_concurrency_per_second DESC
            LIMIT 50
            "#,
            days
        );

        let (_, rows, _) = session.execute(&sql).await?;

        let mut results = Vec::new();
        for row in rows {
            let user = row.get(0).unwrap_or(&String::new()).clone();
            let max_concurrency_per_second: f64 = row.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);

            let suggested_concurrency_limit = (max_concurrency_per_second * 1.5).ceil() as i32;

            results.push(UserConcurrency {
                user,
                max_concurrency_per_second,
                suggested_concurrency_limit: suggested_concurrency_limit.max(1),
            });
        }

        Ok(results)
    }

    fn build_create_sql(req: &CreateResourceGroupRequest) -> Result<String> {
        let mut sql = format!("CREATE RESOURCE GROUP {}", Self::quote_identifier(&req.name));

        if !req.classifiers.is_empty() {
            sql.push_str("\nTO (");
            let classifiers: Vec<String> = req
                .classifiers
                .iter()
                .map(|c| Self::build_classifier_clause(c))
                .collect();
            sql.push_str(&classifiers.join(", "));
            sql.push(')');
        }

        let mut with_clauses = Vec::new();

        if let Some(cpu_weight) = req.cpu_weight {
            with_clauses.push(format!("'cpu_weight' = '{}'", cpu_weight));
        }
        if let Some(exclusive_cpu_cores) = req.exclusive_cpu_cores {
            with_clauses.push(format!("'exclusive_cpu_cores' = '{}'", exclusive_cpu_cores));
        }
        if let Some(ref mem_limit) = req.mem_limit {
            with_clauses.push(format!("'mem_limit' = '{}'", mem_limit));
        }
        if let Some(big_query_cpu_second_limit) = req.big_query_cpu_second_limit {
            with_clauses.push(format!(
                "'big_query_cpu_second_limit' = '{}'",
                big_query_cpu_second_limit
            ));
        }
        if let Some(big_query_scan_rows_limit) = req.big_query_scan_rows_limit {
            with_clauses.push(format!(
                "'big_query_scan_rows_limit' = '{}'",
                big_query_scan_rows_limit
            ));
        }
        if let Some(ref big_query_mem_limit) = req.big_query_mem_limit {
            with_clauses.push(format!(
                "'big_query_mem_limit' = '{}'",
                big_query_mem_limit
            ));
        }
        if let Some(concurrency_limit) = req.concurrency_limit {
            with_clauses.push(format!("'concurrency_limit' = '{}'", concurrency_limit));
        }
        if let Some(ref spill_mem_limit_threshold) = req.spill_mem_limit_threshold {
            with_clauses.push(format!(
                "'spill_mem_limit_threshold' = '{}'",
                spill_mem_limit_threshold
            ));
        }

        if !with_clauses.is_empty() {
            sql.push_str("\nWITH (");
            sql.push_str(&with_clauses.join(", "));
            sql.push(')');
        }

        Ok(sql)
    }

    fn build_alter_sql(name: &str, req: &UpdateResourceGroupRequest) -> Result<String> {
        let mut sql = format!("ALTER RESOURCE GROUP {}", Self::quote_identifier(name));

        let mut set_clauses = Vec::new();

        if let Some(cpu_weight) = req.cpu_weight {
            set_clauses.push(format!("'cpu_weight' = '{}'", cpu_weight));
        }
        if let Some(exclusive_cpu_cores) = req.exclusive_cpu_cores {
            set_clauses.push(format!("'exclusive_cpu_cores' = '{}'", exclusive_cpu_cores));
        }
        if let Some(ref mem_limit) = req.mem_limit {
            set_clauses.push(format!("'mem_limit' = '{}'", mem_limit));
        }
        if let Some(big_query_cpu_second_limit) = req.big_query_cpu_second_limit {
            set_clauses.push(format!(
                "'big_query_cpu_second_limit' = '{}'",
                big_query_cpu_second_limit
            ));
        }
        if let Some(big_query_scan_rows_limit) = req.big_query_scan_rows_limit {
            set_clauses.push(format!(
                "'big_query_scan_rows_limit' = '{}'",
                big_query_scan_rows_limit
            ));
        }
        if let Some(ref big_query_mem_limit) = req.big_query_mem_limit {
            set_clauses.push(format!(
                "'big_query_mem_limit' = '{}'",
                big_query_mem_limit
            ));
        }
        if let Some(concurrency_limit) = req.concurrency_limit {
            set_clauses.push(format!("'concurrency_limit' = '{}'", concurrency_limit));
        }
        if let Some(ref spill_mem_limit_threshold) = req.spill_mem_limit_threshold {
            set_clauses.push(format!(
                "'spill_mem_limit_threshold' = '{}'",
                spill_mem_limit_threshold
            ));
        }

        if !set_clauses.is_empty() {
            sql.push_str(" SET (");
            sql.push_str(&set_clauses.join(", "));
            sql.push(')');
        }

        if let Some(ref add_classifiers) = req.add_classifiers {
            if !add_classifiers.is_empty() {
                sql.push_str(" ADD (");
                let classifiers: Vec<String> = add_classifiers
                    .iter()
                    .map(|c| Self::build_classifier_clause(c))
                    .collect();
                sql.push_str(&classifiers.join(", "));
                sql.push(')');
            }
        }

        if let Some(ref drop_ids) = req.drop_classifier_ids {
            if !drop_ids.is_empty() {
                sql.push_str(" DROP (");
                let ids: Vec<String> = drop_ids.iter().map(|id| id.to_string()).collect();
                sql.push_str(&ids.join(", "));
                sql.push(')');
            }
        }

        Ok(sql)
    }

    fn build_classifier_clause(classifier: &ClassifierRequest) -> String {
        let mut conditions = Vec::new();

        if let Some(ref user) = classifier.user {
            conditions.push(format!("user='{}'", user));
        }
        if let Some(ref role) = classifier.role {
            conditions.push(format!("role='{}'", role));
        }
        if let Some(ref query_types) = classifier.query_type {
            if !query_types.is_empty() {
                let types: Vec<String> = query_types.iter().map(|t| format!("'{}'", t)).collect();
                conditions.push(format!("query_type IN ({})", types.join(", ")));
            }
        }
        if let Some(ref source_ip) = classifier.source_ip {
            conditions.push(format!("source_ip='{}'", source_ip));
        }
        if let Some(ref db) = classifier.db {
            conditions.push(format!("db='{}'", db));
        }

        conditions.join(", ")
    }

    fn parse_classifiers(classifiers_str: &str) -> Result<Vec<Classifier>> {
        if classifiers_str.is_empty() {
            return Ok(Vec::new());
        }

        match serde_json::from_str::<Vec<Classifier>>(classifiers_str) {
            Ok(classifiers) => Ok(classifiers),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn quote_identifier(name: &str) -> String {
        format!("`{}`", name.replace('`', "``"))
    }
}
