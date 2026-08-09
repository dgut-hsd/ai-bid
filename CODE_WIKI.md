# ai-bid 智能标书审核系统 — Code Wiki

> 版本：2026-08-01 · 本文档为代码结构与架构说明，基于仓库当前状态梳理。

## 目录

- [1. 项目概览](#1-项目概览)
- [2. 整体架构](#2-整体架构)
- [3. 目录结构](#3-目录结构)
- [4. backend-rust — AI 引擎](#4-backend-rust--ai-引擎)
- [5. backend-java — 业务网关](#5-backend-java--业务网关)
- [6. frontend — React 前端](#6-frontend--react-前端)
- [7. benchmark — 审核质量评估](#7-benchmark--审核质量评估)
- [8. 依赖关系与数据流](#8-依赖关系与数据流)
- [9. 项目运行方式](#9-项目运行方式)
- [10. 关键设计要点与技术债](#10-关键设计要点与技术债)

---

## 1. 项目概览

**ai-bid** 是一套基于 **Multi-Agent 架构**的智能标书合规性审核平台，采用前后端分离的 monorepo 结构。系统面向采购人员，提供标书上传、AI 实时审核可视化、审核报告导出、知识库管理等功能。

### 技术栈

| 层 | 技术 | 端口 |
|---|---|---|
| 前端 | React 19 + TypeScript + Vite + Ant Design | 5173 |
| 业务网关 | Spring Boot 3.2 + MyBatis-Plus + Druid | 3000（prod 8086） |
| AI 引擎 | Rust 2024 (edition) + Tokio + Axum | 3001 |
| 数据库 | MySQL 8.0 | 3306 |
| 缓存/队列 | Redis 7.2 | 6379 |
| 向量库 | Milvus 2.6 | 19530 |
| 文档转换 | JODConverter + LibreOffice | 8088 |
| LLM | DashScope (qwen-plus) 或 OpenAI 兼容接口 | — |
| 嵌入 | BGE-M3 ONNX 本地推理 / DashScope text-embedding-v4 远程 | — |
| 搜索 | DashScope 联网搜索 / SearXNG 自托管 | — |

### 核心理念

Rust 引擎遵循 **"搜索优先，渐进积累"**：不预设庞大本地知识库，审查时才联网搜法规/案例/负面清单/标准范本，搜过的结果缓存下来渐进生长成知识库。无外部数据库依赖（内存 SessionGraph + JSON 文件）。

---

## 2. 整体架构

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   frontend   │────▶│ backend-java │────▶│ backend-rust │
│  React 5173  │     │ Spring 3000  │     │  Axum 3001   │
└──────────────┘     └──────┬───────┘     └──────┬───────┘
       │                    │                    │
       │ REST/SSE           │ REST/SSE 透明代理   │ 内存审核
       ▼                    ▼                    ▼
                 ┌───────────────┐        ┌─────────────┐
                 │ MySQL 3306    │        │  Milvus     │
                 │ Redis 6379    │        │  19530      │
                 └───────────────┘        └─────────────┘
```

### 三层职责划分

| 层 | 职责 | 不做 |
|---|---|---|
| **Java 业务网关** | 认证鉴权、CRUD、文件管理、审核任务编排、SSE 中继、报告生成、结果持久化 | AI 推理 |
| **Rust AI 引擎**（黑盒） | PDF 解析、向量嵌入、Multi-Agent 审核、RAG 对话、语义搜索 | 状态持久化 |
| **React 前端** | UI 渲染、SSE 实时展示、PDF 高亮、对话交互 | 业务逻辑 |

> **状态真实来源**：Java（MySQL）。Rust 是无状态内存服务，重启丢失内存结果后由 Java 从 `audit_issue` 表降级兜底。

### 通信协议

- 前端 → Java：RESTful JSON + SSE 流式
- Java → Rust：HTTP/1.1 + JSON（snake_case）+ SSE 流式中继
- 连接超时 5s，读取超时 900s（审核最长 15 分钟）

---

## 3. 目录结构

```
ai-bid/
├── backend-rust/          # Rust AI 引擎：CLI + Multi-Agent 审核引擎 + HTTP API
│   ├── src/
│   │   ├── agents/        # Multi-Agent 框架（17 子模块）
│   │   ├── api/           # Axum HTTP 路由与 handlers
│   │   ├── bin/           # 4 个可执行入口（server/test_agents/test_llm/test_api_key）
│   │   ├── domain/        # 领域模型（chunk/raw_document/vector_index）
│   │   ├── metrics/       # 指标采集（4 层测量契约）
│   │   ├── services/      # 服务层（PDF/嵌入/LLM/脱敏/章节/切分）
│   │   ├── lib.rs / main.rs / paths.rs
│   ├── agents/dynamic_agents.json   # 动态 Agent 配置（经验沉淀）
│   ├── docs/              # 设计/实现/API 参考/验证等文档
│   ├── scripts/           # Python 兜底脚本与测试
│   └── tests/             # 测试 PDF 与 fixtures
├── backend-java/          # Java 业务网关：Spring Boot 平台
│   ├── src/main/java/com/ithsd/smart_tender/
│   │   ├── config/        # 5 配置类（Web/Async/Rust/MyBatis/MetaObject）
│   │   ├── common/        # 横切组件（BaseContext/JWT/异常/TypeHandler）
│   │   ├── controller/    # 10 个 REST 控制器
│   │   ├── service/       # 业务服务（含 engine/rust 代理层 + queue 队列层）
│   │   ├── mapper/        # 13 个 MyBatis-Plus Mapper
│   │   ├── model/         # entity(13) + dto + vo + enums + result
│   │   └── sse/           # 5 个 SSE 实时推送组件
│   ├── src/main/resources/
│   │   ├── application.yml / application-prod.yml
│   │   ├── docker-compose.yml   # 基础设施容器
│   │   ├── sql/           # 表结构
│   │   ├── prompts/       # 6 个 Agent 提示词模板
│   │   └── logback-spring.xml
│   └── docs/              # Rust 引擎接口契约、Java 设计
├── frontend/              # React 前端
│   ├── src/
│   │   ├── api/           # Axios 实例 + 类型定义
│   │   ├── app/           # 路由 + 权限守卫
│   │   ├── components/    # 公共组件（layout/StatCard/VersionDrawer 等）
│   │   ├── features/      # 6 个功能模块（Feature-based）
│   │   ├── store/         # Redux（仅 Auth）
│   │   ├── hooks/ lib/ types/ theme/ styles/
│   │   └── App.tsx / main.tsx
│   └── docs/设计.md
├── benchmark/             # 审核质量评估（Python）
│   ├── evaluate.py / risk_policy.py / run_benchmark.py
│   ├── build_dataset.py / build_blind_v2.py / build_intern_report.py
│   └── blind-v2/          # 冻结盲测集
├── README.md / CLAUDE.md / .env
```

---

## 4. backend-rust — AI 引擎

Rust 引擎是整个系统的 AI 核心，使用 Tokio 异步运行时 + Axum HTTP 框架，承载 PDF 解析、向量嵌入、Multi-Agent 审核、RAG 对话等全部 AI 推理负载。

### 4.1 构建配置（Cargo.toml）

- **Package**：`ai-bid` v0.1.0，edition 2024，`default-run = "ai-bid"`
- **核心依赖**：tokio(full)、axum(multipart,macros)、reqwest(json,stream)、fastembed(ort-load-dynamic，BGE-M3 ONNX)、pdfplumber(serde)、regex、uuid、chrono、async-trait、utoipa(OpenAPI)、serde
- **Bin Targets（4 个）**：
  - `ai-bid`（默认，CLI 6 阶段管线）
  - `server`（HTTP 微服务，:3001）
  - `test_agents`（Agent 集成测试）
  - `test_llm` / `test_api_key`（LLM 连通测试）

### 4.2 入口与管线

#### `src/main.rs` — CLI 入口（6 阶段管线）

`#[tokio::main] async fn main()` 编排完整审核管线：

1. **阶段 1：PDF → RawDocument**（Rust pdfplumber 主路径 + Python 兜底）
2. **阶段 2：RawDocument → Sections**（sectionize + 表格检测 + 跨页合并）
3. **阶段 3：Sections → Chunks**（自适应语义切分）
4. **阶段 4：Chunks → Embedding**（`EMBED_ENGINE` 切换 local/remote）
5. **阶段 5：语义搜索验证**（5 条预设查询）
6. **阶段 6：Multi-Agent 合规审查**（需 `AIBID_AGENT=1`）
   - `--chat` → ChatAgent 交互模式
   - `AIBID_COORDINATOR=1` → Coordinator 多 Agent 7 阶段管线 + BlindSpot 异步
   - 否则 → 单 Agent 模式（向后兼容）

#### `src/bin/server.rs` — HTTP 服务入口

加载 .env → `AppState::init()` → `router::build(state)` → `TcpListener::bind("127.0.0.1:3001")` → `axum::serve`

#### `src/paths.rs` — 路径管理

统一路径入口，禁硬编码相对路径。`data_dir()` 读 `AIBID_DATA_DIR`（默认 `.`），monorepo 中从 `backend-rust/` 运行时设为 `..` 指向项目根。

### 4.3 agents/ — Multi-Agent 框架（核心）

#### 4.3.1 协调器 `coordinator.rs`（7 阶段管线）

编排 Multi-Agent 审查，实现 **ROUTE → PRELOAD → EXECUTE → MERGE → LEGAL_VERIFY → DEBATE → TRIAGE** 7 阶段。

| 方法 | 职责 |
|---|---|
| `review(&clauses) -> CoordinatorOutput` | 主入口 |
| `route_clauses(clauses)` | 关键词路由 clauses → 各 Agent |
| `preload_chunks/preload_agents` | SessionGraph 预加载 |
| `batch_search_phase` | 基于 Scout 假设的预搜索 |
| `execute_agents(routing)` | 并行执行 Agent |
| `merge_findings_v3` | 去重合并 + 跨 Agent 关联推导 |
| `legal_verify(merged)` | 法条对抗验证 |
| `debate_high_risk(merged)` | 高风险低置信度辩论 |
| `triage(merged)` | 严重度+置信度排序 |
| `run_blind_spot` | 后台异步盲点扫描（经验沉淀） |

```rust
pub struct CoordinatorOutput {
    pub findings: Vec<RiskFinding>,
    pub routing_summary: RoutingSummary,
    pub graph_snapshot: Option<GraphSnapshot>,
}
```

#### 4.3.2 ReAct 推理循环 `react_loop.rs`

- **`trait LlmClient: Send + Sync`** — `async fn chat(messages, tools, tool_choice) -> LlmResponse`
- `ChatMessage` 枚举：System / User / Assistant{content, tool_calls} / Tool{tool_call_id, content}
- `ToolChoice`：Auto / Required / Specific{name}
- `react_loop(&clause, &risk_id)` — 单条款 ReAct 循环：查 SessionGraph 上下文 → poll AgentBus → LLM 推理 → 处理 output_finding/工具调用 → 动态 tier 升级
- **轮次策略**：倒数第 3 轮汇总提示；倒数第 2 轮 `ToolChoice::Required`；最后 1 轮强制 `output_finding`

#### 4.3.3 会话知识图谱 `session_graph.rs`（Blackboard）

`RwLock` 保护的内存图，多类边：
- `reviewed_by`：chunk → 已审查 Agent 列表
- `has_risk`：chunk → 已知风险列表
- `risks`：risk_id → RiskNode
- `linked_to`：chunk → 关联条款
- `cited_by`：法条引用 → 反向索引风险
- `contradicts`：chunk → 已知矛盾

核心方法 `query_clause_context(chunk_id) -> ClauseContext` 返回完整上下文（已审查 Agent、已知风险、关联条款、引用相同法条的条款、矛盾）。

#### 4.3.4 Agent 注册表 `registry.rs`

`AgentRegistry::builtin()` 注册 11 个内置 Agent：

| AgentId | display_name | max_turns | complexity | 职责 |
|---|---|---|---|---|
| FactCheck | 事实核查 | 10 | Medium | 资质/证书/业绩核查 |
| Procedure | 采购程序审查 | 12 | Medium | 时间节点/流程合规 |
| RuleEngine | 硬性规则引擎 | 14 | Low | 禁止性条款匹配 |
| SemanticRisk | 隐性风险识别 | 14 | High | 隐性歧视/模糊表述 |
| Scout | 初筛 | 3 | Low | STS Phase 0 假设生成 |
| Scoring | 评分合规审查 | 10 | Medium | 评分标准合理性 |
| Demand | 技术需求审查 | 12 | Medium | 需求合理性 |
| Contract | 合同条款审查 | 12 | Medium | 合同风险评估 |
| LegalVerify | 法条验证 | 8 | Low | 法条对抗验证 |
| Debate | 正反辩论 | 8 | High | 高风险交叉验证 |
| BlindSpot | 盲点复查 | 10 | High | 遗漏风险扫描 |

审查层级：**L1 批量快筛**（200 条/次，秒级）→ **L2 定向深审**（~20 条/次）→ **L3 对抗验证**（~5 条/次）。支持 `register_dynamic` 加载动态 Agent。

#### 4.3.5 Agent 工具集 `tools/`

核心抽象：

```rust
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> serde_json::Value;  // JSON Schema
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value>;
}
pub struct ToolRegistry { tools: HashMap<String, Box<dyn AgentTool>> }
```

| 工具 | 类型 | 说明 |
|---|---|---|
| `search_knowledge`/`web_search` | 通用 | 外部知识搜索（SearXNG/Tavily/DashScope/Mock + SearchBuffer 缓冲池） |
| `search_document` | 通用 | 标书内部语义搜索（嵌入+暴力 KNN Top-5） |
| `read_section` | 通用 | 按 chunk_id 精读原文（含相邻上下文） |
| `output_finding` | 终端 | 批量输出审查结论（最多 5 条 finding） |
| `answer_user` | 终端 | ChatAgent 自然语言回答 |
| `validate_calculation` | 专用 | 数值计算验证 |
| `check_cross_reference` | 专用 | 交叉引用完整性 |
| `calculate_timeline` | 专用 | 时间线计算校验 |
| `compare_with_template` | 专用 | 模板比对 |
| `search_contradiction` | 专用 | 矛盾检测 |
| `extract_obligations` | 专用 | 投标人义务聚合 |

**SearchBuffer（搜索缓冲池）**：跨 Agent 查询去重 + 单 worker 串行消费 + 空结果退避重试 + broadcast 广播，保护下游引擎不被限流。

#### 4.3.6 消息与事件

- **`bus.rs`（AgentBus）**：`tokio::broadcast` 实现，仅广播 `severity=High` 消息，按 `risk_type` 选择 topic，每个 Agent 持有专属 Receiver。
- **`review_event.rs`（ReviewEventBus）**：按 doc_id 隔离，`ReviewEvent` 9 变体（Phase/AgentProgress/Trace/FindingAdded/FindingUpdated/FindingRemoved/Stats/Done/Error），`PipelinePhase` 8 阶段。

#### 4.3.7 其他 Agent 文件

- `scout.rs` — Scout 初筛 Agent（STS Phase 0），confidence 0.4-0.5，输出 Hypothesis 引导后续 Agent
- `risk_taxonomy.rs` — 统一风险分类（15 个 `category_code`）+ 证据准入 + 重大问题策略
- `trace.rs` — 审查追溯日志（11 种 `TraceEventType`）
- `chat_agent.rs` — 交互式对话 Agent（RAG 自动注入）
- `fact_check.rs` / `procedure.rs` / `semantic_risk.rs` — Agent 工厂函数
- `prompts.rs` — 11 个系统提示词
- `testing.rs` — 集成测试基础设施

### 4.4 核心类型 `types.rs`

- **`RiskFinding`**（40+ 字段）：风险发现，含 STS 架构字段（`finding_role`/`knowledge_source`/`hypothesized_by`/`verified_by`）、tier 追踪（`initial_tier`/`final_tier`/`tier_escalated`/`truncated`）、定位字段（`page_number`/`section_path`/`context`/`block_ids`/`clause_ids`）
- `enum RiskSeverity { High, Medium, Low, Info }`
- `enum RiskTier` — 审查复杂度分级
- `enum AgentId` — 11 内置 + Dynamic(String)
- `CoordinatorConfig` / `CoordinatorOutput` / `RoutingSummary` / `GraphSnapshot`
- `ReviewClause` — 审查条款（`from_chunk` 构造）
- `ChatAgentConfig` / `ChatResponse` / `ChatStreamEvent`
- `Citation` / `SuggestedAgent` / `BBox` / `BlockRef` / `KnowledgeRef`

### 4.5 api/ — HTTP API（Axum）

#### 路由清单 `router.rs`

`build(state: AppState) -> Router`，CORS 全开放，`DefaultBodyLimit::max(50MB)`。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/health` | 健康检查 |
| POST | `/api/v1/documents` | 上传并解析文档（multipart） |
| GET | `/api/v1/documents/:id` | 获取文档信息 |
| POST | `/api/v1/documents/:id/review` | 启动异步 Multi-Agent 审核（202） |
| POST | `/api/v1/documents/:id/chat` | 与文档对话（非流式 RAG） |
| POST | `/api/v1/documents/:id/chat/stream` | 与文档对话（SSE 流式） |
| POST | `/api/v1/documents/:id/search` | 语义搜索 |
| GET | `/api/v1/documents/:id/blocks` | 获取块 BBox 坐标（前端高亮） |
| GET | `/api/v1/review/:doc_id/stream` | 审核进度 SSE 流 |
| GET | `/api/v1/review/:doc_id/result` | 获取审核结果 |

SSE 事件类型：`phase` / `agent_progress` / `trace` / `finding_added` / `finding_updated` / `finding_removed` / `stats` / `done` / `error`。

#### handlers.rs — 关键 handler

- **`process_document`**：multipart 上传 → DOCX 转 PDF → PDF 提取 → sectionize → chunking → 脱敏 → embedding → 缓存 `DocumentState`
- **`review_document`**：并发控制 → 创建 ReviewEventBus → `tokio::spawn(run_review_pipeline)` → 立即返回 202
- **`run_review_pipeline`**（后台）：构建 Coordinator → review → 恢复脱敏原文 → 填充 block_ids → 写盘 → emit Done/Error
- **`get_review_result`**：内存 → 错误 → pending → 磁盘 fallback 四级查询

**AppState**（全局共享）：`documents`（文档缓存）、`embed_client`、`review_event_buses`、`review_results`、`active_reviews`。

**脱敏双视图设计**：`chunks`（原文，本地定位）+ `review_chunks`（脱敏副本，远程模型接触），`restore_chat_response` 返回前恢复原文。

### 4.6 domain/ — 领域模型

- **`raw_document.rs`**：PDF 解析中间表示。`RawDocument { document_id, source_path, pages: Vec<RawPage> }`，ID 命名 `w_{页}_{序}`/`b_{页}_{序}`/`t_{页}_{序}`，`BBox { x0, top, x1, bottom }`
- **`chunk.rs`**：`Chunk { chunk_id, chunk_type, section_path, text, page_start, page_end, source_block_ids, bbox_refs }`，`ChunkType`：Leaf/Merged/Split。`ChunkingConfig` 默认 merge_min_len=120、split_max_len=1500、split_overlap=200
- **`vector_index.rs`**：`DocumentVectorIndex`，内存暴力 KNN（200×1024d ≈ 0.8MB，<0.5ms），自动 L2 归一化

### 4.7 services/ — 业务服务层

| 服务 | 核心职责 |
|---|---|
| `llm_client.rs` | `create_llm_client()` 工厂，按 `AIBID_LLM_PROTOCOL` 切换 DashScopeNative / OpenAICompatible，解析 `reasoning_content`（推理模型）+ `tool_calls` |
| `embedding_service.rs` | BGE-M3 嵌入（fastembed-rs 纯 Rust ONNX），BATCH_SIZE=32，支持本地并行 / 远程 text-embedding-v4 |
| `pdf_extract_service.rs` | 双引擎：Rust pdfplumber 主路径 + Python 兜底（子进程），文本清洗（CJK 空格合并、多空格归一）、按行间距聚合 |
| `sectionize_service.rs` | 7 种中文标题模式 → 嵌套章节树，含表格检测/跨页合并/注入 |
| `chunking_service.rs` | `chunk_sections` + `populate_bbox_refs` |
| `docx_convert_service.rs` | LibreOffice headless DOCX→PDF |
| `embedding_api_client.rs` | 远程嵌入 API 客户端 |
| `desensitize_service.rs` | 数据脱敏（`RedactionVault` + `DesensitizationMode::Low/Off`） |

### 4.8 metrics/ — 指标采集（4 层测量契约）

- **Layer 1 `LatencyReport`**：6 阶段延迟（DocumentIngestion/DocumentStructure/Chunking/Embedding/AgentReview/PostProcessing）
- **Layer 2 `LlmEfficiencyReport`**：LLM 调用效率（totals + by_agent + ToolUsageSummary + wasted_call_ratio）
- **Layer 3 `ReviewQualityReport`**：审核质量（FindingSummary + dedup_rate + debate/blindspot/cross_agent_links）
- **Layer 4 `ResourceReport`**：资源（TokenCostBreakdown + MemoryUsage + EmbeddingStats）

`MetricsCollector`（`Arc<Mutex>` 线程安全），`finalize(meta) -> RunMetrics` 写盘到 `output/runs/{run_id}.json`。定价：qwen-plus（0.8/2.0 CNY per 1M tokens）。

### 4.9 动态 Agent 配置

`agents/dynamic_agents.json` 由 BlindSpotAgent 经验沉淀生成，格式含 id/display_name/system_prompt/max_turns/complexity/section_keywords/tool_names/active 等字段。当前仅含 1 个非活跃测试 Agent，下次审查自动启用。

---

## 5. backend-java — 业务网关

Java 后端是**薄业务网关**，处理认证/CRUD/文件管理/审核编排/SSE 中继，AI 负载透明转发 Rust。包根 `com.ithsd.smart_tender`。

### 5.1 构建配置（pom.xml）

- **Spring Boot 3.2.3** / Java 17 / artifactId `smart_tender`
- 核心依赖：spring-boot-starter-web、mybatis-plus-spring-boot3-starter 3.5.5、druid 1.2.20、mysql-connector-j、jjwt 0.9.1、fastjson2、pdfbox 2.0.32、poi 5.2.5、lombok、testcontainers
- Flyway 默认禁用，JPA 已移除（纯 MyBatis-Plus）

### 5.2 启动与配置

`SmartTenderApplication`（`@SpringBootApplication` + `@EnableAsync` + `@EnableScheduling`）。

| 配置项 | 值 |
|---|---|
| server.port | 8080（prod 8086） |
| MySQL | localhost:3306/smart_tender_system |
| Redis | localhost:6379 db=10 |
| rust.api.base-url | http://127.0.0.1:3001 |
| rust.api.read-timeout-ms | 900000（15min） |
| audit.queue.mode | async（默认）/ redis-list / redis-stream |
| 文件上传限制 | 50MB |

### 5.3 config 包（5 个配置类）

| 类 | 职责 |
|---|---|
| `WebMvcConfiguration` | CORS 全开放 + 注册 `JwtTokenAdminInterceptor` 拦截 `/api/**`（排除登录/注册/回调） |
| `AsyncConfig` | `auditTaskExecutor` 线程池：core=2/max=4/queue=100/CallerRunsPolicy |
| `RustApiProperties` | `@ConfigurationProperties(prefix="rust.api")`：baseUrl/超时/healthCheck/reviewTimeout/desensitizationMode |
| `MybatisPlusConfig` | 注册 `OptimisticLockerInnerInterceptor`（乐观锁）+ `PaginationInnerInterceptor`（分页） |
| `MyMetaObjectHandler` | insert/update 自动填充 createTime/updateTime/createUser/updateUser |

### 5.4 common 包（横切组件）

| 类 | 职责 |
|---|---|
| `BaseContext` | `ThreadLocal<Long>` 持有当前用户 ID |
| `BizException` | 业务异常（含 code） |
| `GlobalExceptionHandler` | `@RestControllerAdvice` 统一异常处理 |
| `JwtTokenAdminInterceptor` | JWT 拦截器：解析 Bearer → BaseContext.setCurrentId（密钥硬编码，待改进） |
| `JwtUtil` | createJWT/parseJWT（HS256） |
| `MD5Util` | MD5 摘要（待升 BCrypt） |
| `StringListJsonTypeHandler` | List↔JSON TypeHandler（用于 audit_task.enabled_checks/failed_stages） |

### 5.5 controller 包（10 个 REST 控制器）

| Controller | 基路径 | 核心端点 |
|---|---|---|
| `UserController` | `/api/auth` | login/logout/refresh/register（JWT 24h + Redis 黑名单） |
| `AuditTaskController` | `/api/audit-tasks` | createTask / getStatus / recover / getResult / **stream**(SSE) / count-audit / blocks |
| `TenderController` | `/api/bid-documents` | upload / page / stats / get / versions / projects / delete / download |
| `ChatController` | `/api/chat` | chat / history / **stream**(SSE) |
| `KnowledgeFileController` | `/api/knowledge-files` | upload / page / search / CRUD / download / preview |
| `ProjectController` | `/api/projects` | CRUD / my |
| `ReportController` | `/api/audit-reports` | generate / get |
| `AuditHistoryController` | `/api/audit-history` | page / detail / statistics / delete |
| `AuditIssueController` | `/api/audit-issues` | count-issue |
| `TraceController` | `/api` | audit-tasks/{taskId}/traces / traces/{sessionId} |

统一响应体 `Result<T> { code, msg, data, timestamp }`（code=200 成功）+ `PageResult { total, records }`。

### 5.6 service 包（核心业务逻辑）

#### 5.6.1 Rust 引擎代理层（`service/engine/rust/`）

| 类 | 职责 |
|---|---|
| `RustApiClient` | JDK 11 HttpClient 封装所有 Rust HTTP 调用（SNAKE_CASE）。uploadDocument/getDocument/startReview/getReviewResult/chatWithDocument/connectChatStream/searchDocument/getBlockBboxes/healthCheck |
| `RustSseClient` | 连接 Rust SSE 流，逐行解析 `event:`/`data:` 协议，callback 桥接到 SseHub。**先连 SSE 再 POST review** 保证不丢早期事件 |
| `RustDocumentService` | Java Tender ↔ Rust document_id 映射管理。`ensureUploaded(bidId)` 幂等上传 + 断线重传（404 重传） |

#### 5.6.2 任务队列（`service/engine/queue/`）— 策略模式

`AuditTaskDispatcher` 接口，3 种实现（`@ConditionalOnProperty` 切换）：

| 实现 | mode | 机制 |
|---|---|---|
| `AsyncAuditTaskDispatcher` | async（默认） | 直接调 `auditEngineService.start`（@Async） |
| `RedisListAuditTaskDispatcher` | redis-list | LPUSH 投递 + BLPOP 阻塞消费 |
| `RedisStreamAuditTaskDispatcher` | redis-stream | XADD + 消费者组 XREADGROUP + ACK + PEL 重试 + DLQ |

Worker：`RedisListAuditTaskWorker`（@Scheduled fixedDelay=100 轮询）、`RedisStreamAuditTaskWorker`（含消费者组初始化 + 重试 + DLQ）。

#### 5.6.3 审核编排核心 `AuditEngineServiceImpl`

4 阶段流水线（`@Async("auditTaskExecutor")`）：

1. **Stage 1 上传**：`rustDocumentService.ensureUploaded(bidId)`
2. **Stage 2 审核**：先 `rustSseClient.connect`（等 15s）→ `rustApiClient.startReview`（202；409 重试）→ SSE 回调处理 agent_progress/trace/phase/stats/finding_added/removed/updated/done/error → `awaitReviewResult` 轮询
3. **Stage 3 映射**：`completeTaskFromReview`（synchronized 幂等）→ 删旧 issues → 遍历 RustRiskFinding 映射写 `audit_issue` → markCompleted
4. **Stage 4 完成**：emit COMPLETE → finally `sseHub.close(taskId)`

关键设计：
- `RUNNING_TASKS` ConcurrentHashMap.newKeySet() 防并发重入
- `emitSafe`（持久化 audit_task_event + 推 SseHub）vs `emitTransient`（高频事件不落库）
- `recover(taskId)` 从 Rust 已完成结果恢复孤儿任务

#### 5.6.4 `AuditTaskServiceImpl`

- `createTask`（@Transactional）：INSERT audit_task(PENDING) → **afterCommit 调 dispatch**（保证落库后才调度）
- `getResult`：**Rust 内存优先**，失败回退 `audit_issue` 表重建 findings
- `subscribeStream`：`sseHub.subscribe` + Last-Event-ID 断线重连 replay
- `loadTask`：含**归属校验**（创建者或标书上传者才有权，否则 403）

#### 5.6.5 其他业务 Service

| Service | 职责 |
|---|---|
| `ChatService` | 同步对话 + 流式对话（转发 thinking/tool_call/answer/done），含 selection/bbox + 最近 6 条历史 |
| `DocumentPreviewService` | DOCX→PDF 预览（JODConverter REST），SHA-256 缓存 |
| `StoragePathService` | 多路径 fallback：`{root}/{dir}/{yyyy-MM-dd}/{uuid}{ext}` |
| `UserServiceImpl` | login（phone + MD5 + status）/ register |
| `TenderServiceImpl` | 标书 CRUD + 项目聚合 |
| `ReportServiceImpl` | generateReport（Markdown 存 audit_report）/ resolveAuditId（按 bid_id 找最新任务） |
| `TraceServiceImpl` | ingestTraceEvent（持久化 ReAct 步骤）/ markSessionsCompleted |
| `KnowledgeFileServiceImpl` | 知识库文件 CRUD |
| `KnowledgeChunkServiceImpl` | 知识库分块处理 |

### 5.7 sse 包（5 个 SSE 组件）

| 类 | 职责 |
|---|---|
| `SseHub` | SSE 连接池。`ConcurrentHashMap<taskId, ConcurrentHashMap<emitterId, SseEmitter>>`（每 taskId 多标签页），30min 超时，集成 Micrometer 指标 |
| `AuditTaskEventService` | 事件持久化 + 断线回放：persist（INSERT audit_task_event 返回 eventId）/ replay（SELECT id>lastEventId） |
| `RedisSseConnectionStateStore` | Redis 存 SSE 连接状态（TTL 10min） |
| `ReplaySseEvent` | 回放事件 POJO |
| `AuditSseProperties` | replayMaxEvents（默认 100） |

### 5.8 mapper 包（13 个 MyBatis-Plus Mapper）

全部 `extends BaseMapper<Entity>`，无 XML，纯 LambdaQueryWrapper。例外 `AuditTaskMapper`（自定义 SQL 注解）：
- `countByWeek` — 按天统计本周任务数
- `advanceReviewProgress` — 单调进度推进（`GREATEST(progress, ?)`，绕过乐观锁）
- `markFailed` / `markCompleted` — 状态更新（`task_status<>2` 守卫防覆盖已完成）

### 5.9 model 包

#### Entity（13 个，`@TableName`）

`User`/`Tender`/`Project`/`AuditTask`（@Version 乐观锁 + JSON TypeHandler）/`AuditIssue`/`AuditReport`/`AuditTaskEvent`/`ChatMessage`/`KnowledgeFile`/`KnowledgeChunk`/`TraceSession`/`TraceEventEntity`/`TraceEventBlock`

#### 关键枚举

- `AuditTaskStatusEnum`：PENDING(0)/PROCESSING(1)/COMPLETED(2)/FAILED(3)
- `AuditStageEnum`：UPLOADING/REVIEWING/SUMMARY
- `SseEventTypeEnum`：PROGRESS/ISSUE/COMPLETE/AGENT_PROGRESS/TRACE/PHASE/STATS/FINDING_ADDED/UPDATED/REMOVED

#### dto/rust（13 个 Rust API 专用 DTO）

全部 `@JsonIgnoreProperties(ignoreUnknown=true)` 向前兼容。核心 `RustRiskFinding`（含 riskId/clauseIds/blockIds/agent/severity/critical/riskType/sourceQuote/legalBasis/reason/suggestion/confidence/initialTier/finalTier/tierEscalated/truncated/suggestedAgent/citations/pageNumber/sectionPath）。

### 5.10 数据库表结构

主库 9 张表 + SSE 事件 3 张 + 追溯 3 张：

| 表 | 核心字段 |
|---|---|
| `sys_user` | id, username(唯一), password(MD5), phone, status |
| `bid_document` | id, file_name, file_path, file_type, bid_name, supplier_name, page_count, parse_status, project_id, **rust_document_id** |
| `audit_task` | task_id(唯一), bid_id, task_status(0-3), audit_result, issue_count, critical_count, stage, progress, **enabled_checks(JSON)**, **version(乐观锁)** |
| `audit_issue` | audit_id, issue_no, severity, **is_critical**, category, description, suggestion, page_number |
| `audit_report` | audit_id(唯一), doc_content(TEXT Markdown) |
| `audit_task_event` | task_id, event_type, event_data(LONGTEXT) — 索引 (task_id, id) 断线重连 |
| `trace_sessions` | id(UUID), task_id, agent_name, clause_id, initial_tier/final_tier, status, risk_id, total_turns |
| `trace_events` | event_id(UUID 唯一), session_id(FK CASCADE), event_type(turn_start/agent_thought/tool_call/tool_result/output_finding) |
| `trace_event_blocks` | event_id(FK), block_id（反查 block→events） |

---

## 6. frontend — React 前端

React 19 + TypeScript + Vite SPA，Feature-based 架构。

### 6.1 构建配置

- **package.json**：`bid-audit`，type=module。关键依赖：react 19.2、antd 5.27、@reduxjs/toolkit 2.11、@tanstack/react-query 5.90、axios 1.13、react-pdf 10.4、echarts 6.0、react-markdown 10.1、vitest 4.1
- **vite.config.ts**：`@` 别名 → `./src`；dev :5173；代理 `/api` → `127.0.0.1:8086`；`/api/chat/stream` 单独 `selfHandleResponse: true` 手动 pipe SSE
- **scripts**：dev/build(tsc -b && vite build)/lint/test(vitest run)

### 6.2 应用入口与路由

**`src/App.tsx`** Provider 嵌套：ThemeProvider → ReduxProvider → QueryClientProvider → ConfigProvider(zhCN, 动态主题) → AntdApp → RouterProvider

**`src/app/router.tsx`**（createBrowserRouter）：

| 路径 | 组件 | 说明 |
|---|---|---|
| `/login` | LoginPage | 登录/注册（已登录跳 dashboard） |
| `/dashboard` | DashboardPage | 工作台 |
| `/upload/:projectId` | BidUploadPage | 标书上传 |
| `/bidReview` | BidAuditList | 审核列表 |
| `/bidReview/detail/:id` | DetailPage | 审核详情（核心：PDF + 实时分析 + AI 对话） |
| `/bidReview/issues/:id` | IssueListPage | 问题清单 |
| `/bidReview/report/:id` | ReportPage | 审核报告 + DOCX 导出 |
| `/library` | BidLibraryPage | 知识库管理 |
| `/history` | HistoryPage | 历史记录（代码完整，未注册路由） |

**`RouteGuard.tsx`**：从 Redux `state.auth.isAuthenticated` 读登录态，未登录跳 /login。

### 6.3 API 请求层

**`src/api/request.ts`** — Axios 实例：
- 请求拦截器：注入 `Authorization: Bearer {token}`
- 响应拦截器：2xx 返 `response.data`；401 单飞锁防重复跳转 + logout；blob 同样处理（文件下载）

**`src/api/types.ts`**：`BaseResponse<T>{code,msg,data,timestamp}`、`PageResponse<T>{records,total}`

各 feature 统一导出：裸 API 函数 + `xxxOptions`（queryOptions）+ `useXxxMutation`（onSuccess invalidate 关联缓存）。

### 6.4 状态管理（三层分离）

| 层 | 技术 | 用途 |
|---|---|---|
| Redux Toolkit | 仅 `auth` reducer | token/userInfo/isAuthenticated，localStorage/sessionStorage 持久化 |
| TanStack React Query | 服务端数据 | retry=1，staleTime=5min，refetchOnWindowFocus=false，`placeholderData: (prev)=>prev` 防闪烁 |
| URL 状态 | `useUrlState` | 基于 useSearchParams，筛选条件双向同步，number 自动转换 |

### 6.5 公共组件

| 组件 | 职责 |
|---|---|
| `theme-provider.tsx` | dark/light/system 主题，localStorage 持久化 |
| `layout/MainLayout` | 三段式布局，768px 切移动端（侧栏→底部导航） |
| `layout/Sidebar` | 导航项 + Logo |
| `layout/Header` | 折叠按钮 + 面包屑（动态路由拉标书名）+ 主题切换 + 用户退出 |
| `Loading` | Spin 封装 |
| `StatCard` | AuditResultCard / DashboardStatCard |
| `VersionDrawer` | 项目历史版本抽屉（Timeline + 进入审核） |

### 6.6 功能模块

#### 6.6.1 login — 登录/注册
Tab 切换登录/注册，`useLoginMutation`/`useRegisterMutation`，登录成功 dispatch setCredentials 后跳转。

#### 6.6.2 dashboard — 工作台
左列：统计卡片 + 新建项目 Modal + 项目表格；右列：问题分布饼图 + 按星期审计柱状图（ECharts）。

#### 6.6.3 bidUpload — 标书上传
`BidForm` + `UploadInstructions`，`useUploadBidMutation`（FormData POST），成功后跳 `/bidReview`。

#### 6.6.4 bidAudit — 审核功能（最复杂）

**列表 `BidAuditList`**：`useUrlState` 管理筛选，`useQuery` 拉分页，`useDeleteProject` 删除，行点击打开 `VersionDrawer`。`mapFinding.ts`：后端 snake_case → 前端 camelCase 映射。

**详情页 `DetailPage`**（核心）：左 PDF 预览（55%）+ 右分析（44%），移动端堆叠。

`useAuditTask.ts` — 审核生命周期（SSE + 轮询兜底）：
- `startAudit`（POST /api/audit-tasks）→ 写 localStorage（`auditTask:{bidId}` + `auditLastEvent:{taskId}`）
- Hydrate effect：挂载时根据 storage taskId 判断状态（completed→拉 result；failed→清 storage；processing→触发 SSE）
- SSE effect：`connectStream` 分发事件（issue→追加、progress→进度、agent_progress→Map、trace→liveFeed（保留 100 条）、phase→阶段历史、stats→统计、finding_*→维护 liveFindings）
- 轮询 effect：`setInterval(syncStatus, 3000)`，连续失败 5 次停止
- 计时 effect：审核活跃时按秒更新 elapsedSeconds

`useAiChat.ts` — AI 对话（SSE 流式 + 本地持久化）：
- localStorage key `aiChat:v3:{projectId}:{bidId}`（主存储，后端历史为备份）
- `sendMessage`：添加 user 消息 + AI placeholder（streaming）→ `connectChatStream` → 回调填充 reasoning/content/citations → done 标记 sent

`PdfPreview.tsx` — PDF 渲染 + 高亮（最复杂组件之一）：
- react-pdf Document/Page，forwardRef 暴露 `jumpToPage` / `highlightBboxes`
- **三种高亮策略**（`VITE_HIGHLIGHT_MODE`: auto/bbox/text）：
  1. BBox 优先：调 `/blocks` 拿坐标，按 renderedWidth/pageWidth 缩放渲染 overlay
  2. pdfjs 文本索引匹配：`getTextContent()` 构建字符级 bbox 索引，精确/token 降级匹配
  3. Span 文本层匹配：操作 `.react-pdf__Page__textContent` span
- 跨页探测：当前页未命中按 ±3 页→全量搜索

`BidAnalysis.tsx` — 右侧三标签页：process（审核过程）/ results（审核结果）/ chat（智能问答）
- `ClauseActivityMap`：章节树 + 实时状态灯 + Agent 迷你进度 + TraceDetailLog
- `LiveReviewFeed`：ReAct 步骤实时滚动（保留 120 条）
- `AnalysisList`：风险发现列表 + 高亮联动（BBox 优先，降级文本匹配）

**问题清单 `IssueListPage`**：从 localStorage 读 taskId，一次拉 200 条前端筛选分页。

**报告 `ReportPage`**：
- `getReport` 拉 Markdown，无内容则 `generateReport`
- `parseMarkdownToSections` 按章节勾选拼接
- A4 纸预览（react-markdown）+ Ctrl+滚轮缩放
- **DOCX 导出**：取 `.a4Paper` innerHTML → `generateWordDocument`（html-docx-js）→ file-saver 下载

#### 6.6.5 bidLibrary — 知识库管理
`useUrlState` 筛选 + `useKnowledgeData`（列表，placeholderData 防闪烁）+ `useKnowledgeStatistics`（临时方案：拉 size=10000 全量本地计数）。预览/下载用 Blob + URL.createObjectURL。

#### 6.6.6 history — 历史记录（未接入路由）
代码完整：`useUrlState` + `useQuery` + FilterBar + HistoryTable（Tab 状态切换 + 通过率统计）。

### 6.7 共享类型 `src/types/audit.ts`

以 Rust 后端数据结构为标准，跨特性类型唯一来源：
- `Severity`/`RiskTier` + 颜色映射
- `BidDocument`（22 字段，含 parseStatus 0-3）
- `AuditIssue`（对齐 Rust RiskFinding，含 isCritical/initialTier/finalTier/clauseIds/blockIds/citations/anchorQuote 等）
- `GraphSnapshot`（会话知识图谱）
- SSE 事件：AgentProgress/TraceEvent/PhaseEvent/FindingAdded/Updated/RemovedEvent/StatsEvent
- 常量：`AGENT_LABEL_MAP`（Agent ID→中文）、`PHASE_LABELS`（7 阶段）

### 6.8 主题与样式

- **CSS-in-JS**：antd-style `createStyles`，自动读取 Ant Design Token 跟随主题
- 学校绿主色（#2E7D32），深浅模式动态切换 algorithm/token
- 响应式：`useIsMobile()`（768px 断点）

---

## 7. benchmark — 审核质量评估

Python 评估框架，验证标书审核系统的"问题发现能力"。

### 7.1 数据集

- **Silver Benchmark v1**：50 份公开招标 PDF + 150 条人工设计风险注入项（每份末页追加 3 条：Critical+High+Medium 各 1）
- **blind-v2**：10 份新来源 PDF（与 v1 零重叠）+ 30 条全新措辞，冻结 SHA-256 防篡改
- **15 类风险分类体系**：5 类 Critical（地域注册/品牌锁定/无关资格/区域业绩/经营规模门槛）+ 5 类 High + 5 类 Medium

### 7.2 评估流程

```
build_dataset.py / build_blind_v2.py    # 构建 PDF + 真值标注 + 冻结清单
        ↓
run_benchmark.py                          # 启动 Rust 引擎 → 上传 PDF → 定位注入页
        ↓                                  # → POST /review → 轮询 /result → 归一化 finding
risk_policy.py (可选)                     # 后处理：证据过滤 + 类别归一化 + Critical 重算 + 去重
        ↓
evaluate.py                              # 一对一匹配 → 计算 P/R/F1 + Critical 指标 → 判定门禁
        ↓
summary.json / summary.md                # PASS/FAIL 报告
```

### 7.3 关键文件

#### `evaluate.py` — 评估逻辑

匹配算法：按 document_id 分组 → 判断 in_scope（页码在注入页或引文相似度≥0.45）→ 一对一匹配（type_matches + quote_similarity，severity_bonus 0.05）→ 计算 TP/FP/FN。

**release_gate 通过条件**：`F1>=0.80 且 Precision>=0.75 且 Critical标记召回率>=0.95 且 Critical Precision>=0.80`

核心函数：`load_jsonl`/`normalize`/`quote_similarity`/`type_matches`/`metric`/`main(--gold/--pred/--output)`

#### `risk_policy.py` — 风险策略后处理

**只使用结构化字段和原文引文，不检查 gold 标签，可安全用于离线回放**。

| 函数 | 职责 |
|---|---|
| `category_from_evidence(text)` | 基于原文证据的确定性分类器（组合关键词规则），可纠正模型错误分类码 |
| `canonical_category(row)` | 类别归一化入口：证据优先 → clean_code 查 CATEGORY_NAMES/ALIASES |
| `is_critical(code, quote)` | Critical 红线判定：仅对 5 类 Critical 基于证据词组判定（不依赖模型自报） |
| `postprocess(rows)` | 主处理：证据过滤（空/标题样/Info+未发现词拒绝）→ 归一化 → Critical 重算 → 跨 Agent 去重（同 doc+同 code+引文相似度≥0.75，保留高 confidence） |

#### `run_benchmark.py` — 运行入口

无人值守：冻结校验 → 健康检查 → 加载 gold → 逐文档（上传 PDF multipart+desensitize_mode → select_injection_chunks 定位注入页 → POST /review → 轮询 /result → 归一化 → 断点续跑）→ 汇总 predictions → 调 evaluate.py → 生成 summary。

#### `build_dataset.py` / `build_blind_v2.py`

下载公开 PDF → reportlab 生成注入页（A4，SimHei 字体）追加末页 → 写 annotations/taxonomy/source_manifest。blind-v2 额外生成 `freeze_manifest.json`（SHA-256 冻结）。

### 7.4 当前评估结论（2026-07-27）

| 指标 | 门槛 | Silver v1 test | blind-v2 |
|---|---:|---:|---:|
| F1 | 80% | 98.36% | **62.69%** |
| Precision | 75% | 96.77% | 56.76% |
| Critical 标记召回率 | 95% | 100% | **30%** |

**结论：未通过上线门槛**，仅可作为人工辅助工具受控试点。Silver v1 高分只能证明"没有回归"，blind-v2 才是真正泛化能力依据。

下一轮改造方向：从"关键词路由多个 Agent + 文本去重"改为"**原子证据切分 + 高召回候选生成 + 责任制验证 + 证据中心裁决 + 确定性政策计算**"。

---

## 8. 依赖关系与数据流

### 8.1 模块依赖关系

```
frontend (React :5173)
   │ REST/SSE (Vite proxy → :8086)
   ▼
backend-java (Spring :3000)
   │ ├─ MySQL (smart_tender_system) ── 状态持久化
   │ ├─ Redis (db10) ── token/SSE状态/任务队列
   │ ├─ JODConverter (:8088) ── DOCX→PDF 预览
   │ └─ HTTP/SSE 透明代理
   ▼
backend-rust (Axum :3001)
   ├─ BGE-M3 ONNX ── 本地嵌入
   ├─ DashScope/OpenAI ── LLM 推理
   ├─ SearXNG/DashScope ── 联网搜索
   └─ Milvus (:19530) ── 向量检索（Rust 侧）
```

### 8.2 核心数据流（审核任务端到端）

```
1. 前端点击「开始审核」
   → POST /api/audit-tasks { bidId, forceRefresh }
   → Java AuditTaskServiceImpl.createTask (@Transactional)
       INSERT audit_task (PENDING, stage=UPLOADING)
       afterCommit → AuditTaskDispatcher.dispatch(taskId)

2. 调度（策略模式）
   → AsyncAuditTaskDispatcher / Redis-List / Redis-Stream
   → AuditEngineServiceImpl.start (@Async auditTaskExecutor)

3. Stage 1 上传
   → RustDocumentService.ensureUploaded(bidId)  [幂等+断线重传]
   → RustApiClient.uploadDocument → Rust POST /api/v1/documents
       Rust: DOCX→PDF → pdfplumber 提取 → sectionize → chunking → 脱敏 → embedding
   → 回写 Tender.rustDocumentId

4. Stage 2 审核
   → RustSseClient.connect(docId, callback)  [先连 SSE，等 15s]
   → RustApiClient.startReview (POST /api/v1/documents/:id/review, 202)
       Rust 后台 run_review_pipeline:
         Coordinator.review(clauses):
           [1] ROUTE: 关键词路由 → 各 Agent
           [2] PRELOAD: SessionGraph 预加载
           [2.5] BATCH_SEARCH: 预搜索法规
           [3] EXECUTE: 并行 Agent (ReActLoop per clause)
                  查 SessionGraph + poll AgentBus + LLM chat + 工具执行 + output_finding
           [4] MERGE: 去重合并 + 跨 Agent 关联
           [5] LEGAL_VERIFY: 法条对抗验证
           [6] DEBATE: 高风险辩论
           [7] TRIAGE: 排序 → CoordinatorOutput
         恢复脱敏原文 → 填充 block_ids → 写盘 → emit Done

   SSE 事件回调:
     ├─ SseHub.emit → 前端 (多标签页广播)
     ├─ AuditTaskEventService.persist → DB (断线重连回放)
     └─ TraceService.ingestTraceEvent → DB (审查追溯)

   前端 useAuditTask SSE effect:
     issue → 追加 AnalysisList
     agent_progress → AgentProgressCards
     trace → LiveReviewFeed (保留 100 条)
     phase → ClauseActivityMap 阶段切换
     finding_added/updated/removed → ClauseActivityMap 树更新
     complete → 停止 SSE，切「审核结果」Tab

5. Stage 3 映射
   → awaitReviewResult 轮询 getReviewResult
   → RustRiskFinding → AuditIssue → 批量入库 audit_issue
   → markCompleted (version+1, task_status<>2 守卫)

6. Stage 4 完成
   → emit COMPLETE → sseHub.close(taskId)

7. 降级（getResult 时）
   → Rust 内存优先，404 则从 audit_issue 表重建 findings

8. 报告
   → ReportServiceImpl.generateReport → 生成 Markdown 存 audit_report
   → 前端 ReportPage 拉取 → A4 预览 → DOCX 导出
```

### 8.3 Rust 内部双通道通信

| 通道 | 方向 | 用途 |
|---|---|---|
| AgentBus（broadcast） | Agent → Agent | 仅 High 风险实时广播，按 risk_type 选 topic |
| SessionGraph（Blackboard） | Agent 拉取 | 结构化查询已知结论/关联拓扑/矛盾 |
| ReviewEventBus | Coordinator → SSE | 全部事件推前端 |

### 8.4 STS 架构（Scout-Hypothesis-Verify）

Scout（Phase 0）输出低置信度 Hypothesis → SessionGraph 注入 Phase 2 Agent 上下文 → Phase 2 Agent 验证 → `finding_role`/`verified_by` 标记。

---

## 9. 项目运行方式

### 9.1 环境要求

| 工具 | 版本 |
|---|---|
| JDK | 17+ |
| Maven | 3.8+ |
| Rust | 1.80+（2024 edition） |
| Node.js | 18+ |
| pnpm | 8+ |
| Docker | 24+（含 Compose v2） |

### 9.2 启动顺序

```
1. Docker 基础设施  →  MySQL + Redis + Milvus + MinIO + etcd + doc-converter
2. .env 配置        →  填写 API 密钥和环境变量
3. Rust 引擎 :3001  →  AI 审核 / 嵌入 / LLM 调用
4. Java 网关 :3000  →  认证 / CRUD / SSE 推送
5. React 前端 :5173 →  Web 界面
```

### 9.3 配置环境变量（.env）

```bash
# 必填：LLM API 密钥
DASHSCOPE_API_KEY=sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# LLM 协议（dashscope / openai_compatible）
AIBID_LLM_PROTOCOL=dashscope

# 搜索后端（dashscope / searxng）
AIBID_SEARCH_BACKEND=dashscope

# 嵌入引擎（local / remote）
EMBED_ENGINE=local

# Multi-Agent 审核
AIBID_AGENT=1
AIBID_COORDINATOR=1

# 数据根目录（从 backend-rust/ 运行时设为 ..）
AIBID_DATA_DIR=..
```

### 9.4 启动命令

```bash
# 终端 1：基础设施
cd backend-java/src/main/resources && docker compose up -d

# 终端 2：Rust AI 引擎
cd backend-rust
# Windows PowerShell
$env:AIBID_DATA_DIR=".."
cargo run --bin server

# 终端 3：Java 业务网关
cd backend-java
mvn spring-boot:run

# 终端 4：前端
cd frontend
pnpm install && pnpm dev
```

浏览器打开 `http://localhost:5173`，默认账号 `admin/123456`。

### 9.5 其他运行方式

**Rust CLI 模式**（不启动 API，直接审核单文件）：

```bash
cd backend-rust
$env:AIBID_DATA_DIR=".."
cargo run -- <投标文件.pdf>
cargo run -- --chat <投标文件.pdf>    # 交互式对话模式
```

**验证 LLM 连接**：`cargo run --bin test_llm`

**运行 Agent 集成测试**：`cargo test --bin test_agents -- --test all`

**运行 benchmark**：

```bash
cd benchmark
# 启动 Rust 引擎后
python run_benchmark.py --base-url http://127.0.0.1:3001 --scope injected
```

### 9.6 基础设施容器

| 容器 | 端口 | 用途 |
|---|---|---|
| smart-mysql | 3306 | MySQL 8.0（smart_tender_system） |
| smart-redis | 6379 | Redis 7.2 |
| milvus-standalone | 19530 | Milvus 向量库 |
| milvus-minio | 9000/9001 | Milvus 对象存储 |
| milvus-etcd | 2379 | Milvus 配置中心 |
| doc-converter | 8088 | DOCX→PDF 转换 |

> Milvus Attu 占用 3000 端口，与 Java 后端冲突。不需要 Web 管理界面时可在 docker-compose.yml 注释 attu。

---

## 10. 关键设计要点与技术债

### 10.1 关键设计要点

1. **薄网关架构**：Java 专注 Web 层事务/权限/CRUD/文件/SSE 中继，AI 全委托 Rust，职责清晰
2. **Rust 引擎黑盒**：Java 只关心"调用什么/返回什么"，`@JsonIgnoreProperties(ignoreUnknown=true)` + SNAKE_CASE 向前兼容
3. **事务一致性**：`TransactionSynchronizationManager.afterCommit` 保证任务落库后才 dispatch，避免孤儿任务
4. **策略模式队列**：3 种 Dispatcher 适配开发/单机/多实例，`@ConditionalOnProperty` 切换
5. **SSE 多路复用 + 断线重连**：SseHub 每 taskId 多 emitter（多标签页）；Last-Event-ID + audit_task_event 表回放
6. **幂等与降级**：RustDocumentService 幂等上传 + Rust 重启自动重传；getResult Rust 内存优先 + DB 兜底，Java 为 Source of Truth
7. **乐观锁 + 单调推进**：audit_task.version 防并发覆盖；高频进度用 advanceReviewProgress 绕过乐观锁
8. **脱敏双视图**：原文（本地定位）+ 脱敏副本（远程模型接触），返回前恢复原文
9. **Multi-Agent + STS 架构**：Scout 假设 → 验证，AgentBus 实时广播 + SessionGraph 结构化 Blackboard
10. **三层状态管理**：Redux(Auth) + React Query(服务端) + URL(筛选)，职责分离

### 10.2 已知技术债

| 模块 | 技术债 | 改进方向 |
|---|---|---|
| Java | MD5 密码 | 升级 BCrypt |
| Java | JWT 密钥硬编码 | 抽配置项 |
| Java | 无 refresh token | 接入 /api/auth/refresh 拦截器 |
| Java | 测试覆盖不足 | 补充单元/集成测试 |
| Java | 无全链路 traceId | MDC 全链路追踪 |
| 前端 | 无代码分割 | React.lazy + Suspense |
| 前端 | 无 ErrorBoundary | 全局错误边界 |
| 前端 | 无 E2E 测试 | 补充 Playwright |
| 前端 | PDF 全量渲染 | 虚拟滚动（大文件卡顿） |
| 前端 | history 页未注册路由 | 接入路由 |
| Rust | Critical 依赖窄关键词 | 建立结构化事实层 + 确定性政策计算 |
| Rust | 同一证据跨 Agent 重复/错误分类 | 证据中心仲裁 + 类别兼容矩阵 |
| Rust | 审核单位过粗（千字 chunk） | 原子证据切分 |

### 10.3 下一轮架构演进（来自 benchmark 评估）

从"关键词路由多个 Agent + 文本去重"改为"**原子证据切分 → 高召回候选生成 → 候选责任制验证 → 证据中心裁决 → 确定性政策计算**"：

- 路由由候选类别决定，不能只由章节关键词决定
- `is_critical` 是计算字段，模型不得直接写最终值
- 每个候选有状态（待审/成立/驳回/证据不足），不能无声消失
- 同一证据先仲裁类别，再进入最终去重
- 法规验证只能影响引用质量，不能因搜索失败删除已成立的事实风险

分阶段实施：Phase 0 正确性止血 → Phase 1 候选先于路由 → Phase 2 原子证据与仲裁 → Phase 3 语义候选 → Phase 4 新盲测验证。

---

> **参考文档**：[README.md](file:///d:/AI标书/ai-bid/ai-bid/README.md)、[CLAUDE.md](file:///d:/AI标书/ai-bid/ai-bid/CLAUDE.md)、[backend-rust/CLAUDE.md](file:///d:/AI标书/ai-bid/ai-bid/backend-rust/CLAUDE.md)、[backend-java/CLAUDE.md](file:///d:/AI标书/ai-bid/ai-bid/backend-java/CLAUDE.md)、[frontend/CLAUDE.md](file:///d:/AI标书/ai-bid/ai-bid/frontend/CLAUDE.md)、[backend-rust/docs/设计.md](file:///d:/AI标书/ai-bid/ai-bid/backend-rust/docs/设计.md)、[backend-rust/docs/API参考.md](file:///d:/AI标书/ai-bid/ai-bid/backend-rust/docs/API参考.md)、[backend-java/docs/Rust引擎接口契约.md](file:///d:/AI标书/ai-bid/ai-bid/backend-java/docs/Rust引擎接口契约.md)、[backend-java/docs/设计.md](file:///d:/AI标书/ai-bid/ai-bid/backend-java/docs/设计.md)、[frontend/docs/设计.md](file:///d:/AI标书/ai-bid/ai-bid/frontend/docs/设计.md)、[benchmark/README.md](file:///d:/AI标书/ai-bid/ai-bid/benchmark/README.md)、[benchmark/FINAL_LAUNCH_ASSESSMENT_20260727.md](file:///d:/AI标书/ai-bid/ai-bid/benchmark/FINAL_LAUNCH_ASSESSMENT_20260727.md)、[benchmark/NEXT_ROUND_GENERALIZATION_ARCHITECTURE.md](file:///d:/AI标书/ai-bid/ai-bid/benchmark/NEXT_ROUND_GENERALIZATION_ARCHITECTURE.md)
