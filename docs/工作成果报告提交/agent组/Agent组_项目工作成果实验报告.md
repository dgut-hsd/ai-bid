# 项目工作成果实验报告

| 字段 | 内容 |
|---|---|
| 小组名称 | Agent 组 |
| 工作方向 | Agent 成本优化（降轮次 + 强制收尾） |
| 负责人及成员 | （填写实际参与讨论开发的同学） |
| 对应 PR | （复制 PR 标题，本次改动当前在本地 `dev` 分支未提交） |
| 实验分支 / Commit | `dev`（本地改动：`backend-rust/src/agents/types.rs`、`react_loop.rs`、`api/handlers.rs`、`api/router.rs`、`benchmark/run_benchmark.py`） |
| 开发日期范围 | 2026-08-08 ~ 2026-08-09 |

---

## 1. 工作概述

**本组负责方向**：Agent 审核引擎的成本优化，验证「Agent 降轮次」与「强制收尾」两个优化方向对审核质量与 token 成本的影响。

**解决的问题**：Multi-Agent 审核每条条款可能进行多轮 LLM 推理与工具调用，token 成本较高。需要回答「轮次上限是否值得降、强制收尾是否值得用」，并给出**修改前后基于同一盲测数据集的漏报率、误报率、完成率、token 成本量化对比**。

**主要工作**：
1. 打通 token 成本采集链路（Rust server 返回 `usage` 字段 + benchmark 采集汇总）；
2. 实现降轮次开关（`AIBID_TIER_MAX_TURNS`）与强制收尾机制（`AIBID_STALL_FORCE_OUTPUT` 连续空转强制输出）；
3. 重建 blind-v2 盲测数据集并完成**三轮全量盲测**（基线 / 降轮次 / 强制收尾）；
4. 产出 3 轮指标对比报告。

---

## 2. 原有问题与工作目标

### 2.1 原有问题

修改前系统（基线）在以下方面存在成本问题：

- **问题输入**：Multi-Agent 审核模式（`AIBID_AGENT=1`、`AIBID_COORDINATOR=1`），7 个 Agent 审查条款。
- **具体表现**：10 份盲测文档共产生 **175 次 LLM 调用、1,255,339 输入 token、成本 ¥1.14、耗时 701s**；单文档最高调用 45 次（BLIND-007），部分条款存在反复搜索/重复读取的空转现象。
- **对系统影响**：单次审核成本高、耗时长，影响大批量标书审核的经济性与吞吐。
- **频率与范围**：平均每文档 17.5 次调用，高调用文档集中在条款复杂/多 agent 协作场景。
- **证据**：`benchmark/blind-v2/results/baseline-20260808/token_usage.json`（基线 token 数据）、`documents/BLIND-007.json`（45 次调用）。

### 2.2 工作目标

| 序号 | 工作目标 | 预期成果或判断标准 | 完成状态 |
|---|---|---|---|
| 1 | 打通 token 成本采集链路 | `GET /result` 返回每文档 `usage`（llm_calls / tokens_in / tokens_out / cost_cny）；benchmark 汇总出 `token_usage.json` | ✅ 完成 |
| 2 | 实现降轮次，验证轮次上限对成本的影响 | `AIBID_TIER_MAX_TURNS` 可覆盖 tier 上限；跑同数据集对照 | ✅ 完成 |
| 3 | 实现强制收尾，压缩空转调用 | `AIBID_STALL_FORCE_OUTPUT` 连续空转强制输出；成本下降且漏报可控 | ✅ 完成（成本 -32.5%，漏报 +3.33pp，见 §6） |
| 4 | 三轮全量盲测 + 对比报告 | 同数据集输出漏报率/误报率/完成率/token 成本/F1/时长 | ✅ 完成（`docs/降轮次与强制收尾优化对比报告.md`） |

> 每一项"成本更低"的描述均有 §6 的实验数据支持。

---

## 3. 解决方案与实现

### 3.1 方案说明

采用**环境变量可开关**的增量改造，保证基线、降轮次、强制收尾三轮可以用**同一二进制**切换，确保对比只反映参数差异：

```
成本 ≈ 条款数 × Agent 数 × 每 Agent 调用次数 × 单次调用上下文量
                       ↑ 本轮的两个优化抓手            ↑ 下一轮待优化（最大杠杆）
```

- **降轮次**：修改 `RiskTier::max_turns()`，新增 `AIBID_TIER_MAX_TURNS="low:N,medium:N,high:N"` 环境变量覆盖各档轮次上限（默认 5/8/14，实验档 3/6/8），未设置时保持原行为。
- **强制收尾**：在 `react_loop` 每轮 LLM 响应后检测"连续空转"——连续 N 轮**只请求探索类工具**（`read_section` / `search_document` / `search_knowledge` / `web_search`）且未产出 finding，则下一轮强制锁定 `output_finding`（`AIBID_STALL_FORCE_OUTPUT=N`，默认 0=关闭）。

选择该方案的原因：不改核心数据结构、不破坏生产默认行为（默认关闭）、改动可量化对比。

### 3.2 关键实现

1. **token 采集（Rust server）**：`ReviewResultResponse` 新增 `usage: Option<ReviewUsage>`；审核完成点先 `metrics.finalize()` 聚合 totals，再统一持久化（内存 + 磁盘 JSON 均含 usage）；`GET /result` 返回 usage。配套：`AppState.review_usages` 缓存。
2. **benchmark 接入**：`run_benchmark.py` 每份文档结果写入 `usage`，末尾聚合 `token_usage.json`；新增 HMAC 内部接口签名（对齐 Java `InternalRequestSigner`，解决 Rust server 内部鉴权 503）；修复 Windows 下 evaluate 子进程中文编码崩溃（`PYTHONIOENCODING=utf-8`）。
3. **降轮次**：`RiskTier::max_turns()` 优先读 `AIBID_TIER_MAX_TURNS`，未设置走内置默认。
4. **强制收尾**：`react_loop.rs` 新增 `consecutive_stall` 计数器；每轮检测 `r.has_output_finding()` 与探索类工具请求，达到阈值置 `force_output_next=true`，下一轮 `tool_choice` 强制 `output_finding`。
5. **盲测数据重建**：`build_blind_v2.py` 下载 10 份政府采购 PDF + 生成 30 条真值 + freeze 锁定（数据原不在仓库，被 gitignore）。

**与原有实现的变化**：降轮次前轮次上限固定 5/8/14；强制收尾前仅靠"末轮锁定"和"法规充分锁定"被动收尾，无"连续空转"主动检测。

**主要困难与解决**：
- Rust server 对 `/api/v1/*` 有 HMAC 内部鉴权，benchmark 直连 503 → 按 Java 签名算法在 Python 侧实现 `internal_auth_headers()`。
- Windows 下 evaluate 中文输出 GBK/UTF-8 冲突崩溃 → 强制子进程 `PYTHONIOENCODING=utf-8`。
- 磁盘空间不足（96% 满）导致编译/链接失败 → 清理 PDB 与增量缓存；`cargo test --lib` 替代全量 test（磁盘受限）。

---

## 4. 改动范围

### 4.1 本次完成内容

- **Rust server**：
  - `api/handlers.rs`：新增 `ReviewUsage` 结构体；`ReviewResultResponse.usage` 字段；`AppState.review_usages` 缓存；审核完成点 finalize 后统一持久化；`GET /result` 返回 usage。
  - `api/router.rs`：OpenAPI 注册 `ReviewUsage`。
  - `agents/types.rs`：`RiskTier::max_turns()` 支持 `AIBID_TIER_MAX_TURNS` 环境变量覆盖。
  - `agents/react_loop.rs`：新增连续空转强制输出（`AIBID_STALL_FORCE_OUTPUT`）。
  - `agents/tools/verify_bid_deposit.rs`：修复 9 处测试构造缺 `procurement_category` 字段（pre-existing 编译错误，随本次回归一并修复）。
- **benchmark**：
  - `run_benchmark.py`：token 采集接入 + HMAC 签名 + evaluate 编码修复 + `token_usage.json` 汇总。
  - 盲测数据重建：`benchmark/blind-v2/` 下 10 份源 PDF、mutated PDF、30 条标注、freeze_manifest。
- **文档**：`agent修改文档.md`、`docs/降轮次与强制收尾优化对比报告.md`、本报告。

### 4.2 未包含内容

- **单次调用上下文压缩**（更小 chunk、prompt 精简、检索 top-k 截断、历史裁剪）——未实现，属下一轮优化方向（数据表明是更大成本杠杆）。
- **Agent 路由精简**（只派相关 Agent）——未实现。
- **强制收尾阈值调参**（2→3/4 找平衡点）——未跑额外轮次，仅给出建议。
- **cargo clippy 全量**——因磁盘空间受限未执行（`cargo build` + `cargo test --lib` 已覆盖编译与单测）。

### 4.3 影响范围

| 模块或文件 | 主要改动 | 潜在影响或风险 |
|---|---|---|
| `backend-rust/src/api/handlers.rs` | 新增 usage 字段/缓存 | 响应体增大；旧磁盘 JSON 无 usage 时反序列化为 `None`，向后兼容 |
| `backend-rust/src/agents/types.rs` | max_turns 支持环境变量 | 生产未设置 env 时行为不变；设置后轮次上限变化 |
| `backend-rust/src/agents/react_loop.rs` | 连续空转强制输出 | 默认关闭，开启后可能提前收尾（实验显示漏报率 +3.33pp） |
| `benchmark/run_benchmark.py` | 签名 + token 采集 | 需 `RUST_API_INTERNAL_SECRET` 配置；新增 `usage`/`token_usage.json` 字段 |
| `.env` | 新增 `RUST_API_INTERNAL_SECRET` | 内部接口签名密钥，Java 网关与 benchmark 共用，需妥善保管 |

---

## 5. 验证方案

### 5.1 评价指标

| 指标名称 | 指标含义及计算方法 | 数据来源 | 修改前结果 | 目标值 |
|---|---|---|---|---|
| 漏报率 | 1 − Recall = FN/(TP+FN) | evaluate.py / metrics.json | 0% | ≤5%（成本优化不应明显漏报） |
| 误报率 | 1 − Precision = FP/(TP+FP) | evaluate.py / metrics.json | 11.76% | ≤15% |
| 完成率 | 完成文档数 / 总数 | run_benchmark summary | 10/10 | =10/10 |
| token 成本 | LLM 调用真实 usage × 单价聚合 | token_usage.json | ¥1.14/套 | 降低 ≥20% |
| 平均 Token/文档 | tokens_input / 文档数 | token_usage.json | 125,534 | 降低 |
| 平均调用轮数 | llm_calls / 文档数 | token_usage.json | 17.5 | 降低 |
| 截断率 | truncated findings / 预测数 | documents/*.json | 0% | ≤5% |
| F1 | 2PR/(P+R) | evaluate.py | 93.75% | ≥90% |

### 5.2 典型测试

| 用例或场景 | 输入或操作 | 预期结果 | 实际结果 | 结论 |
|---|---|---|---|---|
| 探针验证（链路） | `run_benchmark --limit 1` | 上传+审核+usage 采集全通过 | BLIND-001 3 条发现，usage 正确（13 次 / ¥0.08） | ✅ |
| 降轮次对照 | `AIBID_TIER_MAX_TURNS=low:3,medium:6,high:8` 全量 | 质量不变、成本下降 | F1 93.8% 不变，成本 ¥1.13（-0.9%） | ✅ 无损但收益微 |
| 强制收尾对照 | 额外 `AIBID_STALL_FORCE_OUTPUT=2` 全量 | 成本显著下降、漏报可控 | 成本 ¥0.77（-32.5%）、漏报 3.33%（漏 1 条 medium） | ⚠️ 降本显著但漏报上升 |
| 空转压缩验证 | 检查高调用文档（BLIND-007） | 调用次数下降 | 54→31 次（server 日志出现 `[STALL-FORCE]` 2 次） | ✅ 机制生效 |

---

## 6. 实验结果

| 指标 | 修改前（基线） | 修改后（降轮次+强制收尾） | 变化情况 | 是否达标 |
|---|---|---|---|---|
| 漏报率 | 0% | 3.33% | +3.33pp | ⚠️ 目标 ≤5%，达标但有牺牲 |
| 误报率 | 11.76% | 12.12% | +0.36pp | ✅ |
| 完成率 | 10/10 | 10/10 | 不变 | ✅ |
| token 成本（全套） | ¥1.14 | ¥0.77 | **-32.5%** | ✅ 目标 ≥20% |
| 平均 Token/文档 | 125,534 | 80,401 | **-36.0%** | ✅ |
| 平均调用轮数/文档 | 17.5 | 12.4 | -29.1% | ✅ |
| 总执行时间 | 701s | 664s | -5.3% | ✅ |
| F1 | 93.75% | 92.06% | -1.69pp | ✅ 目标 ≥90% |
| 截断率 | 0% | 0% | 不变 | ✅ |

**证据**（可复现）：
- 数据文件：`benchmark/blind-v2/results/{baseline-20260808, reduced-turns-20260808, force-finish-20260809}/summary.json`、`token_usage.json`、`metrics.json`
- 逐文档：`.../documents/<doc_id>.json`（含每份 `usage`）
- server 日志（强制收尾触发记录）：`[STALL-FORCE] 条款 ch_297 连续 2 轮仅探索未产出 → 强制 output_finding`（触发 2 次）
- 复现命令：`python benchmark/run_benchmark.py --dataset-root benchmark/blind-v2 --scope injected --run-id <run_id>`
- 漏检明细：`BLIND-003-F03 采购人可单方无限变更需求`（medium）

---

## 7. 结果分析

- **工作目标是否完成**：是。token 采集、降轮次、强制收尾、三轮盲测、对比报告全部完成。
- **是否证明改动有效**：强制收尾方向有效（成本 -32.5%、Token -36%）；降轮次方向**无效**（成本 -0.9%，轮次上限不是瓶颈）。
- **符合/不符合预期**：
  - 符合：强制收尾大幅压缩空转调用；降轮次无损但收益极微。
  - 不符合：降轮次几乎不省成本（预期有显著收益）；强制收尾漏报率上升 3.33pp（原以为空转强停不伤召回）。
- **主要原因**：
  - 成本大头是**每次调用的输入上下文量**与**调用次数**，而非轮次上限（注入页单条款多在 3-6 轮内收敛，很少触顶）。
  - 强制收尾漏检的 `BLIND-003-F03` 在被强停的探索链路上未完成分析——空转检测在少数条款上与"仍需探索"难以区分。
- **偶然性/适用范围**：盲测为注入页模式（`scope=injected`），单条款审核；`full` 模式（整份文件）下轮次与空转特征可能不同，结论推广需谨慎。LLM 非确定性导致逐文档调用数有波动（如 BLIND-007 三轮为 45/54/31）。

---

## 8. 问题与局限

| 问题或局限 | 造成的影响 | 原因 | 后续改进计划 |
|---|---|---|---|
| 强制收尾漏报率 +3.33pp | 漏检 1 条 medium 真值 | 连续 2 轮空转强停过于激进，部分条款仍需探索 | 阈值调大到 3/4；空转判定结合"是否引入新信息" |
| 降轮次收益极微 | 优化投入产出低 | 轮次上限非成本瓶颈 | 转向上下文压缩（更大杠杆） |
| 单次调用上下文未优化 | 成本仍有压缩空间 | 本轮未涉及 | 更小 chunk、prompt 精简、检索 top-k 截断 |
| 注入页模式局限 | 结论推广受限 | 盲测集为 injected 单条款 | 补充 `full` 模式验证 |
| LLM 非确定性 | 逐文档调用数波动 | 模型随机性 | 多轮取平均；按文档聚类分析 |

---

## 9. 最终结论

- **目标完成情况**：全部完成。token 成本采集链路打通；降轮次与强制收尾两个方向均完成同数据集三轮对照验证。
- **成果是否得到证明**：是。强制收尾使成本 -32.5%、Token -36%、调用 -29%，代价是漏报率 +3.33pp；降轮次质量无损但收益极微。数据可复现（见 §6 证据）。
- **未解决问题**：强制收尾的质量-成本平衡点未定（建议阈值 3/4 实验）；单次调用上下文压缩未实现——这是数据指向的**最大成本杠杆**，建议作为下一轮 Agent 优化重点。

---

报告人：________________　　日期：________________
