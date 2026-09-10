# 数据库存储抽象与 MySQL 支持 — 设计文档

> 状态：已完成（worktree: `/mnt/data/stellar-mysql`，分支: `feat/mysql-support`，基于 main@f6436ec）
>
> **交付结果**：
> - 运行时双后端（同一二进制，按 `database.url` 协议选择 sqlite:// / mysql://）
> - `db` 模块收口全部方言知识；业务代码只依赖 `DB: AppDb` 泛型
> - **真 MySQL 8.0.46 验证通过**：迁移全量执行、INSERT/自增 ID、upsert（2 插入→1 行）、INSERT IGNORE
> - 已知行为差异：MySQL 版外键约束真实生效（SQLite 内联 REFERENCES 被忽略）；业务 SQL 均以真实外键数据操作，无影响
> - 主工作区未提交的 ~1400 行改动未包含在本分支，合并时需处理 db/mod.rs、main.rs 的小冲突
>
> **实施关键决策（超出初版设计）**：
> 1. Rust bound 不跨签名传播（实测验证），能力 bound 由 `backend/macros` 的
>    `#[app_impl]`（impl 块）/`#[app_db]`（函数）属性宏在使用点注入，清单单一维护；
>    含 Option<T> Encode 直达约束与强枚举（ClusterType/DeploymentMode）能力
> 2. `SqlDialect::adapt_excluded` 处理 MySQL 复杂 SET 表达式中的 `excluded.col → VALUES(col)`
> 3. RETURNING id 统一改为 execute + LastInsertId（无额外往返）
> 4. migrations 双目录：MySQL 版由转换脚本生成并人工修正（TEXT key/DEFAULT→VARCHAR(255)、
>    partial index 去 WHERE、trigger→ON UPDATE CURRENT_TIMESTAMP、UPDATE 自引用子查询→派生表、
>    INSERT OR IGNORE→INSERT IGNORE）

## 1. 背景与目标

平台自身元数据存储当前硬编码 SQLite（`SqlitePool` 散布 25 个文件）。目标：
- 支持运行时按 `database.url` 协议选择 SQLite / MySQL（同一二进制，config.toml 驱动）
- 所有方言知识收口 `db` 模块（高内聚低耦合），业务代码只依赖一个泛型约束
- 利用的 Rust 语言特性：**泛型静态分发（trait 关联函数）代替 dyn trait**，零运行时开销、一份 SQL 代码两库通用

## 2. 方案决策（已评估并否决的备选）

| 方案 | 结论 |
|---|---|
| `sqlx::AnyPool` | ❌ 0.7 的 Any 驱动不支持 chrono 类型 Encode/Decode，代码大量用 chrono |
| `enum DbPool { Sqlite(..), MySql(..) }` | ❌ sqlx 查询类型绑定具体 DB，每个查询点都要 match，不可行 |
| Cargo feature 编译期二选一 + 类型别名 | ❌ 一个二进制只有一种后端，不满足 config.toml 运行时切换 |
| Repository-per-DB trait 对象 | ❌ SQL 是同一份字符串，两份实现违反 DRY，方法签名爆炸 |
| **泛型全链路 `AppState<DB: AppDb>`（采用）** | ✅ `sqlx::query()` 的 DB 由 executor 推断，同一份 SQL/FromRow 代码两库通用；全部具体化收口在 `main::run::<DB>()`，Send 校验在单态化处可判定 |

关键可行性依据：
- 项目**未使用** `query!`/`query_as!` 宏（编译期校验宏绑定单一 DB），全部运行时 SQL
- SQLite 与 MySQL 占位符同为 `?`
- chrono/bool/i64 的 Encode/Decode 两个后端都支持

## 3. 核心抽象（backend/src/db/，已完成）

```
db/
├── mod.rs       # AppDb trait、DatabaseKind、create_pool::<DB>()、find_migrations_dir
├── dialect.rs   # LastInsertId + SqlDialect（方言 SQL 片段，静态分发）
├── sqlite.rs    # impl AppDb for Sqlite（建文件 + PRAGMA×4）
└── mysql.rs     # impl AppDb for MySql（直连，charset 建议走 URL）
```

**AppDb 定义**（一个 bound 走天下）：

```rust
pub trait AppDb:
    Database<Connection: sqlx::migrate::Migrate, QueryResult: LastInsertId>
    + SqlDialect
    + Send + Sync + 'static + Sized
    // TODO: 还需追加 Arguments: Default（llm/repository.rs 动态 UPDATE 需要）
{
    fn connect(url: &str) -> impl Future<Output = sqlx::Result<Pool<Self>>> + Send;
    fn migrations_dir() -> &'static str;
}
// impl AppDb for Sqlite / MySql
```

**SqlDialect API**（`DB::upsert_suffix(...)` 静态分发，不造宏——trait 已是最简形态）：

```rust
pub trait SqlDialect: Database {
    fn upsert_suffix(conflict_keys: &[&str], set_cols: &[&str], extra_set: &[&str]) -> String;
    //   SQLite: ON CONFLICT(k) DO UPDATE SET c = excluded.c, <extra>
    //   MySQL:  ON DUPLICATE KEY UPDATE c = VALUES(c), <extra>   (conflict_keys 被忽略)
    fn insert_ignore() -> &'static str;   // "INSERT OR IGNORE" / "INSERT IGNORE"
    fn insert_replace() -> &'static str;  // "INSERT OR REPLACE" / "REPLACE"
}
pub trait LastInsertId { fn last_insert_id(&self) -> i64; }
// SqliteQueryResult.last_insert_rowid() / MySqlQueryResult.last_insert_id(u64 字段)
```

## 4. 分发链（单一具体化点）

```rust
// main.rs
match DatabaseKind::from_url(&config.database.url)? {
    DatabaseKind::Sqlite => run::<Sqlite>(config).await?,
    DatabaseKind::MySql  => run::<MySql>(config).await?,
}
// run<DB: AppDb>(config) 内：create_pool::<DB>() → AppState<DB> → Router<DB> → serve
// utoipa::path + 泛型 handler 的兼容性待编译验证（最大风险点）
```

## 5. 方言差异点全清单（改写时逐处处理）

### A. upsert `ON CONFLICT(...) DO UPDATE SET`（SQLite）→ `DB::upsert_suffix()`（7 处）
| 位置 | conflict_keys | set_cols | extra_set |
|---|---|---|---|
| services/system_function_service.rs:235 | cluster_id, function_id | category_order, display_order | updated_at = CURRENT_TIMESTAMP |
| services/system_function_service.rs:282 | cluster_id, function_id | （读代码确认） | （确认） |
| services/data_statistics_service.rs:362 | cluster_id | updated_at + 其余列 | 无 |
| services/user_service.rs:270 | user_id | organization_id | 无 |
| services/metrics_collector_service.rs:759 | cluster_id, snapshot_date | 全部列 | 无 |
| services/organization_service.rs:337 | user_id | organization_id | 无 |
| services/llm/repository.rs:510 | date, provider_id | （读代码确认） | （确认） |

### B. 自增主键读取 → `result.last_insert_id()`（LastInsertId trait）
- `services/auth_service.rs:50`
- `services/cluster_service.rs:130`
- `services/user_service.rs:134`
- `services/organization_service.rs:43,253`（2 处）
- `services/llm/repository.rs:~106`（create_provider）
- **RETURNING id 改 execute+rowid（等价、无额外往返）**：
  - `services/system_function_service.rs:154`
  - `services/permission_request_service.rs:63`

### C. INSERT OR IGNORE / INSERT OR REPLACE → `DB::insert_ignore()/insert_replace()`
- `services/organization_service.rs:389,404`（ignore）
- `services/llm/repository.rs:470`（replace）

### D. SQLite 函数 / 拼接 → Rust 端 chrono 计算
- `services/llm/repository.rs:472`：`datetime(CURRENT_TIMESTAMP, '+' || ? || ' hours')` → 计算 expires_at 后直接 bind

### E. 显式 Sqlite 类型（泛型化时替换）
- `Transaction<'_, Sqlite>`：user_service.rs ×6、organization_service.rs ×6 → `Transaction<'_, DB>`
- `sqlx::sqlite::SqliteRow`：permission_request_service.rs:734 `row_to_response(row)` → 泛型 `DB::Row`（或改 FromRow/query_as）
- `SqliteArguments::default()`：llm/repository.rs:123 → `<DB as Database>::Arguments::default()`（AppDb 加 `Arguments: Default`）
- casbin_service.rs:133 `pool: &sqlx::SqlitePool` → 方法级泛型 `<DB: AppDb>(pool: &Pool<DB>)`
- middleware/auth.rs：`AuthState` 泛型化 + `fetch_org_from_user_organizations(db: &Pool<DB>)`

### F. migrations 双方言目录（migrations/sqlite/ 现有 5 个文件移入 + migrations/mysql/ 改写版）
MySQL 改写要点：
- `INTEGER PRIMARY KEY AUTOINCREMENT` → `BIGINT AUTO_INCREMENT PRIMARY KEY`（**i64 decode 要求 BIGINT**，所有会读成 i64 的 INTEGER 列都建 BIGINT）
- `CREATE INDEX IF NOT EXISTS` → 去掉 IF NOT EXISTS（MySQL 8.0 不支持索引条件创建）
- `TIMESTAMP DEFAULT CURRENT_TIMESTAMP` → `DATETIME DEFAULT CURRENT_TIMESTAMP`
- `AUTOINCREMENT` 检查、`datetime()` 函数检查（initial_schema 与 add_query_execution_history 有）
- 老库迁移安全性：SQLite 目录路径变了 → sqlx migrations 表是 `_sqlx_migrations`，同一库文件里记录不变，**路径变化不影响已迁移库**（校验和按文件名匹配）

## 6. 机械替换规则（脚本处理 services）

```text
use sqlx::SqlitePool;                                  → use sqlx::Pool; + use crate::db::AppDb;
pub struct XxxService {                                → pub struct XxxService<DB: AppDb> {
impl XxxService {                                      → impl<DB: AppDb> XxxService<DB> {
pool: SqlitePool / db: SqlitePool                      → : Pool<DB>
db: Arc<SqlitePool>                                    → db: Arc<Pool<DB>>
pub fn new(pool: SqlitePool                            → pub fn new(pool: Pool<DB>
Transaction<'_, Sqlite> / sqlx::Sqlite                 → Transaction<'_, DB>
&SqlitePool                                            → &Pool<DB>
result.last_insert_rowid()                             → result.last_insert_id()
```

注意：`#[derive(Clone)]` 在泛型 struct 上会生成 `where DB: Clone` bound——Sqlite/MySql 是单元类型，自动满足 ✓。

## 7. 不改动 / 保持 SQLite 的部分

- `tests/`（common/mod.rs 的 INSERT OR IGNORE 等）——测试固定 SQLite 具体类型
- `mysql_client.rs` / `mysql_pool_manager.rs` ——那是连 StarRocks/Doris 集群的（mysql_async），与平台存储无关，**不得混淆**
- cluster_adapter/*（走 MySQLPoolManager）
- audit_log_service / baseline_service / materialized_view_service / resource_group_service / db_auth_query_service ——不持 SqlitePool，无改动

## 8. 验证与交付物

- `cargo check` / `cargo clippy -- --deny warnings --allow clippy::uninlined-format-args`（项目规约）
- `cargo test`（SQLite 路径全绿）
- dialect 单元测试（纯字符串断言，无需真实 MySQL）
- 可选：MYSQL_URL 环境变量驱动的 MySQL 集成测试（CI 无 MySQL 实例时 skip）
- 文档：README/CLAUDE.md 数据库配置说明 + conf/config.toml 示例
