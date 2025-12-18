-- ===========================================
-- Stellar - Unified Initial Database Schema
-- ===========================================
-- Purpose: Complete database initialization for new deployments
-- Created: 2025-12-18 (Merged from 7 migration files)
-- Source Files:
--   1. 20250125000000_unified_database_schema.sql - Core tables
--   2. 20250126000000_rbac_complete_permission_system.sql - RBAC system
--   3. 20250127000000_add_organization_system.sql - Organization system
--   4. 20250128000000_add_llm_tables.sql - LLM service
--   5. 20250130000000_fix_system_functions_proc_paths.sql - Fix function paths
--   6. 20251210000000_add_sql_diagnose_permission.sql - SQL diagnose permission
--   7. 20251214000000_merge_configs_and_blacklist.sql - FE configs and blacklist permissions

-- ========================================
-- SECTION 1: CORE TABLES
-- ========================================

-- ==============================================
-- 1.1 Users Table
-- ==============================================
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username VARCHAR(50) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    email VARCHAR(100),
    avatar VARCHAR(255),
    organization_id INTEGER NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_organization_id ON users(organization_id);

-- ==============================================
-- 1.2 Clusters Table
-- ==============================================
CREATE TABLE IF NOT EXISTS clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name VARCHAR(100) NOT NULL UNIQUE,
    description TEXT,
    fe_host VARCHAR(255) NOT NULL,
    fe_http_port INTEGER NOT NULL DEFAULT 8030,
    fe_query_port INTEGER NOT NULL DEFAULT 9030,
    username VARCHAR(100) NOT NULL,
    password_encrypted VARCHAR(255) NOT NULL,
    enable_ssl BOOLEAN DEFAULT 0,
    connection_timeout INTEGER DEFAULT 10,
    tags TEXT,
    catalog VARCHAR(100) DEFAULT 'default_catalog',
    deployment_mode VARCHAR(20) DEFAULT 'shared_nothing' NOT NULL,
    is_active BOOLEAN DEFAULT 0,
    organization_id INTEGER NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by INTEGER
);

CREATE INDEX IF NOT EXISTS idx_clusters_name ON clusters(name);
CREATE INDEX IF NOT EXISTS idx_clusters_is_active ON clusters(is_active);
CREATE INDEX IF NOT EXISTS idx_clusters_deployment_mode ON clusters(deployment_mode);
CREATE INDEX IF NOT EXISTS idx_clusters_organization_id ON clusters(organization_id);

-- ==============================================
-- 1.3 Monitor History Table
-- ==============================================
CREATE TABLE IF NOT EXISTS monitor_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id INTEGER NOT NULL,
    metric_name VARCHAR(100) NOT NULL,
    metric_value TEXT NOT NULL,
    collected_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_monitor_history_cluster_metric ON monitor_history(cluster_id, metric_name);
CREATE INDEX IF NOT EXISTS idx_monitor_history_collected_at ON monitor_history(collected_at);

-- ==============================================
-- 1.4 System Functions Table
-- ==============================================
CREATE TABLE IF NOT EXISTS system_functions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id INTEGER NULL,
    category_name TEXT NOT NULL,
    function_name TEXT NOT NULL,
    description TEXT NOT NULL,
    sql_query TEXT NOT NULL,
    display_order INTEGER NOT NULL DEFAULT 0,
    category_order INTEGER NOT NULL DEFAULT 0,
    is_favorited BOOLEAN NOT NULL DEFAULT FALSE,
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    created_by INTEGER NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (cluster_id) REFERENCES clusters (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_system_functions_cluster_id ON system_functions (cluster_id);
CREATE INDEX IF NOT EXISTS idx_system_functions_category_order ON system_functions (category_order);
CREATE INDEX IF NOT EXISTS idx_system_functions_display_order ON system_functions (display_order);
CREATE INDEX IF NOT EXISTS idx_system_functions_is_system ON system_functions (is_system);

-- ==============================================
-- 1.5 System Function Preferences Table
-- ==============================================
CREATE TABLE IF NOT EXISTS system_function_preferences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id INTEGER NOT NULL,
    function_id INTEGER NOT NULL,
    category_order INTEGER NOT NULL DEFAULT 0,
    display_order INTEGER NOT NULL DEFAULT 0,
    is_favorited BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(cluster_id, function_id),
    FOREIGN KEY (cluster_id) REFERENCES clusters (id) ON DELETE CASCADE,
    FOREIGN KEY (function_id) REFERENCES system_functions (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_preferences_cluster_id ON system_function_preferences (cluster_id);
CREATE INDEX IF NOT EXISTS idx_preferences_function_id ON system_function_preferences (function_id);
CREATE INDEX IF NOT EXISTS idx_preferences_cluster_function ON system_function_preferences (cluster_id, function_id);

-- ==============================================
-- 1.6 Metrics Snapshots Table
-- ==============================================
CREATE TABLE IF NOT EXISTS metrics_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id INTEGER NOT NULL,
    collected_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Query Performance Metrics
    qps REAL NOT NULL DEFAULT 0.0,
    rps REAL NOT NULL DEFAULT 0.0,
    query_latency_p50 REAL NOT NULL DEFAULT 0.0,
    query_latency_p95 REAL NOT NULL DEFAULT 0.0,
    query_latency_p99 REAL NOT NULL DEFAULT 0.0,
    query_total BIGINT NOT NULL DEFAULT 0,
    query_success BIGINT NOT NULL DEFAULT 0,
    query_error BIGINT NOT NULL DEFAULT 0,
    query_timeout BIGINT NOT NULL DEFAULT 0,

    -- Cluster Health Metrics
    backend_total INTEGER NOT NULL DEFAULT 0,
    backend_alive INTEGER NOT NULL DEFAULT 0,
    frontend_total INTEGER NOT NULL DEFAULT 0,
    frontend_alive INTEGER NOT NULL DEFAULT 0,

    -- Resource Usage Metrics
    total_cpu_usage REAL NOT NULL DEFAULT 0.0,
    avg_cpu_usage REAL NOT NULL DEFAULT 0.0,
    total_memory_usage REAL NOT NULL DEFAULT 0.0,
    avg_memory_usage REAL NOT NULL DEFAULT 0.0,
    disk_total_bytes BIGINT NOT NULL DEFAULT 0,
    disk_used_bytes BIGINT NOT NULL DEFAULT 0,
    disk_usage_pct REAL NOT NULL DEFAULT 0.0,

    -- Storage Metrics
    tablet_count BIGINT NOT NULL DEFAULT 0,
    max_compaction_score REAL NOT NULL DEFAULT 0.0,

    -- Transaction Metrics
    txn_running INTEGER NOT NULL DEFAULT 0,
    txn_success_total BIGINT NOT NULL DEFAULT 0,
    txn_failed_total BIGINT NOT NULL DEFAULT 0,

    -- Load Metrics
    load_running INTEGER NOT NULL DEFAULT 0,
    load_finished_total BIGINT NOT NULL DEFAULT 0,

    -- JVM Metrics (FE)
    jvm_heap_total BIGINT NOT NULL DEFAULT 0,
    jvm_heap_used BIGINT NOT NULL DEFAULT 0,
    jvm_heap_usage_pct REAL NOT NULL DEFAULT 0.0,
    jvm_thread_count INTEGER NOT NULL DEFAULT 0,

    -- Network Metrics (BE)
    network_bytes_sent_total BIGINT NOT NULL DEFAULT 0,
    network_bytes_received_total BIGINT NOT NULL DEFAULT 0,
    network_send_rate REAL NOT NULL DEFAULT 0.0,
    network_receive_rate REAL NOT NULL DEFAULT 0.0,

    -- IO Metrics (BE)
    io_read_bytes_total BIGINT NOT NULL DEFAULT 0,
    io_write_bytes_total BIGINT NOT NULL DEFAULT 0,
    io_read_rate REAL NOT NULL DEFAULT 0.0,
    io_write_rate REAL NOT NULL DEFAULT 0.0,

    -- Raw Metrics (JSON format for flexibility)
    raw_metrics TEXT,

    FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_metrics_snapshots_cluster_time ON metrics_snapshots(cluster_id, collected_at DESC);
CREATE INDEX IF NOT EXISTS idx_metrics_snapshots_time ON metrics_snapshots(collected_at DESC);

-- ==============================================
-- 1.7 Daily Snapshots Table
-- ==============================================
CREATE TABLE IF NOT EXISTS daily_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id INTEGER NOT NULL,
    snapshot_date DATE NOT NULL,

    -- Aggregated Query Statistics
    avg_qps REAL NOT NULL DEFAULT 0.0,
    max_qps REAL NOT NULL DEFAULT 0.0,
    min_qps REAL NOT NULL DEFAULT 0.0,
    avg_latency_p99 REAL NOT NULL DEFAULT 0.0,
    max_latency_p99 REAL NOT NULL DEFAULT 0.0,
    total_queries BIGINT NOT NULL DEFAULT 0,
    total_errors BIGINT NOT NULL DEFAULT 0,
    error_rate REAL NOT NULL DEFAULT 0.0,

    -- Aggregated Resource Statistics
    avg_cpu_usage REAL NOT NULL DEFAULT 0.0,
    max_cpu_usage REAL NOT NULL DEFAULT 0.0,
    avg_memory_usage REAL NOT NULL DEFAULT 0.0,
    max_memory_usage REAL NOT NULL DEFAULT 0.0,
    avg_disk_usage_pct REAL NOT NULL DEFAULT 0.0,
    max_disk_usage_pct REAL NOT NULL DEFAULT 0.0,

    -- Availability Statistics
    avg_backend_alive REAL NOT NULL DEFAULT 0.0,
    min_backend_alive INTEGER NOT NULL DEFAULT 0,
    total_downtime_seconds INTEGER NOT NULL DEFAULT 0,
    availability_pct REAL NOT NULL DEFAULT 100.0,

    -- Data Growth Statistics
    data_size_start BIGINT NOT NULL DEFAULT 0,
    data_size_end BIGINT NOT NULL DEFAULT 0,
    data_growth_bytes BIGINT NOT NULL DEFAULT 0,
    data_growth_rate REAL NOT NULL DEFAULT 0.0,

    FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE,
    UNIQUE(cluster_id, snapshot_date)
);

CREATE INDEX IF NOT EXISTS idx_daily_snapshots_cluster_date ON daily_snapshots(cluster_id, snapshot_date DESC);
CREATE INDEX IF NOT EXISTS idx_daily_snapshots_date ON daily_snapshots(snapshot_date DESC);

-- ==============================================
-- 1.8 Data Statistics Table
-- ==============================================
CREATE TABLE IF NOT EXISTS data_statistics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id INTEGER NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Database/Table Statistics
    database_count INTEGER NOT NULL DEFAULT 0,
    table_count INTEGER NOT NULL DEFAULT 0,
    total_data_size BIGINT NOT NULL DEFAULT 0,
    total_index_size BIGINT NOT NULL DEFAULT 0,

    -- Top Tables (JSON Array)
    top_tables_by_size TEXT,
    top_tables_by_access TEXT,

    -- Materialized View Statistics
    mv_total INTEGER NOT NULL DEFAULT 0,
    mv_running INTEGER NOT NULL DEFAULT 0,
    mv_failed INTEGER NOT NULL DEFAULT 0,
    mv_success INTEGER NOT NULL DEFAULT 0,

    -- Schema Change Statistics
    schema_change_running INTEGER NOT NULL DEFAULT 0,
    schema_change_pending INTEGER NOT NULL DEFAULT 0,
    schema_change_finished INTEGER NOT NULL DEFAULT 0,
    schema_change_failed INTEGER NOT NULL DEFAULT 0,

    -- Active Users Statistics
    active_users_1h INTEGER NOT NULL DEFAULT 0,
    active_users_24h INTEGER NOT NULL DEFAULT 0,
    unique_users TEXT,

    -- Query Statistics Cache
    slow_query_count_1h INTEGER NOT NULL DEFAULT 0,
    slow_query_count_24h INTEGER NOT NULL DEFAULT 0,

    FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE,
    UNIQUE(cluster_id)
);

CREATE INDEX IF NOT EXISTS idx_data_statistics_cluster ON data_statistics(cluster_id);
CREATE INDEX IF NOT EXISTS idx_data_statistics_updated ON data_statistics(updated_at DESC);

-- ========================================
-- SECTION 2: RBAC TABLES
-- ========================================

-- ==============================================
-- 2.1 Roles Table
-- ==============================================
CREATE TABLE IF NOT EXISTS roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code VARCHAR(50) UNIQUE NOT NULL,
    name VARCHAR(100) NOT NULL,
    description TEXT,
    is_system BOOLEAN DEFAULT 0,
    organization_id INTEGER NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_roles_code ON roles(code);
CREATE INDEX IF NOT EXISTS idx_roles_organization_id ON roles(organization_id);

-- ==============================================
-- 2.2 Permissions Table
-- ==============================================
CREATE TABLE IF NOT EXISTS permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code VARCHAR(100) UNIQUE NOT NULL,
    name VARCHAR(100) NOT NULL,
    type VARCHAR(20) NOT NULL,
    resource VARCHAR(100),
    action VARCHAR(50),
    parent_id INTEGER,
    description TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_id) REFERENCES permissions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_permissions_code ON permissions(code);
CREATE INDEX IF NOT EXISTS idx_permissions_type ON permissions(type);
CREATE INDEX IF NOT EXISTS idx_permissions_parent_id ON permissions(parent_id);

-- ==============================================
-- 2.3 Role Permissions Table
-- ==============================================
CREATE TABLE IF NOT EXISTS role_permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    role_id INTEGER NOT NULL,
    permission_id INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE,
    FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE,
    UNIQUE(role_id, permission_id)
);

CREATE INDEX IF NOT EXISTS idx_role_permissions_role_id ON role_permissions(role_id);
CREATE INDEX IF NOT EXISTS idx_role_permissions_permission_id ON role_permissions(permission_id);

-- ==============================================
-- 2.4 User Roles Table
-- ==============================================
CREATE TABLE IF NOT EXISTS user_roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    role_id INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE,
    UNIQUE(user_id, role_id)
);

CREATE INDEX IF NOT EXISTS idx_user_roles_user_id ON user_roles(user_id);
CREATE INDEX IF NOT EXISTS idx_user_roles_role_id ON user_roles(role_id);

-- ========================================
-- SECTION 3: ORGANIZATION TABLES
-- ========================================

-- ==============================================
-- 3.1 Organizations Table
-- ==============================================
CREATE TABLE IF NOT EXISTS organizations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code VARCHAR(50) UNIQUE NOT NULL,
    name VARCHAR(100) NOT NULL,
    description TEXT,
    is_system BOOLEAN DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_organizations_code ON organizations(code);
CREATE INDEX IF NOT EXISTS idx_organizations_is_system ON organizations(is_system);

-- ==============================================
-- 3.2 User Organizations Table
-- ==============================================
CREATE TABLE IF NOT EXISTS user_organizations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL UNIQUE,
    organization_id INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id) REFERENCES organizations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_user_organizations_user_id ON user_organizations(user_id);
CREATE INDEX IF NOT EXISTS idx_user_organizations_org_id ON user_organizations(organization_id);

-- ========================================
-- SECTION 4: LLM TABLES
-- ========================================

-- ==============================================
-- 4.1 LLM Providers Table
-- ==============================================
CREATE TABLE IF NOT EXISTS llm_providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    api_base TEXT NOT NULL,
    model_name TEXT NOT NULL,
    api_key_encrypted TEXT,
    is_active BOOLEAN DEFAULT FALSE,
    max_tokens INTEGER DEFAULT 4096,
    temperature REAL DEFAULT 0.3,
    timeout_seconds INTEGER DEFAULT 60,
    enabled BOOLEAN DEFAULT TRUE,
    priority INTEGER DEFAULT 100,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    CHECK (is_active IN (0, 1))
);

CREATE INDEX IF NOT EXISTS idx_llm_providers_active ON llm_providers(is_active) WHERE is_active = 1;
CREATE INDEX IF NOT EXISTS idx_llm_providers_enabled ON llm_providers(enabled, priority);

-- ==============================================
-- 4.2 LLM Analysis Sessions Table
-- ==============================================
CREATE TABLE IF NOT EXISTS llm_analysis_sessions (
    id TEXT PRIMARY KEY,
    provider_id INTEGER REFERENCES llm_providers(id),
    scenario TEXT NOT NULL DEFAULT 'root_cause_analysis',
    query_id TEXT NOT NULL,
    cluster_id INTEGER,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMP,
    input_tokens INTEGER,
    output_tokens INTEGER,
    latency_ms INTEGER,
    error_message TEXT,
    retry_count INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_llm_sessions_query ON llm_analysis_sessions(query_id);
CREATE INDEX IF NOT EXISTS idx_llm_sessions_status ON llm_analysis_sessions(status, created_at);
CREATE INDEX IF NOT EXISTS idx_llm_sessions_cluster ON llm_analysis_sessions(cluster_id, created_at);
CREATE INDEX IF NOT EXISTS idx_llm_sessions_scenario ON llm_analysis_sessions(scenario);

-- ==============================================
-- 4.3 LLM Analysis Requests Table
-- ==============================================
CREATE TABLE IF NOT EXISTS llm_analysis_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES llm_analysis_sessions(id) ON DELETE CASCADE,
    request_json TEXT NOT NULL,
    sql_hash TEXT NOT NULL,
    profile_hash TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_llm_requests_session ON llm_analysis_requests(session_id);
CREATE INDEX IF NOT EXISTS idx_llm_requests_hash ON llm_analysis_requests(sql_hash, profile_hash);

-- ==============================================
-- 4.4 LLM Analysis Results Table
-- ==============================================
CREATE TABLE IF NOT EXISTS llm_analysis_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL UNIQUE REFERENCES llm_analysis_sessions(id) ON DELETE CASCADE,
    root_causes_json TEXT NOT NULL DEFAULT '[]',
    causal_chains_json TEXT NOT NULL DEFAULT '[]',
    recommendations_json TEXT NOT NULL DEFAULT '[]',
    summary TEXT NOT NULL DEFAULT '',
    hidden_issues_json TEXT DEFAULT '[]',
    confidence_avg REAL,
    root_cause_count INTEGER,
    recommendation_count INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_llm_results_session ON llm_analysis_results(session_id);
CREATE INDEX IF NOT EXISTS idx_llm_results_confidence ON llm_analysis_results(confidence_avg);

-- ==============================================
-- 4.5 LLM Cache Table
-- ==============================================
CREATE TABLE IF NOT EXISTS llm_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cache_key TEXT NOT NULL UNIQUE,
    scenario TEXT NOT NULL DEFAULT 'root_cause_analysis',
    request_hash TEXT NOT NULL,
    response_json TEXT NOT NULL,
    hit_count INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    last_accessed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_llm_cache_key ON llm_cache(cache_key);
CREATE INDEX IF NOT EXISTS idx_llm_cache_expires ON llm_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_llm_cache_scenario ON llm_cache(scenario);

-- ==============================================
-- 4.6 LLM Usage Statistics Table
-- ==============================================
CREATE TABLE IF NOT EXISTS llm_usage_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL,
    provider_id INTEGER REFERENCES llm_providers(id),
    total_requests INTEGER DEFAULT 0,
    successful_requests INTEGER DEFAULT 0,
    failed_requests INTEGER DEFAULT 0,
    total_input_tokens INTEGER DEFAULT 0,
    total_output_tokens INTEGER DEFAULT 0,
    avg_latency_ms REAL,
    cache_hits INTEGER DEFAULT 0,
    estimated_cost_usd REAL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(date, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_llm_usage_date ON llm_usage_stats(date, provider_id);

-- ========================================
-- SECTION 5: INDEXES AND CONSTRAINTS
-- ========================================

-- Ensure only one active cluster per organization
CREATE UNIQUE INDEX IF NOT EXISTS idx_clusters_org_active ON clusters(organization_id)
WHERE is_active = 1;

-- ========================================
-- SECTION 6: TRIGGERS
-- ========================================

-- Update LLM providers timestamp
CREATE TRIGGER IF NOT EXISTS update_llm_providers_timestamp
AFTER UPDATE ON llm_providers
BEGIN
    UPDATE llm_providers SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

-- ========================================
-- SECTION 7: DEFAULT DATA
-- ========================================

-- ==============================================
-- 7.1 Insert Default Admin User
-- ==============================================
-- Password: admin (bcrypt hash with DEFAULT_COST=12)
INSERT OR IGNORE INTO users (username, password_hash, email)
VALUES ('admin', '$2b$12$LFxvzXbmyBPO9Zp.1MFU4OX3fb8kID8AHYHklokkZvgyzmHuRTc56', 'admin@example.com');

-- ==============================================
-- 7.2 Insert Default Roles
-- ==============================================
INSERT OR IGNORE INTO roles (code, name, description, is_system, organization_id) VALUES
('admin', '管理员', '拥有所有权限', 1, NULL),
('super_admin', '超级管理员', '拥有所有权限(跨组织)', 1, NULL);

-- ==============================================
-- 7.3 Insert Default Organization
-- ==============================================
INSERT OR IGNORE INTO organizations (code, name, description, is_system)
VALUES ('default_org', 'Default Organization', 'System default organization (built-in)', 1);

-- ==============================================
-- 7.4 Insert Default System Functions
-- ==============================================
INSERT OR IGNORE INTO system_functions (cluster_id, category_name, function_name, description, sql_query, display_order, category_order, is_favorited, is_system, created_by, created_at, updated_at) VALUES
-- Database Management
(NULL, '数据库管理', 'dbs', '数据库信息', 'HTTP_QUERY', 0, 0, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '数据库管理', 'catalog', 'Catalog信息', 'HTTP_QUERY', 1, 0, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- Cluster Information
(NULL, '集群信息', 'backends', 'Backend节点信息', 'HTTP_QUERY', 0, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '集群信息', 'frontends', 'Frontend节点信息', 'HTTP_QUERY', 1, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '集群信息', 'compute_nodes', '计算节点信息(CN)', 'HTTP_QUERY', 2, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '集群信息', 'brokers', 'Broker节点信息', 'HTTP_QUERY', 3, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '集群信息', 'statistic', '统计信息', 'HTTP_QUERY', 4, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '集群信息', 'monitor', '监控信息', 'HTTP_QUERY', 5, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '集群信息', 'cluster_balance', '集群均衡', 'HTTP_QUERY', 6, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '集群信息', 'historical_nodes', '历史节点', 'HTTP_QUERY', 7, 1, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- Transaction Management
(NULL, '事务管理', 'transactions', '事务信息', 'HTTP_QUERY', 0, 2, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- Task Management
(NULL, '任务管理', 'routine_loads', 'Routine Load任务', 'HTTP_QUERY', 0, 3, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '任务管理', 'stream_loads', 'Stream Load任务', 'HTTP_QUERY', 1, 3, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '任务管理', 'load_error_hub', 'Load错误信息', 'HTTP_QUERY', 2, 3, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '任务管理', 'tasks', '任务列表', 'HTTP_QUERY', 3, 3, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '任务管理', 'replications', '复制任务', 'HTTP_QUERY', 4, 3, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- Query Management
(NULL, '查询管理', 'current_queries', '当前查询', 'HTTP_QUERY', 0, 4, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '查询管理', 'global_current_queries', '全局当前查询', 'HTTP_QUERY', 1, 4, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '查询管理', 'current_backend_instances', '当前后端实例', 'HTTP_QUERY', 2, 4, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- Resource Management
(NULL, '资源管理', 'resources', '资源信息', 'HTTP_QUERY', 0, 5, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '资源管理', 'warehouses', '数据仓库信息', 'HTTP_QUERY', 1, 5, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- Storage Management
(NULL, '存储管理', 'compactions', '压缩任务信息', 'HTTP_QUERY', 0, 6, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
(NULL, '存储管理', 'colocation_group', 'Colocation Group', 'HTTP_QUERY', 1, 6, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- Job Management
(NULL, '作业管理', 'jobs', '作业信息', 'HTTP_QUERY', 0, 7, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
-- System Maintenance
(NULL, '系统维护', 'meta_recovery', '元数据恢复', 'HTTP_QUERY', 0, 8, false, true, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);

-- ==============================================
-- 7.5 Insert All Permissions (Menu + API)
-- ==============================================
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
-- Menu Permissions (14 + 3 new)
('menu:dashboard', '集群列表', 'menu', 'dashboard', 'view', '查看集群列表'),
('menu:overview', '集群概览', 'menu', 'overview', 'view', '查看集群概览'),
('menu:nodes', '节点管理', 'menu', 'nodes', 'view', '查看节点管理'),
('menu:nodes:frontends', 'Frontend节点', 'menu', 'nodes', 'view', '查看Frontend节点'),
('menu:nodes:backends', 'Backend节点', 'menu', 'nodes', 'view', '查看Backend节点'),
('menu:queries', '查询管理', 'menu', 'queries', 'view', '查看查询管理'),
('menu:queries:execution', '实时查询', 'menu', 'queries', 'view', '查看实时查询'),
('menu:queries:profiles', 'Profiles', 'menu', 'queries', 'view', '查看Profiles'),
('menu:queries:audit-logs', '审计日志', 'menu', 'queries', 'view', '查看审计日志'),
('menu:queries:blacklist', 'SQL黑名单', 'menu', 'queries', 'view', '查看SQL黑名单管理'),
('menu:materialized-views', '物化视图', 'menu', 'materialized-views', 'view', '查看物化视图'),
('menu:system-functions', '功能卡片', 'menu', 'system-functions', 'view', '查看功能卡片'),
('menu:sessions', '会话管理', 'menu', 'sessions', 'view', '查看会话管理'),
('menu:variables', '变量管理', 'menu', 'variables', 'view', '查看变量管理'),
('menu:system', '系统管理', 'menu', 'system', 'view', '系统管理菜单(父级)'),
('menu:system:users', '用户管理', 'menu', 'system:users', 'view', '查看用户管理'),
('menu:system:roles', '角色管理', 'menu', 'system:roles', 'view', '查看角色管理'),
('menu:system:organizations', '组织管理', 'menu', 'system:organizations', 'view', '组织管理菜单'),
('menu:system:llm', 'LLM管理', 'menu', 'system:llm', 'view', '查看LLM管理'),

-- API Permissions - Cluster Operations (10)
('api:clusters:list', '查询集群列表', 'api', 'clusters', 'list', 'GET /api/clusters'),
('api:clusters:create', '创建集群', 'api', 'clusters', 'create', 'POST /api/clusters'),
('api:clusters:get', '查看集群详情', 'api', 'clusters', 'get', 'GET /api/clusters/:id'),
('api:clusters:update', '更新集群', 'api', 'clusters', 'update', 'PUT /api/clusters/:id'),
('api:clusters:delete', '删除集群', 'api', 'clusters', 'delete', 'DELETE /api/clusters/:id'),
('api:clusters:activate', '激活集群', 'api', 'clusters', 'activate', 'PUT /api/clusters/:id/activate'),
('api:clusters:active', '获取活跃集群', 'api', 'clusters', 'active', 'GET /api/clusters/active'),
('api:clusters:health', '集群健康检查', 'api', 'clusters', 'health', 'GET /api/clusters/:id/health'),
('api:clusters:health:post', '集群健康检查POST', 'api', 'clusters', 'health:post', 'POST /api/clusters/:id/health'),
('api:clusters:health:test', '测试集群连接', 'api', 'clusters', 'health:test', 'POST /api/clusters/health/test'),

-- API Permissions - Cluster Overview (8)
('api:clusters:overview', '集群概览', 'api', 'clusters', 'overview', 'GET /api/clusters/overview'),
('api:clusters:overview:extended', '扩展集群概览', 'api', 'clusters', 'overview:extended', 'GET /api/clusters/overview/extended'),
('api:clusters:overview:health', '集群健康卡片', 'api', 'clusters', 'overview:health', 'GET /api/clusters/overview/health'),
('api:clusters:overview:performance', '性能趋势', 'api', 'clusters', 'overview:performance', 'GET /api/clusters/overview/performance'),
('api:clusters:overview:resources', '资源趋势', 'api', 'clusters', 'overview:resources', 'GET /api/clusters/overview/resources'),
('api:clusters:overview:data:stats', '数据统计', 'api', 'clusters', 'overview:data:stats', 'GET /api/clusters/overview/data-stats'),
('api:clusters:overview:capacity:prediction', '容量预测', 'api', 'clusters', 'overview:capacity:prediction', 'GET /api/clusters/overview/capacity-prediction'),
('api:clusters:overview:compaction:details', '压缩详情统计', 'api', 'clusters', 'overview:compaction:details', 'GET /api/clusters/overview/compaction-details'),

-- API Permissions - Nodes (3)
('api:clusters:backends', 'Backend节点列表', 'api', 'clusters', 'backends', 'GET /api/clusters/backends'),
('api:clusters:backends:delete', '删除Backend节点', 'api', 'clusters', 'backends:delete', 'DELETE /api/clusters/backends/:host/:port'),
('api:clusters:frontends', 'Frontend节点列表', 'api', 'clusters', 'frontends', 'GET /api/clusters/frontends'),

-- API Permissions - Query Management (11)
('api:clusters:catalogs', '查询Catalog列表', 'api', 'clusters', 'catalogs', 'GET /api/clusters/catalogs'),
('api:clusters:databases', '查询数据库列表', 'api', 'clusters', 'databases', 'GET /api/clusters/databases'),
('api:clusters:tables', '查询表列表', 'api', 'clusters', 'tables', 'GET /api/clusters/tables'),
('api:clusters:catalogs:databases', '查询Catalog和数据库树', 'api', 'clusters', 'catalogs:databases', 'GET /api/clusters/catalogs-databases'),
('api:clusters:queries', '查询管理', 'api', 'clusters', 'queries', 'GET /api/clusters/queries'),
('api:clusters:queries:execute', '执行查询', 'api', 'clusters', 'queries:execute', 'POST /api/clusters/queries/execute'),
('api:clusters:queries:kill', '终止查询', 'api', 'clusters', 'queries:kill', 'DELETE /api/clusters/queries/:id'),
('api:clusters:sql:diagnose', 'SQL诊断', 'api', 'clusters', 'sql:diagnose', 'POST /api/clusters/:cluster_id/sql/diagnose'),
('api:clusters:queries:history', '查询历史记录', 'api', 'clusters', 'queries:history', 'GET /api/clusters/queries/history'),
('api:clusters:queries:profile', '查询Profile详情', 'api', 'clusters', 'queries:profile', 'GET /api/clusters/queries/:query_id/profile'),
('api:clusters:profiles', '查询Profile列表', 'api', 'clusters', 'profiles', 'GET /api/clusters/profiles'),
('api:clusters:profiles:get', '查看Profile详情', 'api', 'clusters', 'profiles:get', 'GET /api/clusters/profiles/:query_id'),

-- API Permissions - Materialized Views (9)
('api:clusters:materialized_views', '物化视图列表', 'api', 'clusters', 'materialized_views', 'GET /api/clusters/materialized_views'),
('api:clusters:materialized_views:get', '查看物化视图详情', 'api', 'clusters', 'materialized_views:get', 'GET /api/clusters/materialized_views/:mv_name'),
('api:clusters:materialized_views:create', '创建物化视图', 'api', 'clusters', 'materialized_views:create', 'POST /api/clusters/materialized_views'),
('api:clusters:materialized_views:update', '更新物化视图', 'api', 'clusters', 'materialized_views:update', 'PUT /api/clusters/materialized_views/:name'),
('api:clusters:materialized_views:delete', '删除物化视图', 'api', 'clusters', 'materialized_views:delete', 'DELETE /api/clusters/materialized_views/:name'),
('api:clusters:materialized_views:ddl', '获取物化视图DDL', 'api', 'clusters', 'materialized_views:ddl', 'GET /api/clusters/materialized_views/:mv_name/ddl'),
('api:clusters:materialized_views:refresh', '刷新物化视图', 'api', 'clusters', 'materialized_views:refresh', 'POST /api/clusters/materialized_views/:mv_name/refresh'),
('api:clusters:materialized_views:cancel', '取消刷新物化视图', 'api', 'clusters', 'materialized_views:cancel', 'POST /api/clusters/materialized_views/:mv_name/cancel'),
('api:clusters:materialized_views:alter', '修改物化视图', 'api', 'clusters', 'materialized_views:alter', 'PUT /api/clusters/materialized_views/:mv_name'),

-- API Permissions - Sessions & Variables (5)
('api:clusters:sessions', '会话管理', 'api', 'clusters', 'sessions', 'GET /api/clusters/sessions'),
('api:clusters:sessions:kill', '终止会话', 'api', 'clusters', 'sessions:kill', 'DELETE /api/clusters/sessions/:id'),
('api:clusters:variables', '变量管理', 'api', 'clusters', 'variables', 'GET /api/clusters/variables'),
('api:clusters:variables:update', '更新变量', 'api', 'clusters', 'variables:update', 'PUT /api/clusters/variables/:name'),
('api:clusters:configs', '查看FE配置', 'api', 'clusters', 'configs', 'GET /api/clusters/configs'),

-- API Permissions - SQL Blacklist (4)
('api:clusters:sql:blacklist', '查询SQL黑名单列表', 'api', 'clusters', 'sql:blacklist', 'GET /api/clusters/sql-blacklist'),
('api:clusters:sql:blacklist:add', '添加SQL黑名单规则', 'api', 'clusters', 'sql:blacklist:add', 'POST /api/clusters/sql-blacklist'),
('api:clusters:sql:blacklist:delete', '删除SQL黑名单规则', 'api', 'clusters', 'sql:blacklist:delete', 'DELETE /api/clusters/sql-blacklist/:id'),

-- API Permissions - System Functions (37)
('api:clusters:system', '功能卡片', 'api', 'clusters', 'system', 'GET /api/clusters/system'),
('api:clusters:system:runtime_info', '查询运行时信息', 'api', 'clusters', 'system:runtime_info', 'GET /api/clusters/system/runtime_info'),
('api:clusters:system:brokers', '查询Broker节点', 'api', 'clusters', 'system:brokers', 'GET /api/clusters/system/brokers'),
('api:clusters:system:frontends', '查询Frontend节点', 'api', 'clusters', 'system:frontends', 'GET /api/clusters/system/frontends'),
('api:clusters:system:routine_loads', '查询Routine Load任务', 'api', 'clusters', 'system:routine_loads', 'GET /api/clusters/system/routine_loads'),
('api:clusters:system:catalog', '查询目录', 'api', 'clusters', 'system:catalog', 'GET /api/clusters/system/catalog'),
('api:clusters:system:colocation_group', '查询Colocation Group', 'api', 'clusters', 'system:colocation_group', 'GET /api/clusters/system/colocation_group'),
('api:clusters:system:cluster_balance', '查询集群平衡', 'api', 'clusters', 'system:cluster_balance', 'GET /api/clusters/system/cluster_balance'),
('api:clusters:system:load_error_hub', '查询加载错误信息', 'api', 'clusters', 'system:load_error_hub', 'GET /api/clusters/system/load_error_hub'),
('api:clusters:system:meta_recovery', '查询元数据恢复', 'api', 'clusters', 'system:meta_recovery', 'GET /api/clusters/system/meta_recovery'),
('api:clusters:system:global_current_queries', '查询全局当前查询', 'api', 'clusters', 'system:global_current_queries', 'GET /api/clusters/system/global_current_queries'),
('api:clusters:system:tasks', '查询系统任务', 'api', 'clusters', 'system:tasks', 'GET /api/clusters/system/tasks'),
('api:clusters:system:compute_nodes', '查询计算节点', 'api', 'clusters', 'system:compute_nodes', 'GET /api/clusters/system/compute_nodes'),
('api:clusters:system:statistic', '查询统计信息', 'api', 'clusters', 'system:statistic', 'GET /api/clusters/system/statistic'),
('api:clusters:system:jobs', '查询后台任务', 'api', 'clusters', 'system:jobs', 'GET /api/clusters/system/jobs'),
('api:clusters:system:warehouses', '查询仓库', 'api', 'clusters', 'system:warehouses', 'GET /api/clusters/system/warehouses'),
('api:clusters:system:resources', '查询资源', 'api', 'clusters', 'system:resources', 'GET /api/clusters/system/resources'),
('api:clusters:system:transactions', '查询事务', 'api', 'clusters', 'system:transactions', 'GET /api/clusters/system/transactions'),
('api:clusters:system:backends', '查询Backend节点', 'api', 'clusters', 'system:backends', 'GET /api/clusters/system/backends'),
('api:clusters:system:current_queries', '查询当前查询', 'api', 'clusters', 'system:current_queries', 'GET /api/clusters/system/current_queries'),
('api:clusters:system:stream_loads', '查询Stream Load任务', 'api', 'clusters', 'system:stream_loads', 'GET /api/clusters/system/stream_loads'),
('api:clusters:system:replications', '查询复制状态', 'api', 'clusters', 'system:replications', 'GET /api/clusters/system/replications'),
('api:clusters:system:dbs', '查询数据库', 'api', 'clusters', 'system:dbs', 'GET /api/clusters/system/dbs'),
('api:clusters:system:current_backend_instances', '查询当前Backend实例', 'api', 'clusters', 'system:current_backend_instances', 'GET /api/clusters/system/current_backend_instances'),
('api:clusters:system:historical_nodes', '查询历史节点', 'api', 'clusters', 'system:historical_nodes', 'GET /api/clusters/system/historical_nodes'),
('api:clusters:system:compactions', '查询压缩任务', 'api', 'clusters', 'system:compactions', 'GET /api/clusters/system/compactions'),
('api:clusters:system:monitor', '查询监控信息', 'api', 'clusters', 'system:monitor', 'GET /api/clusters/system/monitor'),
('api:clusters:system:functions', '查询系统函数列表', 'api', 'clusters', 'system:functions', 'GET /api/clusters/system-functions'),
('api:clusters:system:functions:create', '创建系统函数', 'api', 'clusters', 'system:functions:create', 'POST /api/clusters/system-functions'),
('api:clusters:system:functions:update', '更新系统函数', 'api', 'clusters', 'system:functions:update', 'PUT /api/clusters/system-functions/:function_id'),
('api:clusters:system:functions:delete', '删除系统函数', 'api', 'clusters', 'system:functions:delete', 'DELETE /api/clusters/system-functions/:function_id'),
('api:clusters:system:functions:orders', '更新系统函数顺序', 'api', 'clusters', 'system:functions:orders', 'PUT /api/clusters/system-functions/orders'),
('api:clusters:system:functions:execute', '执行系统函数', 'api', 'clusters', 'system:functions:execute', 'POST /api/clusters/system-functions/:function_id/execute'),
('api:clusters:system:functions:favorite', '切换系统函数收藏', 'api', 'clusters', 'system:functions:favorite', 'PUT /api/clusters/system-functions/:function_id/favorite'),
('api:system:functions:access-time', '更新系统函数访问时间', 'api', 'system', 'functions:access-time', 'PUT /api/system-functions/:function_name/access-time'),
('api:system:functions:category:delete', '删除系统函数分类', 'api', 'system', 'functions:category:delete', 'DELETE /api/system-functions/category/:category_name'),

-- API Permissions - RBAC Management (17)
('api:users:list', '查询用户列表', 'api', 'users', 'list', 'GET /api/users'),
('api:users:get', '查看用户详情', 'api', 'users', 'get', 'GET /api/users/:id'),
('api:users:create', '创建用户', 'api', 'users', 'create', 'POST /api/users'),
('api:users:update', '更新用户', 'api', 'users', 'update', 'PUT /api/users/:id'),
('api:users:delete', '删除用户', 'api', 'users', 'delete', 'DELETE /api/users/:id'),
('api:users:roles:get', '查看用户角色', 'api', 'users', 'roles:get', 'GET /api/users/:id/roles'),
('api:users:roles:assign', '分配用户角色', 'api', 'users', 'roles:assign', 'POST /api/users/:id/roles'),
('api:users:roles:remove', '移除用户角色', 'api', 'users', 'roles:remove', 'DELETE /api/users/:id/roles/:role_id'),
('api:roles:list', '查询角色列表', 'api', 'roles', 'list', 'GET /api/roles'),
('api:roles:get', '查看角色详情', 'api', 'roles', 'get', 'GET /api/roles/:id'),
('api:roles:create', '创建角色', 'api', 'roles', 'create', 'POST /api/roles'),
('api:roles:update', '更新角色', 'api', 'roles', 'update', 'PUT /api/roles/:id'),
('api:roles:delete', '删除角色', 'api', 'roles', 'delete', 'DELETE /api/roles/:id'),
('api:roles:permissions:get', '查看角色权限', 'api', 'roles', 'permissions:get', 'GET /api/roles/:id/permissions'),
('api:roles:permissions:update', '更新角色权限', 'api', 'roles', 'permissions:update', 'PUT /api/roles/:id/permissions'),
('api:permissions:list', '查询权限列表', 'api', 'permissions', 'list', 'GET /api/permissions'),
('api:permissions:menu', '查询菜单权限', 'api', 'permissions', 'menu', 'GET /api/permissions/menu'),
('api:permissions:api', '查询API权限', 'api', 'permissions', 'api', 'GET /api/permissions/api'),
('api:permissions:tree', '查询权限树', 'api', 'permissions', 'tree', 'GET /api/permissions/tree'),

-- API Permissions - Auth (3)
('api:auth:me', '获取当前用户信息', 'api', 'auth', 'me', 'GET /api/auth/me'),
('api:auth:me:update', '更新当前用户信息', 'api', 'auth', 'me:update', 'PUT /api/auth/me'),
('api:auth:permissions', '获取当前用户权限', 'api', 'auth', 'permissions', 'GET /api/auth/permissions'),

-- API Permissions - Organizations (5)
('api:organizations:list', '查询组织列表', 'api', 'organizations', 'list', 'GET /api/organizations'),
('api:organizations:get', '查看组织详情', 'api', 'organizations', 'get', 'GET /api/organizations/:id'),
('api:organizations:create', '创建组织', 'api', 'organizations', 'create', 'POST /api/organizations'),
('api:organizations:update', '更新组织', 'api', 'organizations', 'update', 'PUT /api/organizations/:id'),
('api:organizations:delete', '删除组织', 'api', 'organizations', 'delete', 'DELETE /api/organizations/:id'),

-- API Permissions - LLM (11)
('api:llm:status', 'LLM服务状态', 'api', 'llm', 'status', 'GET /api/llm/status'),
('api:llm:providers:list', 'LLM提供商列表', 'api', 'llm', 'providers:list', 'GET /api/llm/providers'),
('api:llm:providers:get', '查看LLM提供商', 'api', 'llm', 'providers:get', 'GET /api/llm/providers/:id'),
('api:llm:providers:active', '获取活跃LLM提供商', 'api', 'llm', 'providers:active', 'GET /api/llm/providers/active'),
('api:llm:providers:create', '创建LLM提供商', 'api', 'llm', 'providers:create', 'POST /api/llm/providers'),
('api:llm:providers:update', '更新LLM提供商', 'api', 'llm', 'providers:update', 'PUT /api/llm/providers/:id'),
('api:llm:providers:delete', '删除LLM提供商', 'api', 'llm', 'providers:delete', 'DELETE /api/llm/providers/:id'),
('api:llm:providers:activate', '激活LLM提供商', 'api', 'llm', 'providers:activate', 'POST /api/llm/providers/:id/activate'),
('api:llm:providers:deactivate', '停用LLM提供商', 'api', 'llm', 'providers:deactivate', 'POST /api/llm/providers/:id/deactivate'),
('api:llm:providers:test', '测试LLM连接', 'api', 'llm', 'providers:test', 'POST /api/llm/providers/:id/test'),
('api:llm:analyze:root-cause', 'LLM根因分析', 'api', 'llm', 'analyze:root-cause', 'POST /api/llm/analyze/root-cause');

-- ==============================================
-- 7.6 Insert Default LLM Provider
-- ==============================================
INSERT OR IGNORE INTO llm_providers (name, display_name, api_base, model_name, is_active, enabled, priority)
VALUES ('deepseek', 'DeepSeek Chat', 'https://api.deepseek.com/v1', 'deepseek-chat', FALSE, TRUE, 1);

-- ========================================
-- SECTION 8: PERMISSION RELATIONSHIPS
-- ========================================

-- ==============================================
-- 8.1 Set Parent ID for Menu Hierarchies
-- ==============================================
-- System menu children
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system')
WHERE code IN ('menu:system:users', 'menu:system:roles', 'menu:system:organizations', 'menu:system:llm');

-- Nodes menu children
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:nodes')
WHERE code IN ('menu:nodes:frontends', 'menu:nodes:backends');

-- Queries menu children
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:queries')
WHERE code IN ('menu:queries:execution', 'menu:queries:profiles', 'menu:queries:audit-logs', 'menu:queries:blacklist');

-- ==============================================
-- 8.2 Set Parent ID for API Permissions
-- ==============================================

-- Dashboard menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:dashboard')
WHERE code IN (
    'api:clusters:list', 'api:clusters:create', 'api:clusters:get', 'api:clusters:update',
    'api:clusters:delete', 'api:clusters:activate', 'api:clusters:active',
    'api:clusters:health', 'api:clusters:health:post', 'api:clusters:health:test'
);

-- Overview menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:overview')
WHERE code IN (
    'api:clusters:overview', 'api:clusters:overview:extended', 'api:clusters:overview:health',
    'api:clusters:overview:performance', 'api:clusters:overview:resources',
    'api:clusters:overview:data:stats', 'api:clusters:overview:capacity:prediction',
    'api:clusters:overview:compaction:details'
);

-- Backend nodes menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:nodes:backends')
WHERE code IN ('api:clusters:backends', 'api:clusters:backends:delete');

-- Frontend nodes menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:nodes:frontends')
WHERE code = 'api:clusters:frontends';

-- Queries execution menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:queries:execution')
WHERE code IN (
    'api:clusters:catalogs', 'api:clusters:databases', 'api:clusters:tables',
    'api:clusters:catalogs:databases', 'api:clusters:queries',
    'api:clusters:queries:execute', 'api:clusters:queries:kill', 'api:clusters:sql:diagnose'
);

-- Profiles menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:queries:profiles')
WHERE code IN ('api:clusters:profiles', 'api:clusters:profiles:get', 'api:clusters:queries:profile');

-- Audit logs menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:queries:audit-logs')
WHERE code = 'api:clusters:queries:history';

-- SQL Blacklist menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:queries:blacklist')
WHERE code IN ('api:clusters:sql:blacklist', 'api:clusters:sql:blacklist:add', 'api:clusters:sql:blacklist:delete');

-- Materialized views menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:materialized-views')
WHERE code LIKE 'api:clusters:materialized_views%';

-- Sessions menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:sessions')
WHERE code IN ('api:clusters:sessions', 'api:clusters:sessions:kill');

-- Variables menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:variables')
WHERE code IN ('api:clusters:variables', 'api:clusters:variables:update', 'api:clusters:configs');

-- System functions menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system-functions')
WHERE code IN (
    'api:clusters:system', 'api:clusters:system:runtime_info',
    'api:clusters:system:brokers', 'api:clusters:system:frontends', 'api:clusters:system:routine_loads',
    'api:clusters:system:catalog', 'api:clusters:system:colocation_group', 'api:clusters:system:cluster_balance',
    'api:clusters:system:load_error_hub', 'api:clusters:system:meta_recovery', 'api:clusters:system:global_current_queries',
    'api:clusters:system:tasks', 'api:clusters:system:compute_nodes', 'api:clusters:system:statistic',
    'api:clusters:system:jobs', 'api:clusters:system:warehouses', 'api:clusters:system:resources',
    'api:clusters:system:monitor', 'api:clusters:system:transactions', 'api:clusters:system:backends',
    'api:clusters:system:current_queries', 'api:clusters:system:stream_loads', 'api:clusters:system:replications',
    'api:clusters:system:dbs', 'api:clusters:system:current_backend_instances', 'api:clusters:system:historical_nodes',
    'api:clusters:system:compactions', 'api:clusters:system:functions', 'api:clusters:system:functions:create',
    'api:clusters:system:functions:update', 'api:clusters:system:functions:delete', 'api:clusters:system:functions:orders',
    'api:clusters:system:functions:execute', 'api:clusters:system:functions:favorite',
    'api:system:functions:access-time', 'api:system:functions:category:delete'
);

-- Users menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system:users')
WHERE code IN (
    'api:users:list', 'api:users:get', 'api:users:create', 'api:users:update', 'api:users:delete',
    'api:users:roles:get', 'api:users:roles:assign', 'api:users:roles:remove'
);

-- Roles menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system:roles')
WHERE code IN (
    'api:roles:list', 'api:roles:get', 'api:roles:create', 'api:roles:update', 'api:roles:delete',
    'api:roles:permissions:get', 'api:roles:permissions:update',
    'api:permissions:list', 'api:permissions:menu', 'api:permissions:api', 'api:permissions:tree'
);

-- Organizations menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system:organizations')
WHERE code IN (
    'api:organizations:list', 'api:organizations:get', 'api:organizations:create',
    'api:organizations:update', 'api:organizations:delete'
);

-- LLM menu APIs
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system:llm')
WHERE code LIKE 'api:llm:%';

-- ========================================
-- SECTION 9: ROLE ASSIGNMENTS
-- ========================================

-- ==============================================
-- 9.1 Assign All Permissions to Admin Role
-- ==============================================
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions;

-- ==============================================
-- 9.2 Assign All Permissions to Super Admin Role
-- ==============================================
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions;

-- ==============================================
-- 9.3 Assign Admin Role to Default Admin User
-- ==============================================
INSERT OR IGNORE INTO user_roles (user_id, role_id)
SELECT u.id, (SELECT id FROM roles WHERE code='admin')
FROM users u
WHERE u.username = 'admin'
LIMIT 1;

-- ==============================================
-- 9.4 Assign Super Admin Role to Default Admin User
-- ==============================================
INSERT OR IGNORE INTO user_roles (user_id, role_id)
SELECT u.id, (SELECT id FROM roles WHERE code='super_admin')
FROM users u
WHERE u.username = 'admin'
LIMIT 1;

-- ==============================================
-- 9.5 Map Admin User to Default Organization
-- ==============================================
INSERT OR IGNORE INTO user_organizations (user_id, organization_id)
SELECT u.id, (SELECT id FROM organizations WHERE code = 'default_org')
FROM users u;

-- ========================================
-- SECTION 10: DATA CONSISTENCY
-- ========================================

-- ==============================================
-- 10.1 Update User Organization IDs
-- ==============================================
UPDATE users
SET organization_id = (SELECT id FROM organizations WHERE code = 'default_org')
WHERE organization_id IS NULL;

-- ==============================================
-- 10.2 Update Cluster Organization IDs
-- ==============================================
UPDATE clusters
SET organization_id = (SELECT id FROM organizations WHERE code = 'default_org')
WHERE organization_id IS NULL;

-- ==============================================
-- 10.3 Update Role Organization IDs
-- ==============================================
-- System roles should have NULL organization_id
UPDATE roles
SET organization_id = NULL
WHERE is_system = 1;

-- Non-system roles get default_org
UPDATE roles
SET organization_id = (SELECT id FROM organizations WHERE code = 'default_org')
WHERE (organization_id IS NULL) AND (is_system = 0);

-- ==============================================
-- 10.4 Auto-grant Parent Menu Permissions
-- ==============================================
-- Grant menu:system to roles that have system child menus
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT DISTINCT rp.role_id, (SELECT id FROM permissions WHERE code = 'menu:system')
FROM role_permissions rp
JOIN permissions p ON rp.permission_id = p.id
WHERE p.code IN ('menu:system:users', 'menu:system:roles', 'menu:system:organizations', 'menu:system:llm');

-- Grant menu:nodes to roles that have node child menus
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT DISTINCT rp.role_id, (SELECT id FROM permissions WHERE code = 'menu:nodes')
FROM role_permissions rp
JOIN permissions p ON rp.permission_id = p.id
WHERE p.code IN ('menu:nodes:frontends', 'menu:nodes:backends');

-- Grant menu:queries to roles that have query child menus
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT DISTINCT rp.role_id, (SELECT id FROM permissions WHERE code = 'menu:queries')
FROM role_permissions rp
JOIN permissions p ON rp.permission_id = p.id
WHERE p.code IN ('menu:queries:execution', 'menu:queries:profiles', 'menu:queries:audit-logs', 'menu:queries:blacklist');

-- ========================================
-- MIGRATION COMPLETE
-- ========================================
-- Summary:
--
-- Tables Created (21):
--   Core Tables (8):
--     1. users - User authentication and profiles
--     2. clusters - Cluster configuration and management
--     3. monitor_history - Legacy monitoring (compatibility)
--     4. system_functions - System function definitions
--     5. system_function_preferences - User preferences for functions
--     6. metrics_snapshots - Real-time metrics (30s, 7-day retention)
--     7. daily_snapshots - Daily aggregations (90-day retention)
--     8. data_statistics - Cached statistics (on-demand update)
--
--   RBAC Tables (4):
--     9. roles - Role definitions
--     10. permissions - Permission definitions
--     11. role_permissions - Role-Permission mappings
--     12. user_roles - User-Role mappings
--
--   Organization Tables (2):
--     13. organizations - Organization definitions
--     14. user_organizations - User-Organization mappings
--
--   LLM Tables (6):
--     15. llm_providers - LLM provider configuration
--     16. llm_analysis_sessions - Analysis session tracking
--     17. llm_analysis_requests - Request data storage
--     18. llm_analysis_results - Analysis results
--     19. llm_cache - Response cache
--     20. llm_usage_stats - Usage statistics
--
--   System Function Tables (1):
--     21. system_function_preferences - Already counted above
--
-- Default Data:
--   - 1 admin user (username: admin, password: admin)
--   - 2 system roles (admin, super_admin)
--   - 1 default organization (default_org)
--   - 25 system functions across 8 categories
--   - 19 menu permissions (including hierarchies)
--   - 143 API permissions (all features including SQL diagnose and blacklist)
--   - 1 LLM provider (DeepSeek, inactive by default)
--   - All permissions assigned to admin and super_admin roles
--   - Admin user mapped to both admin and super_admin roles
--   - Admin user mapped to default_org
--
-- Permission Coverage:
--   Menu Permissions (19):
--     - Dashboard, Overview, Nodes (parent + children), Queries (parent + children + blacklist)
--     - Materialized Views, System Functions, Sessions, Variables
--     - System Management (parent + users + roles + organizations + llm)
--
--   API Permissions (143):
--     - Cluster CRUD & Health (10)
--     - Cluster Overview (8)
--     - Nodes Management (3)
--     - Query Management (12 including SQL diagnose)
--     - Materialized Views (9)
--     - Sessions & Variables (5 including FE configs)
--     - SQL Blacklist (3)
--     - System Functions (37)
--     - RBAC Management (19)
--     - Auth (3)
--     - Organizations (5)
--     - LLM Management (11)
--
-- Next Steps:
--   1. Run this migration: cargo sqlx migrate run
--   2. Start backend service to activate MetricsCollectorService
--   3. Access application at configured port
--   4. Login with admin/admin credentials
--   5. Configure LLM provider if needed for SQL diagnosis feature
