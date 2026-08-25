# ai-bid — 智能标书审核系统

基于 Multi-Agent 架构的智能标书合规性审核平台。前后端分离 monorepo。

## 架构总览

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   frontend   │────▶│ backend-java │────▶│ backend-rust │
│  React 5173  │     │ Spring 3000  │     │  Axum 3001   │
└──────────────┘     └──────┬───────┘     └──────┬───────┘
                            │                    │
                      ┌─────┴─────┐        ┌─────┴─────┐
                      │ MySQL 3306│        │  Qdrant   │
                      │ Redis 6379│        │  6333     │
                      │  Neo4j    │
                      │ 7474/7687 │
                      └───────────┘        └───────────┘
```

| 层 | 技术 | 端口 |
|---|---|---|
| 前端 | React 18 + TypeScript + Vite | 5173 |
| 业务网关 | Spring Boot 3 + MyBatis-Plus | 3000 |
| AI 引擎 | Rust + Tokio + Axum | 3001 |
| 数据库 | MySQL 8.0 | 3306 |
| 缓存 | Redis 7.2 | 6379 |
| 图数据库 | Neo4j 5.21 | 7474/7687 |
| 向量库 | Qdrant 1.7 | 6333 |
| 文档转换 | JODConverter + LibreOffice | 8088 |

## 目录结构

| 目录 | 说明 |
|---|---|
| `backend-rust/` | Rust 后端：CLI 工具 + Multi-Agent 审核引擎 + HTTP API |
| `backend-java/` | Java 后端：Spring Boot 业务平台（认证/CRUD/文件管理/SSE） |
| `frontend/` | React 前端（Vite + TypeScript + pnpm） |
| `benchmark/` | 审核质量评估脚本与数据集 |
| `docs/` | 项目文档（中文） |
| `models/` | ONNX 嵌入模型文件（~568MB，自动下载） |

---

## 快速启动

### 1. 环境要求

| 工具 | 版本要求 |
|---|---|
| JDK | 17+ |
| Maven | 3.8+ |
| Rust | 1.80+ (2024 edition) |
| Node.js | 18+ |
| pnpm | 8+ |
| Docker | 24+ (含 Compose v2) |

### 2. 启动基础设施（Docker）

MySQL、Redis、Qdrant 等依赖服务通过 Docker Compose 一键启动：

```bash
# 进入 Java 资源目录
cd backend-java/src/main/resources

# 启动所有基础设施
docker compose up -d

# 查看运行状态
docker compose ps
```

启动的容器：

| 容器 | 端口 | 用途 |
|---|---|---|
| smart-mysql | 3306 | MySQL 8.0（数据库 `smart_tender_system`） |
| smart-redis | 6379 | Redis 7.2（缓存 / SSE / 任务队列） |
| smart-qdrant | 6333/6334 | Qdrant 向量数据库（6333 REST + Dashboard，6334 gRPC） |
| doc-converter | 8088 | DOCX → PDF 转换服务（默认注释，见下方说明） |
| my-neo4j | 7474/7687 | Neo4j 5.21 图数据库（凭据 `neo4j/b1234567`，APOC 插件） |

> **注意**：Qdrant Web Dashboard 地址为 http://localhost:6333/dashboard，可直接在浏览器查看向量数据。doc-converter 镜像（ghcr.io）国内拉取较慢，默认在 `docker-compose.yml` 中注释，期间 DOCX 转换由 Rust 侧处理，PDF 文件不受影响。

### 3. 配置环境变量

在项目根目录创建 `.env` 文件：

```bash
# ==== 必填：LLM API 密钥 ====
DASHSCOPE_API_KEY=sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# ==== LLM 协议（dashscope / openai_compatible）====
AIBID_LLM_PROTOCOL=dashscope

# ==== OpenAI 兼容接口（仅 openai_compatible 协议时使用）====
# OPENAI_API_KEY=sk-xxxxxxxx
# OPENAI_BASE_URL=https://api.openai.com/v1

# ==== 搜索后端（dashscope / searxng）====
AIBID_SEARCH_BACKEND=dashscope

# ==== 嵌入引擎：推荐 remote（无需下载模型），local 需要 544MB ONNX 模型 ====
EMBED_ENGINE=remote

# ==== Multi-Agent 审核 ====
AIBID_AGENT=1
AIBID_COORDINATOR=1

# ==== 数据根目录（从 backend-rust/ 运行时设为 ..）====
# AIBID_DATA_DIR=..
```

环境变量说明：

| 变量 | 默认值 | 说明 |
|---|---|---|
| `DASHSCOPE_API_KEY` | — | 阿里云 DashScope API 密钥（**必填**） |
| `AIBID_LLM_PROTOCOL` | `dashscope` | LLM 协议：`dashscope` 或 `openai_compatible` |
| `AIBID_SEARCH_BACKEND` | `dashscope` | 搜索后端：`dashscope`（联网搜索）或 `searxng`（自托管） |
| `EMBED_ENGINE` | `local` | 嵌入引擎：详见下方 [嵌入引擎选择](#嵌入引擎选择) |
| `AIBID_AGENT` | — | 设为 `1` 启用 Multi-Agent 模式 |
| `AIBID_COORDINATOR` | — | 设为 `1` 启用 Coordinator 7 阶段管线 |
| `AIBID_DATA_DIR` | `.` | 数据根目录，从 `backend-rust/` 运行时设为 `..` |

#### 嵌入引擎选择

| 模式 | 配置 | 优点 | 缺点 |
|---|---|---|---|
| **remote**（推荐） | `EMBED_ENGINE=remote` | 无需下载模型，开箱即用 | 需要网络，消耗 API 额度 |
| local | `EMBED_ENGINE=local` | 本地推理，不消耗 API 额度 | 需下载 544MB ONNX 模型，占用 ~1.2GB 内存 |

> **给同学的建议**：直接用 `EMBED_ENGINE=remote`，省去下载 544MB 模型的步骤，只要 `DASHSCOPE_API_KEY` 配置正确就能跑。
>
> 本地模式适合需要离线推理或大量调用不想消耗 API 额度的场景。首次启动时程序会自动从 HuggingFace 下载模型到 `backend-rust/models/`，模型文件已加入 `.gitignore`，不会提交到仓库。

### 4. 启动 Rust AI 引擎

```bash
cd backend-rust

# 编译检查
cargo check

# 启动 HTTP API 服务器（端口 3001）
# 如果在 backend-rust/ 目录运行，设置数据目录指向项目根
$env:AIBID_DATA_DIR=".."
cargo run --bin server
```

**CLI 模式**（不启动 API 服务器，直接审核单个文件）：

```bash
cd backend-rust
$env:AIBID_DATA_DIR=".."
cargo run -- <投标文件.pdf>
cargo run -- --chat <投标文件.pdf>    # 交互式对话模式
```

**验证 LLM 连接**：

```bash
cargo run --bin test_llm
```

**运行 Agent 集成测试**：

```bash
cargo test --bin test_agents -- --test all
```

### 5. 启动 Java 业务网关

```bash
cd backend-java

# 编译
mvn clean package -DskipTests

# 启动（端口 3000）
java -jar target/smart_tender-0.0.1-SNAPSHOT.jar
```

或者用 Maven 直接运行：

```bash
mvn spring-boot:run
```

> **依赖检查**：启动前确保 MySQL 和 Redis 已运行（步骤 2），Rust API 服务器已启动（步骤 4）。

### 6. 启动前端

```bash
cd frontend

# 安装依赖
pnpm install

# 启动开发服务器（端口 5173）
pnpm dev
```

浏览器打开 `http://localhost:5173`。

---

## 启动顺序总结

```
1. Docker 基础设施  →  MySQL + Redis + Qdrant + Neo4j
2. .env 配置        →  填写 API 密钥和环境变量
3. Rust 引擎 :3001  →  AI 审核 / 嵌入 / LLM 调用
4. Java 网关 :3000  →  认证 / CRUD / SSE 推送
5. React 前端 :5173 →  Web 界面
```

**全量启动命令一览**：

```bash
# 终端 1：基础设施
cd backend-java/src/main/resources && docker compose up -d

#终端:先导入个mysql
cmd /c "docker exec -i smart-mysql mysql -uroot -p1234 smart_tender_system < D:/github-remote/hsd/ai-bid/backend-java/src/main/resources/sql/smart_tender.sql"

让 CMD 来执行 < 重定向（CMD 原生支持，不会搞坏编码），PowerShell 只是调用一下 CMD。

预期输出：成功的标志是只出现一个 Warning，没有任何 ERROR
mysql: [Warning] Using a password on the command line interface can be insecure.
这个 Warning 是正常的（因为用了 -p1234 明文密码）。

# 终端 2：Rust AI 引擎
cd backend-rust
$env:AIBID_DATA_DIR=".."
cargo run --bin server

# 终端 3：Java 业务网关
cd backend-java
mvn spring-boot:run

# 终端 4：前端
cd frontend
pnpm install && pnpm dev
```

---

## 技术栈

| 层 | 技术 |
|---|---|
| AI 引擎 | Rust 2024, Tokio, Reqwest, Axum |
| 业务平台 | Java 17, Spring Boot 3, MyBatis-Plus, Druid |
| 前端 | React 18, TypeScript, Vite |
| LLM | DashScope (qwen-plus) 或 OpenAI 兼容接口 |
| 嵌入 | BGE-M3 ONNX 本地推理 或 DashScope text-embedding-v4 |
| 搜索 | DashScope 联网搜索 或 SearXNG 自托管 |
| 数据库 | MySQL 8.0 + Redis 7.2 + Qdrant 1.7 + Neo4j 5.21 |
| 文档转换 | JODConverter + LibreOffice |

## 前后端通信

```
浏览器 :5173  →  Java :3000/api  →  Rust :3001
                    (REST)            (REST + SSE)
```

- 认证/CRUD/文件管理 → Java 处理
- AI 审核/RAG 对话/语义搜索 → Java 透明代理到 Rust
- 审核进度 → Rust SSE 流 → Java SseHub → 前端实时渲染

## 文档

- [backend-rust/CLAUDE.md](backend-rust/CLAUDE.md) — Rust 引擎架构
- [backend-java/CLAUDE.md](backend-java/CLAUDE.md) — Java 网关架构
- [frontend/CLAUDE.md](frontend/CLAUDE.md) — 前端开发指南
- [docs/](docs/) — 项目文档（中文）
