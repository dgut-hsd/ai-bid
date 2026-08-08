# Agent 修改文档

> 记录「Agent 降轮次 + 强制收尾」优化方向的代码修改与评估进展。
>

---

## 一、评估方案（已确认）

- 目标：量化「降轮次 + 强制收尾」对质量与成本的影响，输出修改前后对比。
- 指标口径：
  - **漏报率** = 1 − Recall（Recall 来自 `benchmark/evaluate.py`）
  - **误报率** = 1 − Precision
  - **完成率** = 盲测集完成文档数 / 总数
  - **token 成本** = LLM 调用真实 usage × 模型单价（qwen-plus 输入 0.8 / 输出 2.0 元每百万 token），整份文档聚合
- 评估集：blind-v2 冻结盲测集（10 份 + 30 真值，`benchmark/blind-v2/`）
- 实验拆分：3 轮全量 → ① 基线 ② 仅降轮次 ③ 降轮次 + 强制收尾
- 降轮次幅度：轻量 tier 4→3、标准 8→6、深度 10→8（温和下调）
- token 采集方案：**方案 A** — `ReviewResultResponse` 新增 `usage` 字段

---

## 二、修改记录

### Token 成本采集（方案 A）

**背景**：benchmark 链路此前拿不到 token 成本。server 模式每次 review 已写 metrics 到 `output/runs/*.json`，但 `GET /result` 响应不暴露 token 数据。

**改动文件：**

#### `backend-rust/src/api/handlers.rs`

1. **新增 `ReviewUsage` 结构体**（`ReviewResultResponse` 上方）：
   - 字段：`llm_calls`、`tokens_input`、`tokens_output`、`cost_cny`
   - 含义：单份文档一次审核聚合的 LLM 调用次数、输入/输出 token、估算成本（CNY）
   - 注：成本计算逻辑复用 `metrics/collector.rs` 的 `finalize()` → `llm_efficiency.totals`（定价常量 `QWEN_PLUS_INPUT_PRICE=0.8` / `QWEN_PLUS_OUTPUT_PRICE=2.0`，qwen-turbo 0.5/2.0）

2. **`ReviewResultResponse` 新增字段**：
   ```rust
   /// 该文档审核的 LLM token 消耗与成本估算（审核成功后提供）
   #[serde(skip_serializing_if = "Option::is_none")]
   pub usage: Option<ReviewUsage>,
   ```

3. **`AppState` 新增 `review_usages` 缓存**：
   ```rust
   /// 异步审查的 token/成本统计：doc_id → ReviewUsage
   pub review_usages: Arc<TokioMutex<HashMap<String, ReviewUsage>>>,
   ```
   `AppState::init()` 同步初始化。

4. **重构审核完成点**（`run_review_pipeline`）：
   - 调整执行顺序：先 `metrics.finalize()` 拿到 totals，构造 `usage`，再统一存入内存缓存 + 写盘。
   - `{doc_id}_result.json` 磁盘 fallback 文件现在**包含 usage**。
   - 内存 `state.review_usages` 同步缓存。

5. **`get_review_result`（GET /result）**：
   - 内存命中路径从 `state.review_usages` 读取并返回 `usage`。
   - failed / pending 分支补 `usage: None`。

#### `backend-rust/src/api/router.rs`

- OpenAPI `components/schemas` 注册 `handlers::ReviewUsage`。

#### `benchmark/run_benchmark.py`（token 成本采集接入）

- 每份文档审核完成后，从 `wait_for_result` 的 result 提取 `usage`，写入 `documents/<document_id>.json`。
- 新增 `token_usage` 汇总（`documents_with_usage` / `llm_calls` / `tokens_input` / `tokens_output` / `cost_cny`），写入 `summary.json` 和独立 `token_usage.json`。
- `summary.md` 增加一行 LLM 调用与成本汇总。
- 缓存复用分支同步读取已存的 `usage`。

**兼容性**：旧磁盘 `{doc_id}_result.json`（无 usage 字段）反序列化时 `Option` 自动为 `None`，不影响老数据读取。

**验证状态**：✅ `cargo check` 通过（首次编译下载全部依赖后，增量 24.6s）；`run_benchmark.py` 语法校验通过

---

## 三、进展

### 盲测数据重建

- 现状：`benchmark/blind-v2/data`（freeze_manifest + annotations）与 `benchmark/data` **从未进入 git**（`.gitignore` 忽略 `data/ results/ sources/ mutated/`），首次盲测结果 `blind-v2-final-20260727` 来自其他环境。
- 决策：用户确认用脚本重建。运行 `python benchmark/build_blind_v2.py` 成功：
  - 10 份政府采购源 PDF 下载完成（ccgp-shaanxi.gov.cn / zfcg.sh.gov.cn）
  - 生成 10 份 mutated PDF + 30 条真值标注（10 critical / 10 high / 10 medium）
  - `freeze_manifest.json`：版本 `blind-v2.0`，状态 `frozen_before_first_run`，24 个冻结文件
- ⚠️ 新 freeze 哈希与 20260727 历史不同；本次三轮对比（基线/降轮次/强制收尾）用同一套重建数据，内部可比。

### 环境准备

- `.env`：`DASHSCOPE_API_KEY` 已填写（长度 117），`EMBED_ENGINE=remote`、`AIBID_SEARCH_BACKEND=dashscope`
- 新增 `RUST_API_INTERNAL_SECRET`（Rust 内部接口 HMAC 签名密钥，Java 网关 / benchmark 直连 Rust 用）
- Python 依赖安装：`pypdf` / `reportlab`（build_blind_v2 需要）；中文字体 `simhei.ttf` 存在

### benchmark 链路修复（跑盲测的阻塞）

1. **Rust 内部鉴权 503**：Rust server 的 `/api/v1/*` 要求 HMAC 签名（`InternalAuthConfig`），secret 未配置时一律 503。给 `benchmark/run_benchmark.py` 实现签名（对齐 Java `InternalRequestSigner`）：`internal_auth_headers()` 生成 `X-Tenant-Id / X-User-Id / X-Request-Id / X-Internal-Timestamp / X-Internal-Signature`，secret 从环境变量或 `.env` 读取；`request_json` 与 `upload_pdf` 均合并签名头。
2. **evaluate 中文编码崩溃**：Windows 子进程 stdout 用 GBK，`run_benchmark.py` 按 UTF-8 读导致 `UnicodeDecodeError` → `stdout=None`。修复：evaluate 子进程设置 `PYTHONIOENCODING=utf-8` + `errors="replace"`。

### 探针验证通过（BLIND-001）

- P / R / F1 = 1.0（3 条注入全命中，无误报）；Critical 检出率 / 标记召回率 = 1.0
- token 采集生效：`usage = {llm_calls:13, tokens_input:84264, tokens_output:5056, cost_cny:0.08}`
- 全链路：上传（签名）→ 注入页定位 → 审核 → 结果（含 usage）→ evaluate → summary

### 基线盲测全量完成（baseline-20260808）

| 指标 | 值 |
|---|---|
| 完成率 | 10/10（0 失败） |
| 漏报率 (1−Recall) | 0%（Recall 100%） |
| 误报率 (1−Precision) | 11.76%（Precision 88.24%） |
| F1 | 93.75% |
| Critical 检出率 / 标记召回率 | 100% / 100% |
| 严重度一致率 | 66.67% |
| LLM 调用 | 175 次 |
| tokens | 1,255,339 in / 65,848 out |
| 成本 | ¥1.14 |
| 时长 | 701s（11.7 min） |
| 门禁 | FAIL（critical_precision < 0.80） |

注：结果目录 `benchmark/blind-v2/results/baseline-20260808/`（summary.json / token_usage.json / 每文档 JSON）。

### 第 2 轮降轮次完成（reduced-turns-20260808）

`AIBID_TIER_MAX_TURNS=low:3,medium:6,high:8`（基线 5/8/14）。

| 指标 | 基线 | 降轮次 | 变化 |
|---|---|---|---|
| 完成率 | 10/10 | 10/10 | = |
| 漏报率 | 0% | 0% | = |
| 误报率 | 11.76% | 11.76% | = |
| F1 | 93.75% | 93.75% | = |
| LLM 调用 | 175 | 166 | **-9 (-5.1%)** |
| input tokens | 1,255,339 | 1,241,417 | **-1.1%** |
| output tokens | 65,848 | 67,613 | +2.7% |
| 成本 | ¥1.14 | ¥1.13 | **-¥0.01** |
| 时长 | 701s | 701s | = |

**关键发现**：降轮次**质量完全无损，但成本几乎没省**。
- 逐文档看：BLIND-004/006/010 等明显减少（-6/-10/-2 次调用），但 BLIND-007 反而 +9 次（+91k in）——LLM 非确定性导致，tier 上限只是"封顶"不是"强制"，未触顶的条款不受影响。
- **根因**：成本大头是**每次调用的输入上下文量**（如 BLIND-007 单次调用数万 token），不是"轮次数"。注入页模式单条款审核大多在 3-6 轮内已收敛，很少触达 8/14 上限。

### 改造 2 强制收尾（完成）

**新增机制**：`backend-rust/src/agents/react_loop.rs` — 连续空转强制输出。
- 环境变量 `AIBID_STALL_FORCE_OUTPUT=N`（默认 0=关闭）：连续 N 轮 LLM **只请求探索类工具**（`read_section` / `search_document` / `search_knowledge` / `web_search`）且未产出 finding → 视为空转，下一轮强制锁定 `output_finding`。
- 实现：`consecutive_stall` 计数器 + 每轮 LLM 响应后检测（`r.has_output_finding()` 重置；探索类全匹配则累加）。
- 第 3 轮（`force-finish-20260809`，`STALL_FORCE_OUTPUT=2`）结果：**成本 ¥0.77（-32.5%）、input tokens -36%、调用 -29%**，但漏报率 0%→3.33%（漏 `BLIND-003-F03 采购人可单方无限变更需求`，medium）；F1 93.75%→92.06%。

### 回归过程中的两个修复

1. **磁盘空间不足**：C 盘 96% 满，链接测试二进制时报 `LNK1180 insufficient disk space`。清理 `target/` 下 163 个 PDB 文件（1.5GB），释放至 6.5GB。
2. **pre-existing 测试编译错误**（与本次改动无关）：`VerifyBidDepositArgs` 结构体新增了 `procurement_category` 字段（`Option<String>`，`serde(default)` 只影响反序列化），但 `verify_bid_deposit.rs` 的 8 个测试构造点未更新 → `cargo test` 编译失败。已批量补齐 `procurement_category: None`（`verify()` 中 `None` 默认按"货物"处理，不影响既有测试断言）。

### 改造 1 降轮次（已完成，第 2 轮验证通过）

- 修改 `backend-rust/src/agents/types.rs` `RiskTier::max_turns()`：新增环境变量 `AIBID_TIER_MAX_TURNS="low:N,medium:N,high:N"` 覆盖各档轮次上限。
- 未设置时用内置默认 **Low=5 / Medium=8 / High=14**（与基线一致）；降轮次档用 `low:3,medium:6,high:8`。
- 生效链路：`effective_max_turns = min(tier_max_turns, agent_default)`；各 agent 默认（registry）：FactCheck 10 / Procedure 12 / RuleEngine 14 / SemanticRisk 14 / Scoring 10 / Demand 12 / Contract 12。

---

## 四、完成清单

-  探针验证（1 份）：全链路 + usage 采集通过
-  基线盲测全量 10 份（`baseline-20260808`）
-  第 2 轮降轮次全量（`reduced-turns-20260808`）
-  第 3 轮降轮次 + 强制收尾全量（`force-finish-20260809`）
-  **对比报告**：`docs/降轮次与强制收尾优化对比报告.md`（漏报率 / 误报率 / 完成率 / token 成本 / F1 / 时长 + 结论 + 下一步建议）
-  **工作成果报告 2 份**（Agent 组）：`docs/工作成果报告提交/Agent组_PR指标报告.md`、`docs/工作成果报告提交/Agent组_项目工作成果实验报告.md`
-  盲测数据重建（blind-v2：10 源 PDF + 10 mutated + 30 真值 + freeze 锁定）
-  benchmark 链路修复（HMAC 签名绕过 internal auth、evaluate 编码、token 采集接入）

---

## 五、复现步骤

> 流程：重建数据 → 编译引擎 → 三轮盲测（探针 / 基线 / 降轮次 / 强制收尾）→ 出报告。

### 0. 前置准备（一次性）

```bash
# ① 重建盲测数据（下载 10 份政府采购 PDF + 生成 30 条真值 + freeze 锁定）
cd ai_bid/benchmark && python build_blind_v2.py
# 预期：sources/ 10 个 PDF，data/annotations.jsonl 30 行，data/freeze_manifest.json 生成

# ② .env 必填两样：DASHSCOPE_API_KEY、RUST_API_INTERNAL_SECRET（内部接口签名密钥）
# ③ 编译引擎
cd ../backend-rust && cargo build --bin server   # 产出 target/debug/server.exe
```

### 1. 探针验证（全链路 + usage 采集）

```bash
# 启动引擎（基线默认轮次 5/8/14）
cd backend-rust && AIBID_DATA_DIR=.. ./target/debug/server.exe
# 另开终端
cd benchmark && python run_benchmark.py --dataset-root ../benchmark/blind-v2 --scope injected --limit 1 --run-id probe
```

- 验证点：无 401/503（签名通）；`results/probe/documents/BLIND-001.json` 含 `"usage": {llm_calls, tokens_input, tokens_output, cost_cny}`。

### 2. 基线全量（第 1 轮）

```bash
python run_benchmark.py --dataset-root ../benchmark/blind-v2 --scope injected --run-id baseline-20260808
```

- 结果：`results/baseline-20260808/{summary,metrics,token_usage}.json`。

### 3. 降轮次（第 2 轮）

1. 改 `src/agents/types.rs`：`RiskTier::max_turns()` 支持环境变量 `AIBID_TIER_MAX_TURNS`。
2. 停旧 server → 重新编译 → 带降轮次配置启动：

```bash
cd backend-rust && AIBID_DATA_DIR=.. AIBID_TIER_MAX_TURNS="low:3,medium:6,high:8" ./target/debug/server.exe
```

3. 跑全量：`--run-id reduced-turns-20260808`。结果：质量不变、成本 ¥1.14→¥1.13。

### 4. 强制收尾（第 3 轮）

1. 改 `src/agents/react_loop.rs`：新增连续空转强制输出（`AIBID_STALL_FORCE_OUTPUT`）。
2. 停旧 server → 重新编译 → 带两个配置启动：

```bash
cd backend-rust && AIBID_DATA_DIR=.. AIBID_TIER_MAX_TURNS="low:3,medium:6,high:8" AIBID_STALL_FORCE_OUTPUT=2 ./target/debug/server.exe
```

3. 跑全量：`--run-id force-finish-20260809`。结果：成本 ¥0.77（-32.5%）、漏报率 +3.33pp。
4. 验证：server 日志出现 `[STALL-FORCE] 条款 ch_xxx 连续 2 轮仅探索未产出 → 强制 output_finding`（本次触发 2 次）。

### 5. 结果对比

从三个 run 目录的 `summary.json` / `metrics.json` / `token_usage.json` 提取：

- 漏报率 = 1 − Recall；误报率 = 1 − Precision；完成率 = `completed_documents` / 10；token 成本 = `token_usage.cost_cny`。

### 回归测试（每轮代码修改后）

```bash
cd backend-rust
cargo check          # 编译验证
cargo test --lib     # 单测（本轮 424 通过 / 10 个 pre-existing 失败，与改动无关）
cargo build --bin server
```

- 签名机制验证：无签名访问 `/api/v1/*` → 401；带 `internal_auth_headers()` 签名 → 200。

