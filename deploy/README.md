# ai-bid 容器化部署指南

一套「**push 即自动更新 + 一键回滚**」的 Docker 部署方案。服务器只装 Docker，不装 Rust / JDK / Node 环境。

## 一、架构

```
浏览器 → frontend (Nginx, 端口 8080)
              │  反代 /api 与 /api/chat/stream(SSE)
              ▼
        backend-java (Spring Boot, 内部 3000)
              │  ┌── MySQL 8（smart_tender_system，数据卷持久化）
              │  ├── Redis 7（db 10）
              └──▶ backend-rust (Axum AI 引擎, 内部 3001)
                        ├── Qdrant 向量库
                        ├── Neo4j 图库
                        └── DashScope / OpenAI 兼容 LLM
```

| 服务 | 容器内端口 | 对外端口 | 数据卷 |
|---|---|---|---|
| frontend | 80 | `FRONTEND_PORT`(8080) | — |
| backend-java | 3000 | 不暴露 | app_data（上传文件） |
| backend-rust | 3001 | 不暴露 | rust_data（中间产物） |
| mysql | 3306 | `MYSQL_PORT`(3306) | mysql_data |
| redis | 6379 | 不暴露 | redis_data |
| qdrant | 6333/6334 | 不暴露 | qdrant_data |
| neo4j | 7474/7687 | 不暴露 | neo4j_data |

所有数据卷都是「项目级命名卷」（带 `ai-bid_` 前缀），多项目互不干扰。

## 二、目录结构

```
ai-bid/
├── frontend/
│   ├── Dockerfile        # Node 构建 → Nginx 托管
│   ├── nginx.conf        # 反代 /api + SSE
│   └── .dockerignore
├── backend-java/
│   ├── Dockerfile        # Maven 构建 → JRE 运行
│   └── .dockerignore
├── backend-rust/
│   ├── Dockerfile        # cargo 构建 → 精简运行
│   └── .dockerignore
└── deploy/
    ├── docker-compose.yml # 全栈编排
    ├── .env.example       # 环境变量模板
    ├── update.sh          # 自动更新
    ├── rollback.sh        # 回滚
    └── README.md          # 本文件
```

## 三、首次部署

### 前置条件

- 服务器已装 Docker + Compose 插件（你已满足）
- 服务器能访问 GitHub 和 Docker Hub（你已满足）
- git 能免密 pull 你的仓库（https 配好凭证，或 SSH key）

### 步骤

```bash
# 1. 克隆仓库（放在你规划的应用目录，例如 /srv/apps/）
mkdir -p /srv/apps && cd /srv/apps
git clone https://github.com/dgut-hsd/ai-bid.git
cd ai-bid
git checkout main        # 确保在 main 分支（部署用这个分支）
cd deploy

# 2. 生成环境变量并填写密钥
cp .env.example .env
nano .env    # 改 MySQL 密码、JWT secret、DashScope key、Neo4j 密码等

# 3. 脚本授权 + 转换换行（Windows 编辑的脚本可能带 \r）
chmod +x update.sh rollback.sh
sed -i 's/\r$//' update.sh rollback.sh

# 4. 首次构建并启动（Rust 首次编译较慢，约 10~30 分钟）
docker compose up -d --build

# 5. 查看状态
docker compose ps
docker compose logs -f backend-rust   # 看 AI 引擎是否就绪
```

> 首次构建完成后，把当前版本记入历史，后续回滚才有基线：
> ```bash
> git rev-parse HEAD >> .deploy-history
> ```

## 四、日常使用

### 更新（push main 后服务器自动/手动更新）

```bash
cd /srv/apps/ai-bid/deploy
./update.sh          # 检查并更新
./update.sh --force  # 强制更新（清除回滚保持）
```

### 回滚

```bash
./rollback.sh                 # 回退到上一个成功部署的版本
./rollback.sh <commit-hash>   # 回退到指定 commit
```

回滚后脚本会自动创建 `.rollback-hold` 标记，**暂停自动更新**，避免把你立刻拉回 main。排查完问题后：

```bash
rm .rollback-hold        # 或直接
./update.sh --force      # 恢复追 main 最新版
```

## 五、开启「push 即自动更新」（服务器主动拉取）

你的服务器在内网，GitHub 连不进来，所以用「服务器定时主动拉取」实现自动部署：

```bash
# 用 cron 每 2 分钟检查一次 main 是否有新提交
crontab -e
# 加一行（路径改成你的实际路径）：
*/2 * * * * cd /srv/apps/ai-bid/deploy && ./update.sh >> /tmp/ai-bid-update.log 2>&1
```

也可以换 systemd timer，但 cron 最简单。push 后最多 2 分钟服务器自动完成构建和上线。

## 六、多项目（A/B/C）不冲突

每个项目一套独立目录 + 独立 compose（`name:` 项目名），天然隔离。冲突点只有**对外端口**，改 `.env` 即可：

| 项目 | FRONTEND_PORT | MYSQL_PORT |
|---|---|---|
| ai-bid（本项目） | 8080 | 3306 |
| 项目 B | 8081 | 3307 |
| 项目 C | 8082 | 3308 |

内部服务（Java/Rust/Redis/Qdrant/Neo4j）**不暴露宿主机端口**，只在各自网络内经服务名互通，不会和别的项目打架。

## 七、常见问题

**Q：`update.sh` 报 `$'\r': command not found`**
脚本从 Windows 带入 CRLF 换行，执行 `sed -i 's/\r$//' deploy/*.sh` 即可。

**Q：Rust 构建报错（cmake / protobuf / nasm 相关）**
Dockerfile 已预装常见依赖；若新引入的 crate 报缺库，按报错补 `apt-get install` 对应包，改 `backend-rust/Dockerfile` 后重新 build。

**Q：`EMBED_ENGINE=remote` 和 `local` 有什么区别？**
- `remote`（推荐）：用远程嵌入接口，无需下载 568MB BGE-M3 模型，启动快、省内存。
- `local`：本地 ONNX 推理，首次会下载模型，吃内存，但离线可用。

**Q：MySQL 数据会不会在更新时丢失？**
不会。数据在命名卷 `ai-bid_mysql_data` 里，`docker compose up` 不重建卷；只有显式 `docker compose down -v` 才会删卷。

**Q：改了后端代码，更新慢吗？**
`update.sh` 用 `--build`，Docker 层缓存命中时只重建变化层。Rust 依赖层缓存后，改业务代码的增量编译会快很多。

**Q：想看容器日志/进容器排查**
```bash
docker compose -f deploy/docker-compose.yml logs -f backend-java
docker compose -f deploy/docker-compose.yml exec mysql mysql -uroot -p
```