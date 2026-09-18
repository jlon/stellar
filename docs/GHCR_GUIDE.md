# GitHub Container Registry (GHCR) 使用指南

## 什么是 GitHub Container Registry?

GitHub Container Registry (ghcr.io) 是 GitHub 提供的容器镜像托管服务，相当于你自己的 Docker Hub。

### 核心优势

- ✅ **完全免费** - 公开镜像无限存储和流量
- ✅ **无缝集成** - 代码和镜像在同一个仓库管理
- ✅ **自动构建** - 代码提交后自动构建并推送镜像
- ✅ **权限管理** - 细粒度的访问控制
- ✅ **多架构支持** - 支持 AMD64/ARM64 等多平台

## 工作流程

```mermaid
%%{init: {'theme':'dark', 'themeVariables': { 'primaryColor':'#238636','primaryTextColor':'#c9d1d9','primaryBorderColor':'#30363d','lineColor':'#58a6ff','secondaryColor':'#161b22','tertiaryColor':'#0d1117','background':'#0d1117','mainBkg':'#161b22','textColor':'#c9d1d9','fontSize':'16px','fontFamily':'Arial'}}}%%
flowchart TB
    A[代码提交到 GitHub] --> B{触发条件?}
    B -->|打 Tag v1.0.0| C[GitHub Actions 启动]
    B -->|手动触发| C
    
    C --> D[拉取代码]
    D --> E[构建 Docker 镜像]
    E --> F[推送到 ghcr.io]
    F --> G[镜像可用]
    
    G --> H[开发者拉取使用]
    G --> I[部署到生产环境]
    
    style A fill:#238636,stroke:#30363d,color:#fff,stroke-width:2px
    style C fill:#1f6feb,stroke:#30363d,color:#fff,stroke-width:2px
    style F fill:#da3633,stroke:#30363d,color:#fff,stroke-width:2px
    style G fill:#bf8700,stroke:#30363d,color:#fff,stroke-width:2px
```

## 快速开始

### 1. 首次使用（已自动配置）

项目已经配置好了 `.github/workflows/docker-publish.yml`，无需额外操作。

### 2. 触发构建

有三种方式触发自动构建：

#### 方式 1: 打版本标签（推荐）

```bash
# 创建并推送版本标签
git tag v1.0.0
git push origin v1.0.0

# 自动生成的镜像标签:
# - ghcr.io/jlon/stellar:1.0.0
# - ghcr.io/jlon/stellar:1.0
# - ghcr.io/jlon/stellar:1
# - ghcr.io/jlon/stellar:latest
```

`latest` 只会由稳定版标签更新；预发布标签不会覆盖它。

#### 方式 2: 手动触发

1. 打开 GitHub 仓库页面并点击 **Actions**。
2. 选择 **Docker Build and Publish** workflow。
3. 在 **Use workflow from** 中选择已有的 `vX.Y.Z` 标签，再点击 **Run workflow**。

手动运行不会接受分支名：镜像标签必须与已验证的发布版本一致。

### 3. 查看构建状态

1. 打开仓库的 **Actions** 标签
2. 查看最新的 workflow 运行记录
3. 点击查看详细日志和构建摘要

### 4. 使用镜像

#### 拉取镜像

```bash
# 拉取最新版本
docker pull ghcr.io/jlon/stellar:latest

# 拉取特定版本
docker pull ghcr.io/jlon/stellar:1.0.0

# 如果是私有镜像，需要先登录
echo $GITHUB_TOKEN | docker login ghcr.io -u USERNAME --password-stdin
```

#### 运行容器

```bash
docker run -d \
  --name stellar \
  -p 8080:8080 \
  -v $(pwd)/data:/data \
  ghcr.io/jlon/stellar:latest
```

首次启动在 `docker logs stellar` 中打印一次性管理员密码（默认用户 `admin`）。

## 镜像标签说明

### 自动生成的标签

| 触发方式 | 生成的标签 | 示例 |
|---------|-----------|------|
| 推送稳定版 `v1.2.3` 标签 | `1.2.3`, `1.2`, `1`, `latest` | `ghcr.io/jlon/stellar:1.2.3` |
| 推送预发布标签 | 对应的 SemVer 标签，不更新 `latest` | `ghcr.io/jlon/stellar:1.2.3-rc.1` |

### 推荐的标签使用策略

- **生产环境**: 使用具体版本号 `1.2.3`
- **测试环境**: 使用预发布版本号
- **开发环境**: 使用 `latest`

## 高级配置

### 1. 多架构构建（可选）

如果需要支持 ARM 架构（如 Apple M1/M2），可以修改 workflow:

```yaml
# In .github/workflows/docker-publish.yml
- name: Build and push Docker image
  uses: docker/build-push-action@v5
  with:
    platforms: linux/amd64,linux/arm64  # 添加这行
    # ... other options
```

### 2. 构建优化

已启用的优化:
- ✅ GitHub Actions 缓存 (加速重复构建)
- ✅ Docker BuildKit
- ✅ 多层缓存策略

### 3. 镜像可见性设置

默认情况下，镜像的可见性继承自仓库:
- 公开仓库 → 公开镜像
- 私有仓库 → 私有镜像

修改镜像可见性:
1. 访问 `https://github.com/jlon/stellar/pkgs/container/stellar`
2. 点击 **Package settings**
3. 修改 **Danger Zone** 中的可见性设置

## 常见问题

### Q1: 构建失败怎么办?

1. 检查 Actions 日志中的错误信息
2. 常见原因:
   - Dockerfile 路径错误
   - 构建依赖缺失
   - 构建超时（免费版有时间限制）

### Q2: 如何删除旧镜像?

1. 访问 `https://github.com/jlon/stellar/pkgs/container/stellar`
2. 选择要删除的版本
3. 点击 **Delete** 按钮

### Q3: 如何给其他人镜像访问权限?

对于私有镜像:
1. 访问包设置页面
2. 点击 **Manage access**
3. 添加用户或团队

### Q4: 构建太慢怎么办?

优化建议:
1. 使用 `.dockerignore` 排除不必要的文件
2. 优化 Dockerfile 层顺序（不常变的层放前面）
3. 使用构建缓存
4. 考虑使用自托管 runner

## 与 Docker Hub 对比

| 特性 | GitHub Container Registry | Docker Hub |
|------|--------------------------|------------|
| 公开镜像存储 | ✅ 无限 | ✅ 无限 |
| 私有镜像 | ✅ 免费（有限额度） | ⚠️ 收费 |
| 与代码集成 | ✅ 原生集成 | ❌ 需要配置 |
| 构建速度 | ✅ 快（在同一网络） | ⚠️ 一般 |
| 访问速度（国内） | ⚠️ 需要代理 | ⚠️ 需要代理 |

## 更新 README

建议更新 README.md 中的 Docker 部署说明:

```bash
# 使用 GitHub Container Registry 镜像（推荐）
docker pull ghcr.io/jlon/stellar:latest
docker run -d -p 8080:8080 --name stellar \
  -v $(pwd)/data:/data \
  ghcr.io/jlon/stellar:latest

# 或者使用 Docker Hub 镜像
docker pull docker.io/johndockerhub/stellar:latest
```

## 监控和统计

查看镜像使用统计:
1. 访问 `https://github.com/jlon/stellar/pkgs/container/stellar`
2. 查看 **Insights** 标签
3. 可以看到:
   - 拉取次数
   - 下载量
   - 版本分布

## 架构图

```mermaid
%%{init: {'theme':'dark', 'themeVariables': { 'primaryColor':'#238636','primaryTextColor':'#c9d1d9','primaryBorderColor':'#30363d','lineColor':'#58a6ff','secondaryColor':'#161b22','tertiaryColor':'#0d1117','background':'#0d1117','mainBkg':'#161b22','textColor':'#c9d1d9','fontSize':'14px'}}}%%
graph TB
    subgraph GitHub["GitHub 平台"]
        Code[代码仓库<br/>jlon/stellar]
        Actions[GitHub Actions<br/>自动构建]
        GHCR[Container Registry<br/>ghcr.io]
    end
    
    subgraph Developers["开发者"]
        Dev1[开发者 A<br/>推送代码]
        Dev2[开发者 B<br/>拉取镜像]
    end
    
    subgraph Production["生产环境"]
        Server1[服务器 1]
        Server2[服务器 2]
        K8s[Kubernetes 集群]
    end
    
    Dev1 -->|git push| Code
    Code -->|触发| Actions
    Actions -->|构建推送| GHCR
    
    GHCR -->|docker pull| Dev2
    GHCR -->|docker pull| Server1
    GHCR -->|docker pull| Server2
    GHCR -->|docker pull| K8s
    
    style Code fill:#238636,stroke:#30363d,color:#fff
    style Actions fill:#1f6feb,stroke:#30363d,color:#fff
    style GHCR fill:#da3633,stroke:#30363d,color:#fff
```

## 下一步

1. ✅ 配置已完成
2. 📝 推送代码到 GitHub 测试构建
3. 🚀 打版本标签触发正式发布
4. 📊 监控镜像使用情况

---

**提示**: 所有配置已经完成，直接使用即可！
