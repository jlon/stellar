# StarRocks 物理机部署与生命周期管理设计（修订版）

> **状态**：设计评审修订稿。实施前必须满足第 3 节的前置条件和第 15 节的验收门禁。
>
> **目标**：在 Stellar 中安全地部署并管理 Linux x86_64 物理机/虚机上的 StarRocks。部署成功后，监控、查询和集群详情继续复用现有 `clusters` 体系。
>
> **P0 定位**：可审计、可恢复的 shared-nothing 新建部署与只读接管，不承诺完整生产生命周期自动化。

## 1. 结论与范围

### 1.1 保留的核心决策

- **不引入 Agent 和 Ansible 运行时**：Stellar 后端通过系统 OpenSSH 下发受控命令，降低常驻组件、端口和运维面。
- **不建立并行的业务集群体系**：部署成功后创建并关联现有 `clusters` 记录，现有 `ClusterService`、`ClusterAdapter`、监控和权限体系继续使用。
- **任务和事件落库**：前端轮询任务与事件；不以 WebSocket/SSE 作为 P0 前提。
- **失败不自动回滚**：部署或运维失败时保留现场和证据，避免错误删除元数据或数据目录。

### 1.2 P0：只做以下能力

1. 主机资产登记、只支持 **SSH 私钥** 的凭据登记、SSH host key 指纹确认。
2. StarRocks **shared-nothing** 新建部署：Leader FE、Follower FE、BE。
3. 安装包缓存、强制 SHA-256 校验、目标主机二次校验。
4. 预检、部署任务、事件查询、失败现场说明。
5. 部署成功后注册现有 `clusters`，并执行现有健康检查。
6. 只读接管：从已有 FE 导入观测到的节点信息，不执行远程生命周期操作。

### 1.3 明确不做

| 阶段 | 不做项 | 原因 |
|---|---|---|
| P0 | SSH 密码认证 | `ssh -o BatchMode=yes` 无法非交互式使用密码；避免引入 `sshpass` 或不安全的 `SSH_ASKPASS`。 |
| P0 | 自动修改 `sysctl`、`ulimit`、防火墙、系统用户或 systemd | 这些是主机全局或 root 级变更，必须有独立的权限模型和审批。 |
| P0 | shared-data、存储卷创建 | 存储卷含云存储凭据，不能放进任务 payload。 |
| P0 | 启停、扩缩容、配置变更、升级 | 先完成可恢复的任务框架、主机绑定与版本能力矩阵。 |
| P0 | 可写接管 | 仅靠 `SHOW PROC` 无法可靠获得安装路径、运行用户、SSH 指纹和真实配置。 |
| 全部阶段 | Agent、K8s 部署、通用 DAG 编排、自动数据回滚、Doris 物理机部署 | 当前需求之外。 |

P1 进入受控启停、滚动重启和 scale-out；P2 才进入 shared-data、缩容、静态配置、升级和可写接管。

## 2. 与现有仓库的边界

以下事实决定实现不能直接照搬旧方案：

1. `clusters.name` 在现有 SQLite schema 中全局唯一，`ClusterService::create_cluster` 也按全局名称检查。因此 P0 的 `sr_managed_clusters.name` 同样使用全局唯一；不能只做组织内唯一后再注册失败。
2. `MySQLPoolManager` 仅以 `cluster.id` 缓存连接池；现有临时连接测试使用 `id = 0`。部署尚未注册到 `clusters` 前，**不得**复用该全局连接池，否则不同操作可能共用错误连接或旧密码。
3. 现有 `password_encrypted` / `admin_password_encrypted` 字段当前并不代表已完成加密。物理机能力接入前，必须先提供独立的凭据加密服务；不得把明文密码当作“已加密字段”存储。
4. 当前权限提取器只识别 `roles`、`permissions`、`users`、`clusters`。新增 `/api/sr-ops/*` 路由必须同步增加权限提取规则，否则路由虽然受 JWT 保护，但不会经过 Casbin 动作授权。
5. `Cargo.toml` 没有直接声明 `futures`。P0 的并发实现使用已有 Tokio 的 `JoinSet`、`Semaphore` 和 `tokio::process`，不依赖 transitive dependency。

## 3. 运行模型与前置条件

### 3.1 控制面和主机责任

- Stellar 后端所在机器是控制面，必须安装并可执行 `ssh`、`scp`；其本地缓存目录必须仅对 Stellar 运行账号可读写。
- P0 依赖现有 SQLite 单实例部署约束：同一时刻只能有一个活动 Stellar 实例执行 task runner，且 SQLite 数据库、包缓存和临时工作目录位于持久本地磁盘。
- 目标主机必须是 Linux x86_64，已安装并启动 SSH 服务，且可被控制面连通。
- P0 使用预先创建的非 root 服务账号。该账号必须拥有安装目录、元数据目录和数据目录的读写权限。
- P0 不代替主机管理员完成 Java、`ulimit`、时钟同步、系统参数、防火墙和开机自启设置；预检只报告差异和修复建议。
- P0 采用保守资源策略：**一台物理主机只能分配给一个 P0 managed cluster**。同一集群可以在同一主机共置 FE 与 BE；跨集群复用主机留到引入端口/路径资源预占模型后再支持。

### 3.2 网络与身份

- 每个节点必须显式选择一个 `advertise_host`（IP 或 FQDN）和对应的 `priority_networks`/FQDN 模式；不能从 SSH 目标地址隐式推导多网卡节点的对外地址。
- P0 预检必须覆盖：控制面到 SSH 端口、所有节点间的 FE/BE 端口矩阵和所选 `advertise_host` 的可达性。对象存储与 CN 连通性仅在 P2 shared-data 预检中检查。
- 对于首次登记的主机，Stellar 展示 host key fingerprint；操作员必须通过可信带外渠道确认后保存。任务执行使用独立 `known_hosts` 文件和 `StrictHostKeyChecking=yes`，**禁止** `accept-new`。

### 3.3 拓扑策略

- 向导必须区分开发/验证拓扑和高可用拓扑。单 FE 或单 BE 仅允许在明确确认“非 HA”后创建。
- 单 BE 时在 `fe.conf` 设置 `default_replication_num=1`；高可用拓扑由独立规则校验 FE 数量、BE 数量和故障域，而不是仅检查主机数量。
- 已选择的 StarRocks 版本必须落在产品维护的“已验证版本矩阵”内。版本不在矩阵内时，P0 拒绝执行部署。

### 3.4 控制面安全配置

部署功能启用前，控制面必须配置以下环境变量；不配置时凭据创建和部署提交会被拒绝：

```bash
# 独立于 JWT 的 16 字符以上高熵密钥；变更前必须完成凭据轮换。
APP_SR_PHYSICAL_ENCRYPTION_KEY='replace-with-a-random-secret'

# 仅允许从明确审批的制品站点下载；逗号分隔主机名，不接受重定向。
APP_SR_PHYSICAL_PACKAGE_ALLOWED_HOSTS='packages.example.com,artifacts.example.net'

# 仅允许已验证的版本，逗号分隔。
APP_SR_PHYSICAL_SUPPORTED_VERSIONS='3.3.9,3.4.4'

# 可选，默认 data/sr-physical；目录仅允许 Stellar 运行账号读写。
APP_SR_PHYSICAL_CACHE_DIR='/var/lib/stellar/sr-physical'
```

包地址使用 HTTPS，且 host 必须精确匹配允许列表。控制面不从任意用户提供的 URL 下载制品。

## 4. 总体架构

```text
handlers/sr_physical.rs                 # HTTP 层：鉴权、DTO 校验、提交任务
services/sr_physical/
  ├── mod.rs                            # SrPhysicalService
  ├── task_runner.rs                    # DB lease、领取任务、启动恢复、超时
  ├── executor.rs                       # SshExecutor + OpenSshExecutor
  ├── operation_mysql.rs                # 操作期直连，不使用全局 MySQLPoolManager
  ├── credentials.rs                    # 独立 AEAD 凭据加密/解密和脱敏
  ├── precheck.rs                       # 只读预检与资源预占
  ├── templates.rs                      # 纯函数模板与原子配置写入内容
  ├── deploy.rs                         # P0 shared-nothing 部署编排
  ├── adopt.rs                          # 只读接管
  └── tests.rs
models/sr_physical.rs
migrations/2026xxxx_add_sr_physical.sql
```

关键约束：

- `operation_mysql.rs` 使用操作范围内的直连客户端，连接身份来自加密凭据引用；任务完成后关闭连接。`MySQLPoolManager` 只在 `clusters` 成功注册后用于常规产品能力。
- 每个 SSH 子进程必须有连接、执行和总任务超时；任务失败或服务关闭时回收子进程。不得在请求处理线程中长期等待部署完成。
- 远程命令只能由固定模板生成。所有参数经过类型校验和 POSIX shell 单引号转义；不得接收用户自由脚本、原始 shell 片段或原始 SQL。

## 5. 数据模型

### 5.1 设计规则

1. 所有资源带 `organization_id`，并带指向 `organizations(id)` 的外键。
2. SQLite 无法在 `UNIQUE` 表约束中使用 `COALESCE` 等表达式；节点使用必填的标准化 `service_port`，不使用表达式唯一约束。
3. 由于 SQLite 外键不能表达“所有被引用记录必须属于同一组织”，创建和更新操作必须在同一事务内校验 host、credential、package、managed cluster 和关联 cluster 的组织一致性。
4. task 的 payload、result 和 event 不保存任何明文密钥、密码、私钥、令牌或存储卷凭据。

### 5.2 核心表

```sql
-- 主机资产。ssh_target 可以是 IP 或 FQDN；host key 必须已确认。
CREATE TABLE physical_hosts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  organization_id INTEGER NOT NULL REFERENCES organizations(id),
  hostname TEXT NOT NULL,
  ssh_target TEXT NOT NULL,
  ssh_port INTEGER NOT NULL DEFAULT 22 CHECK (ssh_port BETWEEN 1 AND 65535),
  -- host_key 保存算法和 Base64 公钥，用于生成任务专属 known_hosts；fingerprint 仅用于人工核验展示。
  host_key TEXT NOT NULL,
  host_key_fingerprint TEXT NOT NULL,
  os_info TEXT,
  cpu_cores INTEGER,
  memory_gb INTEGER,
  disk_gb INTEGER,
  status TEXT NOT NULL DEFAULT 'unknown'
    CHECK (status IN ('online', 'offline', 'unknown')),
  labels_json TEXT,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(organization_id, ssh_target, ssh_port)
);

-- P0 仅支持 key；ciphertext、nonce、key_version 由独立密钥管理服务处理。
CREATE TABLE ssh_credentials (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  organization_id INTEGER NOT NULL REFERENCES organizations(id),
  name TEXT NOT NULL,
  username TEXT NOT NULL,
  auth_type TEXT NOT NULL CHECK (auth_type = 'key'),
  algorithm TEXT NOT NULL DEFAULT 'aes-256-gcm',
  key_version INTEGER NOT NULL,
  secret_nonce BLOB NOT NULL,
  secret_ciphertext BLOB NOT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(organization_id, name)
);

-- bootstrap/operator 数据库凭据。密文格式与 ssh_credentials 相同，且不出现在 API 响应中。
CREATE TABLE sr_database_credentials (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  organization_id INTEGER NOT NULL REFERENCES organizations(id),
  name TEXT NOT NULL,
  username TEXT NOT NULL,
  algorithm TEXT NOT NULL DEFAULT 'aes-256-gcm',
  key_version INTEGER NOT NULL,
  secret_nonce BLOB NOT NULL,
  secret_ciphertext BLOB NOT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(organization_id, name)
);

-- 安装包必须具备不可变校验值；缓存按 SHA-256 管理。
CREATE TABLE sr_packages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  organization_id INTEGER NOT NULL REFERENCES organizations(id),
  version TEXT NOT NULL,
  package_url TEXT NOT NULL,
  sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
  cached_path TEXT,
  size_bytes INTEGER,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'cached', 'failed')),
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(organization_id, version, sha256)
);

-- P0 名称与现有 clusters.name 对齐，使用全局唯一。
CREATE TABLE sr_managed_clusters (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  organization_id INTEGER NOT NULL REFERENCES organizations(id),
  name TEXT NOT NULL UNIQUE,
  deployment_mode TEXT NOT NULL DEFAULT 'shared_nothing'
    CHECK (deployment_mode = 'shared_nothing'),
  sr_version TEXT NOT NULL,
  cluster_id INTEGER REFERENCES clusters(id) ON DELETE SET NULL,
  install_dir TEXT NOT NULL,
  ssh_credential_id INTEGER NOT NULL REFERENCES ssh_credentials(id),
  package_id INTEGER NOT NULL REFERENCES sr_packages(id),
  operator_credential_id INTEGER REFERENCES sr_database_credentials(id),
  status TEXT NOT NULL DEFAULT 'planning'
    CHECK (status IN ('planning', 'deploying', 'running', 'failed', 'adopted_read_only')),
  created_by INTEGER NOT NULL REFERENCES users(id),
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- P0 的保守资源策略：一个物理主机只归属一个 managed cluster。
CREATE TABLE sr_host_allocations (
  host_id INTEGER PRIMARY KEY REFERENCES physical_hosts(id) ON DELETE CASCADE,
  managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sr_cluster_nodes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
  host_id INTEGER NOT NULL REFERENCES physical_hosts(id),
  role TEXT NOT NULL CHECK (role IN ('fe', 'be')),
  fe_role TEXT CHECK (fe_role IN ('leader', 'follower', 'observer')),
  advertise_host TEXT NOT NULL,
  -- FE 使用 edit_log_port；BE 使用 heartbeat_service_port。
  service_port INTEGER NOT NULL CHECK (service_port BETWEEN 1 AND 65535),
  http_port INTEGER,
  query_port INTEGER,
  rpc_port INTEGER,
  brpc_port INTEGER,
  webserver_port INTEGER,
  meta_dir TEXT,
  storage_dir TEXT,
  status TEXT NOT NULL DEFAULT 'planned'
    CHECK (status IN ('planned', 'installed', 'running', 'stopped', 'failed', 'removed')),
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(managed_cluster_id, host_id, role, service_port)
);

CREATE TABLE sr_operation_tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  organization_id INTEGER NOT NULL REFERENCES organizations(id),
  managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
  bootstrap_credential_id INTEGER REFERENCES sr_database_credentials(id),
  task_type TEXT NOT NULL CHECK (task_type IN ('deploy', 'adopt_read_only')),
  -- 只保存经过校验的非敏感计划快照和资源 ID。
  payload_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'interrupted', 'cancelled')),
  current_step TEXT,
  lease_token TEXT,
  lease_expires_at TIMESTAMP,
  error_message TEXT,
  result_json TEXT,
  created_by INTEGER NOT NULL REFERENCES users(id),
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  started_at TIMESTAMP,
  finished_at TIMESTAMP
);

-- SQLite 支持 partial index；保证同一集群同时最多一个 running 任务。
CREATE UNIQUE INDEX idx_sr_one_running_task
  ON sr_operation_tasks(managed_cluster_id)
  WHERE status = 'running';

CREATE TABLE sr_operation_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id INTEGER NOT NULL REFERENCES sr_operation_tasks(id) ON DELETE CASCADE,
  step TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'skipped')),
  node_id INTEGER REFERENCES sr_cluster_nodes(id),
  message TEXT NOT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

数据库凭据（bootstrap/root/operator）保存于 `sr_database_credentials`，并以 ID 形式被任务和 managed cluster 引用。它们不写入 `sr_operation_tasks.payload_json`，也不通过任务查询接口返回。注册到既有 `clusters` 前，必须先完成既有 cluster 密码字段的安全存储改造。

## 6. 安全设计

### 6.1 凭据与密钥

- 加密根密钥独立于 `auth.jwt_secret`，支持 key version 和轮换；每条密文使用随机 nonce，并以 `organization_id`、凭据 ID、字段类型作为 AEAD AAD。
- API 永不返回密文、私钥或密码；凭据删除前检查是否被 managed cluster 或待运行任务引用。
- 私钥只写入任务专属的 0700 临时目录，文件权限 0600；任务完成、失败、服务启动恢复时均清理。进程崩溃后残留目录由启动恢复逻辑扫描并删除。
- 事件记录不采用“猜测密码关键字”的正则脱敏。执行器为每条命令标记是否敏感：敏感命令不持久化原始 stdout/stderr；其余输出先替换本任务已知 secret 后再截取上限。相同规则适用于 `error_message` 和 `result_json`。

### 6.2 OpenSSH 调用

P0 使用类似以下的固定选项：

```text
ssh -o BatchMode=yes \
    -o StrictHostKeyChecking=yes \
    -o UserKnownHostsFile=<task-known-hosts> \
    -o GlobalKnownHostsFile=/dev/null \
    -o IdentitiesOnly=yes \
    -o ConnectTimeout=10 \
    -o ServerAliveInterval=15 \
    -o ServerAliveCountMax=3 \
    -i <temporary-key> \
    -p <ssh-port> <username>@<ssh-target> <fixed-command>
```

- 禁止 `StrictHostKeyChecking=accept-new`。
- `ssh_target`、`advertise_host`、端口和路径分别以类型解析；路径必须为绝对路径，拒绝空白、`..` 和 shell 元字符。
- `scp` 的本地和远程路径同样必须经过单独转义，不能把未校验值直接拼进 `user@host:path`。
- P0 不接受任意远程 shell 和 SQL；模板中的 SQL 参数使用经过严格 IP/FQDN/端口校验的值。

### 6.3 包与权限

- 包仅允许管理端 HTTPS 下载或受控上传；必须带 SHA-256，下载后管理端和每个目标主机均校验。
- 包 URL 需要协议、重定向次数和允许域名校验，防止 SSRF；缓存目录不对外暴露。解压前必须验证归档成员路径：拒绝绝对路径、`..`、危险符号链接和硬链接逃逸，且不得保留归档中的所有者/权限。
- `/api/sr-ops/*` 必须加入 `permission_extractor`，产生稳定的 `sr-ops` resource/action；前端 `PermissionGuard` 仅用于体验，不是授权边界。
- 每个写操作都必须校验请求者是组织管理员或超级管理员，并在事务中校验所有引用记录属于同一组织。
- 新建、取消、失败、清理建议和高风险确认写入审计日志。

## 7. 任务状态机与恢复

```text
pending -> running -> succeeded
                  -> failed
                  -> interrupted   # 服务重启、lease 过期或 runner 异常
pending -> cancelled
```

### 7.1 领取任务

1. API 只创建 `pending` 任务，保存经过服务端规范化的计划快照和资源 ID。
2. runner 在 SQLite 事务中领取任务，写入 `lease_token`、`lease_expires_at` 和 `running` 状态。
3. `idx_sr_one_running_task` 是最终并发保护；内存锁仅用于降低无效竞争，不承担正确性。
4. runner 定期续租。续租失败即停止后续步骤并标记 `interrupted`。
5. 服务启动时扫描已过期或遗留的 `running` 任务，标记为 `interrupted`、清理临时密钥、写入事件；**不自动续跑**。

### 7.2 失败与取消

- 仅 `pending` 任务可取消。
- 对正在执行的 SSH 命令设置单步和总任务超时；停止 runner 时回收本地 child process。回收 SSH 进程**不能保证**已经下发的远程命令未继续执行，因此 `interrupted` 任务重试前必须重新探测远端进程、配置和 `SHOW PROC` 状态。
- 不自动删除 `meta_dir`、`storage_dir` 或已安装文件。失败事件只给出经过审查的人工检查/清理建议。
- 重试创建新任务；模板和远程探测必须幂等，但不得把“目录存在”直接等同于“部署成功”。

## 8. 预检与资源预占

预检本身是只读操作，输出结构化结果，任何失败都阻止后续资源预占和部署任务执行。资源预占在预检成功后由任务 runner 以独立事务完成。

| 类别 | 必检项 |
|---|---|
| 控制面 | `ssh/scp` 可执行、缓存空间、凭据权限、已确认 host key。 |
| 目标主机 | Linux x86_64、SSH 登录、服务账号、安装/元数据/数据目录的属主与可写性、Java 版本、磁盘和挂载信息。 |
| 网络 | 所有节点之间的 FE/BE 端口矩阵、控制面到 SSH、所选 `advertise_host` 可达性。 |
| 资源 | 安装路径、数据路径、端口未占用，以及同一部署计划内的路径/端口互斥检查。预检成功后，runner 在独立事务中创建 `sr_host_allocations`，防止两个集群竞争同一主机。 |
| StarRocks | 版本处于支持矩阵；单 BE 复制数规则；`priority_networks` 或 FQDN 配置完整。 |
| OS 建议 | `nofile`、swappiness、overcommit、时钟同步、THP 等仅报告，不自动修改。 |

`ulimit`、`sysctl`、防火墙和 systemd 的修改必须在后续独立的、经显式批准的主机基线能力中实现；不能混入部署任务。

## 9. P0 部署流程（shared-nothing）

所有步骤使用任务计划快照，而非重新读取可变表单。涉及凭据时仅保存凭据 ID。

| 步骤 | 名称 | 动作与成功条件 |
|---|---|---|
| 1 | validate | 校验组织、权限、拓扑、版本、凭据、包和路径。 |
| 2 | precheck | 执行第 8 节只读预检；失败即终止。 |
| 3 | reserve | 在事务中原子预占主机；失败任务保留对其自身主机的分配，直到同一集群重试成功或经人工确认完成远端清理后释放。 |
| 4 | cache_package | 下载受控包并校验 SHA-256；缓存以 hash 命名。 |
| 5 | distribute | 将包复制到目标主机临时目录，目标端执行 SHA-256 校验后原子改名。 |
| 6 | install | 解压到版本目录；仅在目录内容与 hash 一致时复用；以原子符号链接切换 `current`。 |
| 7 | render_config | 通过纯函数生成 `fe.conf`/`be.conf`；先写临时文件，再原子替换。禁止 `sed` 追加或替换未知配置。 |
| 8 | start_leader_fe | 启动首个 FE，轮询 HTTP 和 MySQL 连接直到可用或超时。 |
| 9 | add_followers | 对每个 Follower：先在 Leader 执行 `ALTER SYSTEM ADD FOLLOWER`，再首次使用 `start_fe.sh --helper <leader>:<edit_log_port> --daemon` 启动，最后确认 `SHOW PROC '/frontends'` 的 `Alive=true`。 |
| 10 | start_and_add_be | 先启动每个 BE，再通过操作期 MySQL 客户端执行 `ALTER SYSTEM ADD BACKEND`，确认 `SHOW PROC '/backends'` 的 `Alive=true`。 |
| 11 | create_operator | 使用加密的 bootstrap 凭据创建最小权限的持续运维账号。P0 不自动轮换 root 密码；该动作必须按已验证的 StarRocks 版本模板执行。 |
| 12 | register_cluster | 仅使用第 11 步创建的最小权限 operator 凭据注册既有 `clusters`，绝不保存 bootstrap/root 凭据。使用显式的“是否激活”策略；不得让“组织首个集群自动激活”的现有副作用替代用户选择。 |
| 13 | verify | 使用已注册 cluster 的常规适配器执行健康检查；成功后更新节点和 managed cluster 为 `running`。 |

部署期间 `ADD FOLLOWER`、`ADD BACKEND` 均先检查现存节点，确保重试不会重复添加。实际使用的地址必须是节点登记的 `advertise_host`，而不是 SSH 目标地址的猜测值。

## 10. 只读接管

接管表单提供 FE 地址、查询端口和数据库凭据引用，执行以下动作：

1. 通过操作期直连验证连接与版本。
2. 从 `SHOW PROC '/frontends'`、`SHOW PROC '/backends'` 导入**观测节点**。
3. 创建 `sr_managed_clusters(status='adopted_read_only')` 并关联既有 `clusters`。
4. 仅允许查看状态、事件和跳转到集群详情。

接管记录在完成每个节点的 SSH 主机、host key、安装路径、运行用户、端口和配置核验前，禁止启停、配置、扩缩容和升级。

## 11. 后续生命周期能力

### 11.1 P1：启停、重启与 scale-out

前提：任务 lease、主机绑定、版本能力矩阵、服务账号和系统启动策略已经完成。

- 启动顺序：Leader FE → Follower FE（非首启不带 `--helper`）→ BE。
- 停止顺序：BE → Follower FE → Leader FE。高可用集群的全停必须显示数据面和控制面不可用确认。
- 重启分为“整集群停启”和“滚动重启”；默认只提供滚动重启，并在每个节点恢复 `Alive=true` 后继续。
- scale-out 复用 P0 的预检、校验、安装和节点注册步骤。
- 主机重启后的自动拉起必须依赖经审计的 systemd 能力；未配置时 UI 明确显示“主机重启后需人工恢复”。

### 11.2 P2：缩容、配置与升级

- **缩容**：BE 先校验副本数、剩余磁盘容量和故障域，再 `DECOMMISSION`，等待完成后才 `DROP BACKEND` 和停止进程。FE 禁止破坏选举/仲裁约束；Leader 必须先完成切主。CN 有独立的无数据迁移流程，不能复用 BE 缩容。
- **配置变更**：只接受版本化参数白名单。动态参数使用对应的受控 SQL；静态参数通过受控配置文件片段和原子替换，禁止通用 `sed`。执行前读取当前值，执行后验证生效值。
- **升级**：仅支持版本矩阵中明确允许的路径，且必须先做单节点可用性测试。顺序为 BE/CN → Follower FE → Leader FE。升级前保存现有 balance/tablet clone 相关配置，结束后按原值恢复，不能硬编码“默认恢复值”。分别保留 BE/CN 的 UDF 目录和 FE 的 `spark-dpp`；`bin`、`lib`、`spark-dpp` 的替换使用独立原子命令。连续小版本、降级后二次升级等场景按官方要求处理 image 同步和兼容性配置。
- **shared-data**：先增加独立的加密存储凭据/存储卷模型、对象存储连通性预检和 CN 生命周期；不得在任务 JSON 中保存 `CREATE STORAGE VOLUME` 的密钥。

## 12. API 与权限

所有路由挂在认证中间件后，但授权必须通过扩展后的 `permission_extractor` 产生 Casbin resource/action。

| 方法 | 路径 | 权限动作 | 说明 |
|---|---|---|---|
| GET/POST | `/api/sr-ops/hosts` | `hosts:list` / `hosts:manage` | 资产查询、登记；主机指纹确认独立动作。 |
| POST | `/api/sr-ops/hosts/:id/check` | `hosts:check` | 只读预检。 |
| GET/POST | `/api/sr-ops/credentials` | `credentials:list` / `credentials:manage` | 仅返回元数据，secret 只写不读。 |
| GET/POST | `/api/sr-ops/packages` | `packages:list` / `packages:manage` | 包登记与缓存。 |
| GET | `/api/sr-ops/clusters` | `clusters:list` | 托管集群和节点查询。 |
| POST | `/api/sr-ops/deployments` | `deployments:create` | 使用强类型 `DeployRequest` 创建 deploy 任务；不接受泛型 task payload。 |
| POST | `/api/sr-ops/adoptions` | `adoptions:create` | 只读接管。 |
| GET | `/api/sr-ops/tasks/:id` | `tasks:get` | 查询任务与已脱敏事件。 |
| POST | `/api/sr-ops/tasks/:id/cancel` | `tasks:cancel` | 仅取消 pending。 |

每个 handler 在写入前必须：

1. 验证 Casbin 权限；
2. 验证请求者的组织管理员/超级管理员角色；
3. 在同一事务中验证所有引用资源的组织归属；
4. 以任务 ID、操作人和目标资源写审计记录。

## 13. 前端

```text
pages/starrocks/physical/
  physical-list.component       # 托管集群、任务两个 tab
  deploy-wizard.component       # 主机 -> 凭据 -> 包 -> 拓扑 -> 预检 -> 确认
  host-manage.component         # 主机、fingerprint、只读预检
  credential-dialog.component   # 私钥只写入，不回显
  task-detail.component         # 任务步骤、脱敏事件，3 秒轮询
```

- 向导在提交前展示不可变计划摘要：节点地址、端口、路径、包 SHA-256、版本、是否为非 HA 拓扑。
- `deploy-wizard` 不接收或展示存储密钥、root 密码、私钥内容的回显。
- “接管”页面必须标示 `只读`，直到后续版本完成基础设施绑定。
- 菜单权限只做可见性控制；真实授权以后端 Casbin 和组织校验为准。

## 14. 测试策略

### 单元与集成测试

- SQLite migration 测试：真实执行迁移，覆盖唯一索引和外键；禁止表达式 `UNIQUE` 回归。
- 参数校验：IP/FQDN、端口、绝对路径、host key fingerprint、URL、SHA-256、命令/SQL 注入样本。
- 凭据服务：随机 nonce、AAD、key rotation、响应无 secret、日志无 secret。
- 任务 runner：并发创建同集群任务、lease 竞争、服务重启后的 `interrupted` 恢复、临时私钥清理、超时停止。
- 组织隔离：跨组织 host、credential、package、managed cluster、cluster ID 引用一律拒绝；未授权 `/api/sr-ops/*` 返回 403。
- mock executor：Follower 的 ADD/`--helper` 顺序、BE 启动/添加顺序、失败中断、幂等重试。
- `scripts/test/sr-deploy-smoke.sh`：针对 Docker 化 SSH 靶机验证 1FE+1BE 非 HA 部署；高可用拓扑另建环境验证。

### 手工验收

- SSH host key 变更后任务拒绝执行。
- 包 hash 不匹配时不解压、不启动。
- 控制面重启期间任务不会继续误执行，遗留任务变为 `interrupted`；重试前必须重新探测远端实际状态。
- 普通组织用户不能读取、引用或操作其他组织的任何物理机资源。
- 部署后通过既有集群详情、健康检查和适配器访问成功。

## 15. 实施门禁与分期

### P0 开发前必须完成

- [ ] 独立凭据加密服务与既有 cluster 凭据安全存储改造。
- [ ] `/api/sr-ops/*` 的 Casbin 权限提取、组织校验和 403 测试。
- [ ] SQLite migration 原型通过真实迁移测试。
- [ ] task lease、启动恢复和 SSH child process 清理实现完成。
- [ ] SSH fingerprint 确认、私钥专属临时文件和严格 `known_hosts` 实现完成。
- [ ] 强制包 SHA-256、管理端/目标端双重校验完成。
- [ ] 已验证 StarRocks 版本矩阵和 P0 拓扑规则落地。

### 分期

| 阶段 | 交付 |
|---|---|
| P0 | shared-nothing 新建部署、只读接管、预检、受控包、任务事件、注册既有 clusters。 |
| P1 | systemd/主机基线能力、滚动启停、scale-out、动态配置白名单。 |
| P2 | 缩容、静态配置、滚动升级、shared-data、经核验后的可写接管。 |

## 16. 参考资料

- [StarRocks 手工部署 shared-nothing 集群](https://docs.starrocks.io/docs/deployment/deploy_manually/)
- [StarRocks 手工部署 shared-data 集群](https://docs.starrocks.io/docs/deployment/deploy_shared_data_manually/)
- [StarRocks 升级指南](https://docs.starrocks.io/docs/deployment/manage_deployment/upgrade/)
- 本仓库：`backend/src/services/cluster_service.rs`、`backend/src/services/mysql_pool_manager.rs`、`backend/src/middleware/permission_extractor.rs`、`backend/migrations/00000000_initial_schema.sql`
