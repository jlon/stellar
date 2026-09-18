# Stellar 离线授权系统设计文档

- 状态：设计定稿（v1）
- 范围：固定到期日授权、无 TPM、按集群数限制
- 关联代码：`backend/src/services/license_service.rs`（新增）、`backend/src/bin/license-issuer.rs`（新增）、`backend/src/services/cluster_service.rs`、`backend/src/main.rs`

## 1. 背景与目标

Stellar 是离线部署的企业级 OLAP 集群管理平台（无在线服务器）。需要一种商业授权机制来控制**使用期限**与**可管理集群数量**，并抵抗常见的破解手段。

**目标（按优先级）**：

1. 客户无法**伪造**授权文件（自己生成一个永久授权）。
2. 客户无法**修改**授权内容（延期、加集群数、升版本）。
3. 客户无法把授权**复制到另一台机器**使用。
4. 客户**回拨系统时钟**无法无限续期。
5. 到期后产品进入可控的只读状态，不造成数据破坏。

**业界一致性**：本方案是业界通用做法的现代分支——老牌 FlexNet Publisher（工业软件授权事实标准）的 node-locked license 即"机器指纹 + 签名 license 文件 + 离线部署"；现代 Ed25519 离线授权实现（Keygen / Keymint / Keymaster 等）均为"内嵌公钥 + 签名 payload + 机器绑定 + 固定到期日 + 完全离线验证"。与本方案唯一差异：机器绑定用设备密钥对而非 MAC hostid（MAC 可伪造，密钥对是现代更稳做法）。

**明确不做的事（YAGNI）**：

- 不做累计运行时长计量（无 TPM / 无在线服务时无法可靠保证，做了也是假的）。
- 不做 MAC / IP / 磁盘序列号绑定（易伪造、误伤率高，防不了整盘复制）。
- 不做授权文件加密（授权内容不是秘密，签名完整性才是关键；加密密钥藏在程序里没有意义）。
- 不做代码混淆 / 自研加密算法（收益低，成本高，只防君子）。
- 不做在线激活 / 吊销（无服务器）。

## 2. 威胁模型与信任边界

| 攻击方式 | 防护手段 | 能否防住 |
|---|---|---|
| 修改 license 字段（延期/加集群数） | Ed25519 签名 | ✅ 能（私钥不泄露） |
| 自行生成 license | 私钥仅授权方离线持有 | ✅ 能（私钥不泄露） |
| 把 license.lic 拷到另一台已安装机器 | 设备公钥绑定 | ✅ 能（他机无私钥） |
| 回拨系统时钟 | 最大已见时间水位 | ⚠️ 显著提高成本，防不住快照回滚 |
| 整个安装目录（含设备私钥）拷走 | 无 TPM 无法防 | ❌ 明确无法防 |
| root 权限修改二进制 / 绕过校验 | 无 TPM 无法防 | ❌ 明确无法防（需加密狗） |

**信任边界声明**：本方案保证"授权文件不可伪造、不可篡改、不可跨机复用"；**不保证**客户拥有 root 权限且愿意逆向修改程序时仍能强制到期。这是纯软件离线方案的理论上限，写入销售与合同话术，避免过度承诺。

## 3. 总体架构

```
你方（授权方）                              客户机器（Stellar 部署）
─────────────                              ─────────────────────
license-issuer 签发工具                        Stellar 产品二进制
  ├─ 离线持有 Ed25519 私钥        ┌─激活请求──▶  生成设备密钥对
  └─ 签发 license.lic ───────────┘              内置 Ed25519 公钥
        │                                          │
        ▼                                          ▼
  人工交付（U 盘/邮件/工单） ────────────▶  import license.lic
                                                启动时验签 + 设备绑定 + 时间检查
```

核心思想：**非对称签名，而非可计算的序列号**。客户能看、能复制授权文件，但改任何字段都会导致验签失败。

### 3.1 使用方式（操作流程）

**客户视角——首次部署（4 步）**：

```bash
# 1. 部署完 Stellar，生成本机激活请求（内容：设备公钥 + 产品标识）
stellar license request --out request.json

# 2. 把 request.json 发给厂商（邮件 / 工单 / U 盘均可，完全离线）
#    —— 厂商审核订单后回传 license.lic ——

# 3. 导入，立即生效
stellar license import license.lic

# 4. 确认状态
stellar license status
# => 状态: 有效 | 版本: standard | 到期: 2027-01-01 | 剩余: 356 天 | 集群: 1/5
```

> 前端亦提供同等操作（页面按钮生成请求 / 上传导入 / 状态展示），CLI 与页面等价。

**客户视角——日常与续期**：日常无需任何操作；到期前 30 天页面出现黄色提示"授权将于 X 到期"。续期 = 厂商发新 `license.lic` → 客户导入，不重装、不换机器码。

**厂商视角**：

```bash
# 客户发来 request.json，审核订单后签发（自动带入请求文件中的设备公钥）
license-issuer issue \
  --request request.json \
  --expires-at 2027-01-01 \
  --max-clusters 5 \
  --out license.lic
```

**完整示例**：北京某客户 2026-01-01 购买 1 年、5 集群授权。① 客户 `license request` 生成请求文件 → ② 你签发 `license.lic`（到期 2027-01-01、上限 5）发回 → ③ 客户导入，页面显示"有效，1/5 集群" → ④ 到期未续：后端自动只读——可查看/导出/导入新授权，创建集群与执行 SQL 被拒（提示"授权已过期"）→ ⑤ 续费后你签发新文件（到期 2028-01-01），客户导入即恢复。

客户全程不接触任何密钥，也无需输入易抄错的序列号，就是一个文件导入即用。厂商侧唯一职责：保管好 `private_key.pem`（泄露需换 key 重签）。

## 4. 授权文件格式

文件为 JSON 外壳（人类可读、便于工单排查），签名覆盖 payload 的**原始字节**（杜绝"反序列化后重新序列化再验签"的规范化问题）：

```json
{
  "encoding": "stellar-license-v1",
  "payload_b64": "<payload 字节的 base64>",
  "signature_b64": "<Ed25519(payload 字节) 的 base64，64 字节>"
}
```

payload（serde struct，字段顺序固定，序列化确定）：

```json
{
  "version": 1,
  "license_id": "uuid",
  "key_id": "prod-2026-01",
  "product": "stellar",
  "edition": "standard",
  "issued_at": 1767225600,
  "not_before": 1767225600,
  "expires_at": 1798761600,
  "device_public_key_b64": "<32 字节 Ed25519 设备公钥>",
  "max_managed_clusters": 5
}
```

- 时间均为 UTC unix 秒（`chrono` 现有依赖可直接处理）。
- `key_id` 对应产品内置公钥表，支持密钥轮换（见 §6）。
- 校验步骤：解码外壳 → 按 `key_id` 取公钥验签 payload 原始字节 → serde 解析 → 业务校验（§7）。

## 5. 密钥体系与密钥管理

**算法**：Ed25519（`ring` crate —— 已在 `jsonwebtoken` 依赖树中，**零新增下载依赖**）。

**密钥对**：

| 密钥 | 归属 | 存储 |
|---|---|---|
| 签发私钥（seed/PKCS#8） | 授权方 | 授权方离线环境文件，0600 权限，加密备份；**禁止**进 Git/CI/Docker 镜像/产品二进制 |
| 签发公钥（32B） | 产品 | 编译期内嵌常量 `LICENSE_PUBLIC_KEYS: &[(&str, &[u8])]`（key_id + 公钥） |
| 设备私钥（seed） | 客户机器 | `data/device_key.json`（0600），首次启动生成 |
| 设备公钥（32B） | 客户机器 | 同上文件 + 写入激活请求 + 签入 license |

**管理规则**：

1. 生产私钥与测试私钥分离；发布版内嵌的公钥必须对应生产私钥，测试私钥签发的 license 一律拒绝（`key_id` 校验天然实现）。
2. 私钥在授权方的 `license-issuer` 工具环境生成，`generate` 子命令输出 `private_key.pem` + 公钥展示，由人工贴入产品公钥常量。
3. 轮换：新 license 用新 `key_id` 签发，产品内置公钥表保留旧 key 直至旧 license 全部到期，实现无缝过渡。

## 6. 设备绑定（软绑定，无 TPM）

不绑定任何硬件字符串，绑定**设备私钥的存在性**：

1. 首次启动生成设备 Ed25519 密钥对，存 `data/device_key.json`。
2. `license request` 输出激活请求文件（含设备公钥）。
3. license 中签入 `device_public_key_b64`。
4. 产品校验时要求 `license.device_public_key_b64 == 本机设备公钥`。

效果：把 `license.lic` 拷到另一台已安装的 Stellar 机器 → 设备公钥不匹配 → 拒绝。无需挑战-响应签名（无 TPM 时私钥就在本机，挑战无额外价值，KISS）。

**上限**：与 `data/device_key.json` 一起整体拷贝可复制（等价于整盘拷贝/虚拟机快照），无 TPM 时无法防，按 §2 声明。

**容器化部署（Docker / Kubernetes）**：本方案绑定的是"Stellar 实例身份"（`data/` 下的设备密钥），而非物理机硬件，因此天然适配容器，无需额外设计：

- **硬性部署要求**：`data/` 目录必须挂持久卷（Docker named volume / K8s PVC），设备密钥、license 文件、时间水位与 SQLite 数据库同卷。这是容器化部署本来就必须满足的条件（数据库不持久化则部署无意义）。
- Pod 删除重建、节点迁移、滚动升级：密钥与 license 随卷存活，授权连续有效，**无需重新激活**。
- 多副本（未来 HA，共享同一数据卷）：共享设备密钥，按"同一实例"计一份授权。
- 不同 K8s 集群 / 不同数据卷各部署一套 Stellar = 各自独立实例身份，各自需要一份授权（instance-based licensing；业界同此语义，如 AppsCode 按集群签发离线 license、kx 按 scope 签发）。
- 时间水位与回退检测随卷持久化，行为与裸机部署一致。
- 业界佐证：Cryptlex 明确容器/VM 中硬件指纹不可靠、推荐基于密钥的绑定（本方案即此路线）；MoveIt Pro 容器化要求"每次启动提供相同的持久身份与数据"，与本方案"身份随持久卷"一致。

## 7. 校验流程与时间防护

### 7.1 校验时机

- 后端启动时完整校验一次，缓存结果到 `AppState`。
- 每日定时任务（复用 `utils/scheduled_executor.rs`）刷新时间检查与水位。
- `/api/license/status` 实时重校验（不信任缓存）。

### 7.2 校验顺序

```
1. 验签（key_id → 公钥 → payload 原始字节）
2. product == "stellar"
3. not_before <= now <= expires_at
4. device_public_key_b64 == 本机设备公钥
5. 时间水位检查（见下）
```

任一失败 → 明确错误码（`INVALID_SIGNATURE` / `EXPIRED` / `DEVICE_MISMATCH` / `CLOCK_ROLLBACK`），写入日志。

### 7.3 时间回退防护

状态文件 `data/license_state.json`：

```json
{ "license_id": "...", "last_seen_utc": 1798761600, "clock_suspect": false }
```

- 每次校验后 `last_seen_utc = max(now, last_seen_utc)`。
- `now < last_seen_utc - 300s` → `clock_suspect = true` → 受保护操作全部拒绝，仅保留：查看授权状态、导入新授权、导出诊断信息。
- 恢复通道：授权方重新签发一份 license（合法导入即清除水位），客户无需任何"重置命令"——不留后门开关。
- 水位是文件，可被删除——这就是它的上限，不构成硬防线（§2 已声明）。

### 7.4 到期与无授权策略

| 状态 | 允许 | 拒绝 |
|---|---|---|
| 有效 | 全部功能，集群数受 `max_managed_clusters` 限制 | — |
| 已过期 / 无效签名 / 设备不匹配 / 时钟存疑 | 查看状态、查看/导出历史数据、导入新 license | 创建/修改/删除集群、执行 SQL、后台任务、高级功能 |
| 无 license 文件 | 同上（默认只读） | 同上 |

原则：**只读但不破坏**。不删数据、不停库、不使已纳管集群失联；到期是"关水龙头"，不是"炸机房"。

## 8. 后端集成设计

### 8.1 `LicenseService`（新增 `backend/src/services/license_service.rs`）

```
struct LicenseStatus {
    state: Licensed | Expired | InvalidSignature | DeviceMismatch | ClockRollback | NotInstalled,
    edition, expires_at, max_managed_clusters, license_id, issued_at,
}

trait: 
  load_and_verify()          // 启动时全量校验，写缓存
  status() -> LicenseStatus  // 实时重校验
  import_license(bytes)      // 导入 + 原子写 data/license.lic + 全量校验
  ensure_writable() -> ApiResult<()>   // 非有效状态 → 403 LICENSE_INACTIVE
  ensure_cluster_capacity() -> ApiResult<()>  // 有效集群数 >= max → 403 LICENSE_LIMIT_EXCEEDED
```

挂载到 `AppState`（与 cluster_service 同生命周期），注入 main.rs 初始化。

### 8.2 集群数限制点位（真实锚点）

`cluster_service.rs` 的 `create_cluster`（约 L79）在名称/连接校验通过后、写库前调用：

```rust
self.license.ensure_writable()?;
self.license.ensure_cluster_capacity()?;   // 内部查有效集群数并比对
```

集群计数口径与现有代码一致：`SELECT COUNT(*) FROM clusters WHERE is_active = TRUE`（cluster_service.rs L157 已存在该查询模式）。**全局计数**（不限 org），如后续需要按组织授权再扩展（现有 L121 也有 org 维度查询可复用）。

### 8.3 写操作网关

`ensure_writable()` 在以下核心写路径调用（handler 层或 service 入口，二选一，不重复）：

- `POST /api/clusters`（create_cluster）
- `POST /api/clusters/queries/execute`
- 集群修改/删除、权限变更、物化视图创建、指标采集后台任务启动前（后台任务每轮执行前检查状态缓存）

`/api/license/*` 自身**永不**被网关拦截（救命通道）。

### 8.4 路由（main.rs）

```
GET  /api/license/status    → handlers::license::status
POST /api/license/import    → handlers::license::import_license
POST /api/license/request   → handlers::license::create_request   // 生成激活请求文件
```

### 8.5 签发工具（新增 `backend/src/bin/license-issuer.rs`）

独立 bin target，**不随产品分发**（产品镜像只构建主程序）：

```
cargo run --bin license-issuer -- keygen --out private_key.pem        # 授权方一次性
cargo run --bin license-issuer -- issue \
    --request activation-request.json \
    --key private_key.pem \
    --expires-at 2027-01-01T00:00:00Z \
    --max-clusters 5 \
    --out license.lic
```

issue 时自动将 `device_public_key_b64` 从请求文件带入 payload。

## 9. 前端呈现

新增 `pages/system/license/` 模块（与现有 system 模块组织一致）：

- **状态页**：license_id、版本、到期时间、剩余天数、授权集群数/已用集群数、当前状态徽标。
- **导入页**：文件选择 + 上传 `/api/license/import`，展示校验结果。
- 仅展示态 + 导入操作，不做任何"监管"交互（容量超限提示由后端 403 驱动，前端展示错误码文案）。

未授权/过期时前端隐藏菜单 **伴随** 后端网关双重生效（前端隐藏仅为体验，安全以后端为准）。

## 10. 测试与验证清单

位于 `backend/tests/`（集成测试目录，按功能模块建 `license_tests.rs`）与 license_service 模块测试：

1. 合法 license 全部校验通过；篡改 payload 任一字段（含 expires_at、max_managed_clusters）→ 验签失败。
2. 过期 license → `EXPIRED`；即将到期（<30 天）→ 状态含剩余天数，行为不变。
3. 设备公钥不匹配 → `DEVICE_MISMATCH`。
4. 时钟回拨（水位后 5 分钟以上）→ `CLOCK_ROLLBACK`，写路径被拒、导入通道可用。
5. 集群数达到 `max_managed_clusters` 再创建 → 403 `LICENSE_LIMIT_EXCEEDED`；删除集群释放额度后可再创建。
6. 无 license / 空文件 / 截断文件 / base64 损坏 → 明确错误且进程不 panic。
7. 测试密钥签发的 license（key_id 不在公钥表）→ 拒绝。
8. 密钥轮换：旧 key_id license 仍可验证（公钥表含双 key）。

## 11. 发布与运维流程

> **部署要求（容器化）**：`data/` 目录必须挂持久卷（Docker volume / K8s PVC），设备密钥、license、时间水位均在卷内；未持久化会导致 Pod 重建后授权失效，需重新激活。裸机/传统部署无此限制（data/ 即本地目录）。

1. 授权方运行 `keygen` 生成生产私钥 → 公钥写入产品常量 → 随版本发布。
2. 客户部署后运行 `stellar license request`（或前端页面）生成激活请求 → 提交工单。
3. 授权方审核订单后用 `issue` 签发 license.lic → 人工交付。
4. 客户导入即生效。续期 = 重新签发（新 expires_at、同设备公钥），无需重装。
5. 私钥保管：加密离线备份，禁止进 Git/CI/镜像；泄露即轮换（新 key_id + 重新签发所有在途授权）。
6. 仓库策略：签发工具代码随主仓库版本管理（`backend/src/bin/license-issuer.rs`），不单独建私有仓库——工具代码不涉密（无私钥则无法伪造授权，格式公开亦无害），真正需保密的是私钥（第 5 条）；镜像构建不含该 bin target，客户侧无法获得工具。若主仓库将来开源，再迁移至独立私有仓库并抽出共享格式 crate 防漂移。

## 12. 里程碑

| 阶段 | 内容 |
|---|---|
| M1 | 授权文件格式 + 签发工具 + LicenseService 验签/状态/导入 + `create_cluster` 容量限制 + 路由与测试（§4-§8、§10） |
| M2 | 时间水位与回退检测 + 写路径网关 + 前端状态/导入页（§7、§9） |
| M3（可选） | 按组织授权、功能模块开关（当前 YAGNI，需时再扩） |