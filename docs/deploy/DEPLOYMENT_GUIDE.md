# Stellar 部署指南

> 面向业务运维人员的部署手册。所有安装包均从 [GitHub Releases](https://github.com/jlon/stellar/releases) 下载，无需源码、无需编译环境。
>
> 以下命令中的 `1.0.0` 请替换为实际要安装的版本号（可在 [Releases 页面](https://github.com/jlon/stellar/releases) 查看最新版本）。

---

## 目录

- [安装包选择](#安装包选择)
- [系统要求](#系统要求)
- [首次启动必读](#首次启动必读)
- [方式一：Debian / Ubuntu（DEB 包，推荐）](#方式一debian--ubuntudeb-包推荐)
- [方式二：二进制压缩包（任意 Linux）](#方式二二进制压缩包任意-linux)
- [方式三：Docker](#方式三docker)
- [方式四：npm（已有 Node.js 环境）](#方式四npm已有-nodejs-环境)
- [生产环境建议](#生产环境建议)
- [配置说明](#配置说明)
- [日常运维](#日常运维)
- [常见问题](#常见问题)

---

## 安装包选择

每个版本提供 4 种安装包，**内容完全相同**（同一个静态编译的服务端），按环境选择即可：

| 安装包 | 文件名 | 适用场景 |
|-------|--------|---------|
| DEB 包 | `stellar-server-1.0.0-amd64.deb` | Debian / Ubuntu 服务器，**推荐**：自动注册 systemd 服务 |
| 压缩包 | `stellar-1.0.0-linux-amd64-musl.tar.gz` | 任意 Linux（含 CentOS / 麒麟等），解压即用 |
| Docker 镜像 | `ghcr.io/jlon/stellar:1.0.0` | 已有 Docker / Kubernetes 环境 |
| npm 包 | `stellar-server-1.0.0.tgz` | 已有 Node.js 环境，仅作分发通道（运行不依赖 Node） |

> 所有 Linux 安装包均为全静态编译（musl），无任何运行时依赖，CentOS 7 等老系统可直接运行。
>
> 每个安装包均附带 `.sha256` 校验文件，安全要求高的环境请先校验：
> `sha256sum -c stellar-server-1.0.0-amd64.deb.sha256`

---

## 系统要求

| 项目 | 要求 |
|-----|------|
| 操作系统 | 64 位 Linux（内核 3.10+） |
| 硬件 | 2 核 CPU / 2 GB 内存 / 10 GB 磁盘（最低） |
| 端口 | TCP 9527（Web 界面与 API，可改） |
| 数据库 | 无需安装：内置 SQLite；生产也可选 MySQL / PostgreSQL |
| 被管理集群 | StarRocks 或 Apache Doris 的 FE 地址与账号（安装后再在界面上添加） |

---

## 首次启动必读

Stellar 采用 **MinIO 风格的一次性密码**机制，所有安装方式通用：

1. **第一次启动**时，自动创建管理员账号 `admin`，并在控制台**打印一次性随机密码**：

   ```
     Created initial user 'admin' with password: dZ6b8V5wDTnCnVaQ

     This is shown once. Change it after first login.
   ```

2. 用该密码登录 Web 界面后**立即修改密码**（右上角 → 用户设置）。

3. 所有数据（数据库、密钥、日志）都存放在一个**数据目录**中，备份该目录即可备份全部状态：

   ```
   <数据目录>/
   ├── stellar.db      # 平台数据库
   ├── .jwt-secret     # 自动生成的登录密钥（0600）
   └── logs/           # 运行日志（按天轮转）
   ```

不想找密码？也可以在首次启动前设置环境变量 `STELLAR_ROOT_PASSWORD=你的密码`。

> 默认运行语义为 `production`。仅源码开发使用 `STELLAR_ENV=development`，它会让空的
> 本地数据目录初始化为 `admin/admin`；已初始化账户不会因该变量或
> `STELLAR_ROOT_PASSWORD` 在重启时自动改密。部署环境不要设置 `STELLAR_ENV=development`。

---

## 方式一：Debian / Ubuntu（DEB 包，推荐）

```bash
# 1. 下载
wget https://github.com/jlon/stellar/releases/download/v1.0.0/stellar-server-1.0.0-amd64.deb

# 2. 安装（自动创建 stellar 系统用户并注册 systemd 服务）
sudo apt install ./stellar-server-1.0.0-amd64.deb
sudo systemctl start stellar

# 3. 查看一次性管理员密码
sudo journalctl -u stellar | grep 'password:'
```

安装后即完成部署：

| 项目 | 值 |
|-----|-----|
| 服务 | `systemctl status stellar` |
| 访问地址 | http://<服务器IP>:9527 |
| 程序 | `/opt/stellar/bin/stellar` |
| 数据目录 | `/opt/stellar`（数据库与日志在 `data/`、`logs/` 子目录） |
| 配置文件 | `/opt/stellar/conf/config.toml` |

常用操作：

```bash
sudo systemctl restart stellar    # 重启
sudo journalctl -u stellar -f     # 跟踪日志
```

升级：下载新版本 .deb 后再次 `sudo apt install ./...deb`，数据自动保留。

---

## 方式二：二进制压缩包（任意 Linux）

```bash
# 1. 下载并解压
wget https://github.com/jlon/stellar/releases/download/v1.0.0/stellar-1.0.0-linux-amd64-musl.tar.gz
tar xzf stellar-1.0.0-linux-amd64-musl.tar.gz
cd stellar

# 2. 启动（使用随包 conf/config.toml；数据和日志保存在 ./data）
./bin/stellar.sh start

# 3. 查看一次性管理员密码
grep 'password:' data/logs/console.log
```

访问 http://<服务器IP>:9527。

> 上面用 `nohup` 是为了快速体验；**生产环境请务必用 systemd 托管**（进程崩溃自动拉起、开机自启）。
> 参考服务文件已在压缩包外的仓库中提供：[deploy/systemd/stellar.service](https://github.com/jlon/stellar/blob/main/deploy/systemd/stellar.service)。
>
> 升级：停止进程 → 用新版压缩包中的 `bin/stellar` 替换旧文件 → 重新启动。数据目录不动。

---

## 方式三：Docker

镜像发布在 GitHub Container Registry：`ghcr.io/jlon/stellar`。

```bash
# 1. 启动（数据持久化在宿主目录 ./data）
docker run -d \
  --name stellar \
  -p 9527:9527 \
  -v $(pwd)/data:/data \
  --restart unless-stopped \
  ghcr.io/jlon/stellar:1.0.0

# 2. 查看一次性管理员密码
docker logs stellar 2>&1 | grep 'password:'
```

访问 http://<服务器IP>:9527。

升级：`docker pull` 新版本镜像 → `docker rm -f stellar` → 用同一 `-v` 卷重新 `docker run`。

**Docker Compose：**

```yaml
services:
  stellar:
    image: ghcr.io/jlon/stellar:1.0.0
    container_name: stellar
    ports:
      - "9527:9527"
    volumes:
      - ./data:/data
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9527/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s
```

```bash
docker compose up -d
docker compose logs stellar | grep 'password:'
```

> Kubernetes / Helm 用户：使用仓库内 [deploy/chart](https://github.com/jlon/stellar/tree/main/deploy/chart)（`helm install stellar deploy/chart`），同样从首次启动日志获取一次性密码。

---

## 方式四：npm（已有 Node.js 环境）

npm 仅作为下载通道，Stellar 运行本身**不依赖 Node.js**。

```bash
# 1. 从 Release 下载并全局安装
wget https://github.com/jlon/stellar/releases/download/v1.0.0/stellar-server-1.0.0.tgz
sudo npm install -g ./stellar-server-1.0.0.tgz

# 2. 启动（数据目录 /var/lib/stellar）
sudo mkdir -p /var/lib/stellar
stellar server /var/lib/stellar

# 3. 一次性管理员密码直接打印在控制台
```

> npm 全局安装只释放文件与命令软链，不会注册系统服务。生产环境请配合 systemd 使用
> （参考 [deploy/systemd/stellar.service](https://github.com/jlon/stellar/blob/main/deploy/systemd/stellar.service)，`ExecStart` 使用 `stellar server /var/lib/stellar` 数据目录模式）。

---

## 生产环境建议

1. **立即改密码**：首次登录后修改 `admin` 初始密码。
2. **数据目录持久化**：Docker/K8s 必须挂载卷（见上文）；裸机部署把数据目录放在 `/var/lib/stellar` 这类固定路径。
3. **数据库升级（可选）**：默认 SQLite 适合单机；需要高可用时在配置文件中将 `database.url` 指向外部 MySQL / PostgreSQL（同一二进制自动识别，无需换包）。
4. **日志保留**：日志只按天轮转、不自动删除，建议配置 logrotate（`copytruncate` 方式）或定期清理 `<数据目录>/logs/`。
5. **专用监控账号**：添加被管理集群时，按 [scripts/permissions/README_PERMISSIONS.md](https://github.com/jlon/stellar/blob/main/scripts/permissions/README_PERMISSIONS.md) 在 StarRocks/Doris 侧创建专用监控用户，不要使用 root。
6. **下载包校验**：核对 `.sha256` 文件后再安装。

---

## 配置说明

### 两种配置方式

| 方式 | 适用 | 说明 |
|-----|------|------|
| 零配置（推荐起步） | 快速上手 | 什么都不配即可跑；数据目录默认 `./data` |
| 配置文件 | 生产定制 | `stellar server /var/lib/stellar --config /etc/stellar/config.toml` |

优先级：命令行参数 > 环境变量（`APP_` 前缀）> 配置文件 > 默认值。

### 主配置文件 (config.toml)

```toml
[server]
host = "0.0.0.0"          # 监听地址
port = 9527               # 监听端口

[database]
# 内置 SQLite（默认）；也可指向外部 MySQL / PostgreSQL
url = "sqlite:///var/lib/stellar/stellar.db"
# url = "mysql://user:pass@localhost:3306/stellar?charset=utf8mb4"
# url = "postgres://user:pass@localhost:5432/stellar"

[auth]
# 配置文件模式必须设置此项，也可改用 APP_JWT_SECRET 环境变量
jwt_secret = "请替换为至少 32 位随机密钥"
jwt_expires_in = "24h"    # 登录有效期

[logging]
level = "info,stellar_backend=info"  # 日志级别
file = "/var/lib/stellar/logs/stellar.log"  # 日志文件；删除此行则只输出到控制台

[metrics]
interval_secs = "30s"     # 指标采集间隔
retention_days = "7d"     # 指标保留时长
enabled = true            # 是否启用采集

[audit]
# StarRocks 审计日志库表；Doris 固定使用 __internal_schema.audit_log
database = "starrocks_audit_db__"
table = "starrocks_audit_tbl__"
```

> 数据目录模式（`stellar server /var/lib/stellar`）会自动生成并保存 `.jwt-secret`，重启不失效。
> 将数据目录与 `--config` 一起传入时，配置文件控制服务、数据库、日志和审计表，数据目录仅保存自动生成的 `.jwt-secret`；未传数据目录时，显式 `--config` 模式必须设置 `jwt_secret` 或 `APP_JWT_SECRET`。

### 常用环境变量

| 用途 | 变量 |
|-----|------|
| 数据目录 | `STELLAR_DATA_DIR`（等价于 `server` 后的位置参数） |
| 运行环境 | `STELLAR_ENV`（默认 `production`；源码开发可设为 `development`） |
| 初始管理员密码 | `STELLAR_ROOT_PASSWORD` |
| 监听端口 | `APP_SERVER_PORT` |
| 数据库 | `APP_DATABASE_URL` |
| JWT 密钥 | `APP_JWT_SECRET` |
| 日志级别 | `APP_LOG_LEVEL` |
| 采集开关 | `APP_METRICS_ENABLED=true/false` |
| StarRocks 审计库 | `APP_AUDIT_DATABASE` |
| StarRocks 审计表 | `APP_AUDIT_TABLE` |

---

## 日常运维

### 健康检查

```bash
curl http://127.0.0.1:9527/health     # 返回 OK 即正常
```

### 查看与打包日志

界面：**系统管理 → 操作日志 → 右上角下载图标**，一键打包当前全部运行日志（zip）。

命令行：

```bash
TOKEN=$(curl -s -X POST http://127.0.0.1:9527/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"***"}' | jq -r .token)
curl -f -H "Authorization: Bearer $TOKEN" -o stellar-logs.zip \
  http://127.0.0.1:9527/api/system/logs/archive
```

### 备份与恢复

```bash
# 备份（停服后执行最稳妥；在线备份 SQLite 请配合 sqlite3 .backup）
tar czf stellar-backup.tgz /var/lib/stellar

# 恢复：解压回原路径，重启服务
```

### 升级

所有安装方式共用同一原则：**替换程序，不动数据目录**。

| 安装方式 | 升级操作 |
|---------|---------|
| DEB | `sudo apt install ./stellar-server-<新版本>-amd64.deb` |
| 压缩包 | 停进程 → 替换 `bin/stellar` → 启动 |
| Docker | `docker pull` 新镜像 → 重建容器（同一数据卷） |
| npm | `sudo npm install -g ./stellar-server-<新版本>.tgz` |

---

## 常见问题

### Q1: 首次启动的控制台密码没看到 / 忘了改密码？

密码只在**数据目录为空**的首次启动打印一次。若已丢失：

1. 停止服务，删除数据目录中的 `stellar.db`（会清空全部平台数据，集群连接需重新添加）；
2. 重新启动获取新密码。

或预先规划：首次启动前设置 `STELLAR_ROOT_PASSWORD`。

### Q2: 启动失败 / 页面打不开？

```bash
# 看服务状态与日志
sudo journalctl -u stellar -n 50        # DEB
docker logs --tail 50 stellar           # Docker

# 常见原因
# 1) 端口被占用：换端口（--server-port 或配置文件 port）
# 2) 数据目录无写权限：chown -R <运行用户> <数据目录>
```

### Q3: 如何修改监听端口？

```bash
# 方式一：命令行
stellar server /var/lib/stellar --server-port 9090

# 方式二：配置文件 [server] port = 9090
# 方式三：环境变量 APP_SERVER_PORT=9090
```

### Q4: 如何接入 StarRocks / Doris 集群？

登录 Web 界面 → 集群管理 → 添加集群，填写 FE 的 HTTP 地址与管理账号。
建议先用 [监控专用账号脚本](https://github.com/jlon/stellar/blob/main/scripts/permissions/README_PERMISSIONS.md) 创建低权限用户。

### Q5: 忘记 admin 密码（非首次）？

当前版本请通过数据库重置（停服后用 sqlite3 更新 `users` 表的 `password_hash`，或删除 `stellar.db` 重建——后者会清空平台数据）。管理员的密码自助修改入口在右上角用户菜单。

---

## 相关链接

- [版本发布页（全部安装包）](https://github.com/jlon/stellar/releases)
- [Docker 镜像（GHCR）](https://github.com/jlon/stellar/pkgs/container/stellar)
- [操作审计与权限说明](https://github.com/jlon/stellar/blob/main/docs/PERMISSION_MANAGEMENT_DESIGN.md)
- [API 文档](https://github.com/jlon/stellar#api-文档)：服务启动后访问 `http://<host>:9527/swagger-ui/`
