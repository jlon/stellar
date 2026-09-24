use anyhow::{Context, Result};
use mysql_async::Pool;

use crate::config::AuditLogConfig;
use crate::models::{
    Classifier, ClassifierRequest, CreateResourceGroupRequest, ResourceGroup, ResourceGroupUsage,
    ResourceUsageAnalysis, UpdateResourceGroupRequest, UserConcurrency, UserCpuUsage,
    UserMemoryUsage,
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
            let name = row.first().cloned().unwrap_or_default();
            let id: i64 = row.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let cpu_weight: Option<i32> = row.get(2).and_then(|s| s.parse().ok());
            let exclusive_cpu_cores: Option<i32> = row.get(3).and_then(|s| s.parse().ok());
            let mem_limit: Option<String> = row.get(4).cloned();
            let big_query_cpu_second_limit: Option<i64> = row.get(5).and_then(|s| s.parse().ok());
            let big_query_scan_rows_limit: Option<i64> = row.get(6).and_then(|s| s.parse().ok());
            let big_query_mem_limit: Option<String> = row.get(7).cloned();
            let concurrency_limit: Option<i32> = row.get(8).and_then(|s| s.parse().ok());
            let spill_mem_limit_threshold: Option<String> = row.get(9).cloned();
            let classifiers_str: Option<String> = row.get(10).cloned();

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

        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Resource group not found"))?;

        let name = row.first().cloned().unwrap_or_default();
        let id: i64 = row.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let cpu_weight: Option<i32> = row.get(2).and_then(|s| s.parse().ok());
        let exclusive_cpu_cores: Option<i32> = row.get(3).and_then(|s| s.parse().ok());
        let mem_limit: Option<String> = row.get(4).cloned();
        let big_query_cpu_second_limit: Option<i64> = row.get(5).and_then(|s| s.parse().ok());
        let big_query_scan_rows_limit: Option<i64> = row.get(6).and_then(|s| s.parse().ok());
        let big_query_mem_limit: Option<String> = row.get(7).cloned();
        let concurrency_limit: Option<i32> = row.get(8).and_then(|s| s.parse().ok());
        let spill_mem_limit_threshold: Option<String> = row.get(9).cloned();
        let classifiers_str: Option<String> = row.get(10).cloned();

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

    pub async fn create_resource_group(pool: &Pool, req: CreateResourceGroupRequest) -> Result<()> {
        Self::validate_create_request(&req).map_err(anyhow::Error::msg)?;

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
        Self::validate_update_request(&req).map_err(anyhow::Error::msg)?;

        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;

        let statements = Self::build_alter_sqls(name, &req)?;
        for (index, sql) in statements.iter().enumerate() {
            session.execute(sql).await.with_context(|| {
                if index == 0 {
                    format!("resource group update failed at statement {}/{}", index + 1, statements.len())
                } else {
                    format!(
                        "resource group update failed at statement {}/{}; earlier statements may already be applied",
                        index + 1,
                        statements.len()
                    )
                }
            })?;
        }
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
            let id: i64 = row.first().and_then(|s| s.parse().ok()).unwrap_or(0);
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
        audit_config: &AuditLogConfig,
    ) -> Result<ResourceUsageAnalysis> {
        let cpu_analysis = Self::analyze_cpu_usage(pool, days, audit_config).await?;
        let memory_analysis = Self::analyze_memory_usage(pool, days, audit_config).await?;
        let concurrency_analysis = Self::analyze_concurrency(pool, days, audit_config).await?;

        Ok(ResourceUsageAnalysis { cpu_analysis, memory_analysis, concurrency_analysis })
    }

    async fn analyze_cpu_usage(
        pool: &Pool,
        days: u32,
        audit_config: &AuditLogConfig,
    ) -> Result<Vec<UserCpuUsage>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;
        let sql = format!(
            r#"
            SELECT 
                user,
                SUM(cpuCostNs) / 1e9 AS total_cpu_seconds,
                (SUM(cpuCostNs) / (
                    SELECT SUM(cpuCostNs) 
                    FROM {audit_table}
                    WHERE timestamp >= DATE_SUB(NOW(), INTERVAL {days} DAY)
                )) * 100 AS cpu_usage_percentage
            FROM {audit_table}
            WHERE timestamp >= DATE_SUB(NOW(), INTERVAL {days} DAY)
              AND state IN ('EOF', 'OK')
            GROUP BY user
            ORDER BY total_cpu_seconds DESC
            LIMIT 50
            "#,
            audit_table = Self::audit_table(audit_config),
            days = days
        );

        let (_, rows, _) = session.execute(&sql).await?;

        let mut results = Vec::new();
        for row in rows {
            let user = row.first().cloned().unwrap_or_default();
            let total_cpu_seconds: f64 = row.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let cpu_usage_percentage: f64 = row.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);

            let suggested_cpu_weight = (cpu_usage_percentage * 100.0).round() as i32;
            let suggested_exclusive_cores = (cpu_usage_percentage * 64.0 / 100.0).round() as i32;

            results.push(UserCpuUsage {
                user,
                total_cpu_seconds,
                cpu_usage_percentage,
                suggested_cpu_weight: suggested_cpu_weight.clamp(1, 100),
                suggested_exclusive_cores: suggested_exclusive_cores.max(0),
            });
        }

        Ok(results)
    }

    async fn analyze_memory_usage(
        pool: &Pool,
        days: u32,
        audit_config: &AuditLogConfig,
    ) -> Result<Vec<UserMemoryUsage>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;

        let sql = format!(
            r#"
            SELECT 
                user,
                MAX(memCostBytes) / 1024 / 1024 AS max_mem_mb
            FROM {audit_table}
            WHERE timestamp >= DATE_SUB(NOW(), INTERVAL {days} DAY)
              AND state IN ('EOF', 'OK')
            GROUP BY user
            ORDER BY max_mem_mb DESC
            LIMIT 50
            "#,
            audit_table = Self::audit_table(audit_config),
            days = days
        );

        let (_, rows, _) = session.execute(&sql).await?;

        let mut results = Vec::new();
        for row in rows {
            let user = row.first().cloned().unwrap_or_default();
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

    async fn analyze_concurrency(
        pool: &Pool,
        days: u32,
        audit_config: &AuditLogConfig,
    ) -> Result<Vec<UserConcurrency>> {
        let mysql_client = MySQLClient::from_pool(pool.clone());
        let mut session = mysql_client.create_session().await?;

        let sql = format!(
            r#"
            WITH UserConcurrency AS (
                SELECT 
                    user,
                    DATE_FORMAT(timestamp, '%Y-%m-%d %H:%i') AS minute_bucket,
                    COUNT(*) AS query_concurrency
                FROM {audit_table}
                WHERE state IN ('EOF', 'OK')
                  AND timestamp >= DATE_SUB(NOW(), INTERVAL {days} DAY)
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
            audit_table = Self::audit_table(audit_config),
            days = days
        );

        let (_, rows, _) = session.execute(&sql).await?;

        let mut results = Vec::new();
        for row in rows {
            let user = row.first().cloned().unwrap_or_default();
            let max_concurrency_per_second: f64 =
                row.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);

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

        sql.push_str("\nTO (");
        let classifiers: Vec<String> = req
            .classifiers
            .iter()
            .map(Self::build_classifier_clause)
            .collect();
        sql.push_str(&classifiers.join(", "));
        sql.push(')');

        let mut with_clauses = Vec::new();

        if let Some(cpu_weight) = req.cpu_weight {
            with_clauses.push(format!("'cpu_weight' = '{}'", cpu_weight));
        }
        if let Some(exclusive_cpu_cores) = req.exclusive_cpu_cores {
            with_clauses.push(format!("'exclusive_cpu_cores' = '{}'", exclusive_cpu_cores));
        }
        if let Some(ref mem_limit) = req.mem_limit {
            with_clauses.push(format!("'mem_limit' = {}", Self::quote_literal(mem_limit)));
        }
        if let Some(big_query_cpu_second_limit) = req.big_query_cpu_second_limit {
            with_clauses
                .push(format!("'big_query_cpu_second_limit' = '{}'", big_query_cpu_second_limit));
        }
        if let Some(big_query_scan_rows_limit) = req.big_query_scan_rows_limit {
            with_clauses
                .push(format!("'big_query_scan_rows_limit' = '{}'", big_query_scan_rows_limit));
        }
        if let Some(ref big_query_mem_limit) = req.big_query_mem_limit {
            with_clauses.push(format!(
                "'big_query_mem_limit' = {}",
                Self::quote_literal(big_query_mem_limit)
            ));
        }
        if let Some(concurrency_limit) = req.concurrency_limit {
            with_clauses.push(format!("'concurrency_limit' = '{}'", concurrency_limit));
        }
        if let Some(ref spill_mem_limit_threshold) = req.spill_mem_limit_threshold {
            with_clauses.push(format!(
                "'spill_mem_limit_threshold' = {}",
                Self::quote_literal(spill_mem_limit_threshold)
            ));
        }

        if !with_clauses.is_empty() {
            sql.push_str("\nWITH (");
            sql.push_str(&with_clauses.join(", "));
            sql.push(')');
        }

        Ok(sql)
    }

    pub(crate) fn validate_create_request(
        req: &CreateResourceGroupRequest,
    ) -> std::result::Result<(), &'static str> {
        if req
            .mem_limit
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err("mem_limit is required");
        }

        let configured_cpu_modes = usize::from(req.cpu_weight.is_some_and(|value| value > 0))
            + usize::from(req.exclusive_cpu_cores.is_some_and(|value| value > 0));
        if configured_cpu_modes == 0 {
            return Err("exactly one CPU mode must be positive");
        }
        if configured_cpu_modes > 1 {
            return Err("cpu_weight and exclusive_cpu_cores cannot both be positive");
        }

        Self::validate_classifier_requests(&req.classifiers, true)
    }

    pub(crate) fn validate_update_request(
        req: &UpdateResourceGroupRequest,
    ) -> std::result::Result<(), &'static str> {
        if req.cpu_weight.is_some() && req.exclusive_cpu_cores.is_some() {
            let configured_cpu_modes = usize::from(req.cpu_weight.is_some_and(|value| value > 0))
                + usize::from(req.exclusive_cpu_cores.is_some_and(|value| value > 0));
            if configured_cpu_modes != 1 {
                return Err("exactly one CPU mode must be positive when both modes are updated");
            }
        }

        if req
            .mem_limit
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err("mem_limit cannot be empty");
        }

        if let Some(classifiers) = &req.add_classifiers {
            Self::validate_classifier_requests(classifiers, false)?;
        }

        let has_property_change = req.cpu_weight.is_some()
            || req.exclusive_cpu_cores.is_some()
            || req.mem_limit.is_some()
            || req.big_query_cpu_second_limit.is_some()
            || req.big_query_scan_rows_limit.is_some()
            || req.big_query_mem_limit.is_some()
            || req.concurrency_limit.is_some()
            || req.spill_mem_limit_threshold.is_some();
        let has_classifier_change = req
            .add_classifiers
            .as_ref()
            .is_some_and(|items| !items.is_empty())
            || req
                .drop_classifier_ids
                .as_ref()
                .is_some_and(|ids| !ids.is_empty());
        if !has_property_change && !has_classifier_change {
            return Err("at least one resource group update is required");
        }

        Ok(())
    }

    fn validate_classifier_requests(
        classifiers: &[ClassifierRequest],
        require_classifier: bool,
    ) -> std::result::Result<(), &'static str> {
        if require_classifier && classifiers.is_empty() {
            return Err("at least one classifier is required");
        }

        for classifier in classifiers {
            if !Self::classifier_has_condition(classifier) {
                return Err("each classifier must define at least one condition");
            }
            if classifier.query_type.as_ref().is_some_and(|query_types| {
                query_types.iter().any(|query_type| {
                    !query_type.eq_ignore_ascii_case("SELECT")
                        && !query_type.eq_ignore_ascii_case("INSERT")
                })
            }) {
                return Err("query_type must contain only SELECT or INSERT");
            }
        }

        Ok(())
    }

    fn classifier_has_condition(classifier: &ClassifierRequest) -> bool {
        [
            classifier.user.as_deref(),
            classifier.role.as_deref(),
            classifier.source_ip.as_deref(),
            classifier.db.as_deref(),
        ]
        .into_iter()
        .any(|value| value.is_some_and(|value| !value.trim().is_empty()))
            || classifier
                .query_type
                .as_ref()
                .is_some_and(|query_types| query_types.iter().any(|value| !value.trim().is_empty()))
    }

    pub(crate) fn build_alter_sqls(
        name: &str,
        req: &UpdateResourceGroupRequest,
    ) -> Result<Vec<String>> {
        Self::validate_update_request(req).map_err(anyhow::Error::msg)?;

        let resource_group = format!("ALTER RESOURCE GROUP {}", Self::quote_identifier(name));
        let mut set_clauses = Vec::new();

        if let Some(cpu_weight) = req.cpu_weight {
            set_clauses.push(format!("'cpu_weight' = '{}'", cpu_weight));
        }
        if let Some(exclusive_cpu_cores) = req.exclusive_cpu_cores {
            set_clauses.push(format!("'exclusive_cpu_cores' = '{}'", exclusive_cpu_cores));
        }
        if let Some(ref mem_limit) = req.mem_limit {
            set_clauses.push(format!("'mem_limit' = {}", Self::quote_literal(mem_limit)));
        }
        if let Some(big_query_cpu_second_limit) = req.big_query_cpu_second_limit {
            set_clauses
                .push(format!("'big_query_cpu_second_limit' = '{}'", big_query_cpu_second_limit));
        }
        if let Some(big_query_scan_rows_limit) = req.big_query_scan_rows_limit {
            set_clauses
                .push(format!("'big_query_scan_rows_limit' = '{}'", big_query_scan_rows_limit));
        }
        if let Some(ref big_query_mem_limit) = req.big_query_mem_limit {
            set_clauses.push(format!(
                "'big_query_mem_limit' = {}",
                Self::quote_literal(big_query_mem_limit)
            ));
        }
        if let Some(concurrency_limit) = req.concurrency_limit {
            set_clauses.push(format!("'concurrency_limit' = '{}'", concurrency_limit));
        }
        if let Some(ref spill_mem_limit_threshold) = req.spill_mem_limit_threshold {
            set_clauses.push(format!(
                "'spill_mem_limit_threshold' = {}",
                Self::quote_literal(spill_mem_limit_threshold)
            ));
        }

        let mut statements = Vec::new();
        // Add before dropping replacements so invalid additions preserve the existing classifier.
        if let Some(ref add_classifiers) = req.add_classifiers {
            if !add_classifiers.is_empty() {
                let classifiers: Vec<String> = add_classifiers
                    .iter()
                    .map(Self::build_classifier_clause)
                    .collect();
                statements.push(format!("{} ADD ({})", resource_group, classifiers.join(", ")));
            }
        }

        if !set_clauses.is_empty() {
            statements.push(format!("{} WITH ({})", resource_group, set_clauses.join(", ")));
        }

        if let Some(ref drop_ids) = req.drop_classifier_ids {
            if !drop_ids.is_empty() {
                let ids: Vec<String> = drop_ids.iter().map(|id| id.to_string()).collect();
                statements.push(format!("{} DROP ({})", resource_group, ids.join(", ")));
            }
        }

        Ok(statements)
    }

    fn build_classifier_clause(classifier: &ClassifierRequest) -> String {
        let mut conditions = Vec::new();

        if let Some(ref user) = classifier.user {
            conditions.push(format!("user={}", Self::quote_literal(user)));
        }
        if let Some(ref role) = classifier.role {
            conditions.push(format!("role={}", Self::quote_literal(role)));
        }
        if let Some(ref query_types) = classifier.query_type {
            if !query_types.is_empty() {
                let types: Vec<String> =
                    query_types.iter().map(|t| Self::quote_literal(t)).collect();
                conditions.push(format!("query_type IN ({})", types.join(", ")));
            }
        }
        if let Some(ref source_ip) = classifier.source_ip {
            conditions.push(format!("source_ip={}", Self::quote_literal(source_ip)));
        }
        if let Some(ref db) = classifier.db {
            conditions.push(format!("db={}", Self::quote_literal(db)));
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

    fn quote_literal(value: &str) -> String {
        format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
    }

    pub(crate) fn audit_table(audit_config: &AuditLogConfig) -> String {
        audit_config.full_table_name()
    }
}
