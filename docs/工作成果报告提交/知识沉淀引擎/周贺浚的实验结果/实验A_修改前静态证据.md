# 实验 A（修改前基线）静态证据 — commit 5ee8264

> 用途：证明修改前（PR/10 合并前）系统**没有知识沉淀能力**（无入库模块、无 Neo4j 依赖、无流水线入口、无入库开关）。
> 说明：基线 `5ee8264` 本身无法编译（merge PR/7 时带入其他组半成品代码，`enabled_agent_ids` 未定义、`CompareVersionsTool`/`DetectBoilerplateTool` 构造缺失、`SearchBuffer::new` 签名不匹配等），无法运行完整审核。
> 故改用**静态代码审计**（对比同一仓库 修改前 `5ee8264` ↔ 修改后 HEAD `cbe28ed`）提供证据。
> 验证日期：2026-08-09

---

## 1. 无知识沉淀模块

```bash
# 修改前（5ee8264）— 无输出，目录不存在
git ls-tree 5ee8264 -- backend-rust/src/knowledge

# 修改后（HEAD cbe28ed）— 新增 6 个文件
git ls-tree -r HEAD --name-only backend-rust/src/knowledge/
```

| 版本 | 结果 |
|---|---|
| 修改前 `5ee8264` | 无 `backend-rust/src/knowledge/` 目录 |
| 修改后 `cbe28ed` | `collect.rs` / `extract.rs` / `graph.rs` / `mod.rs` / `run.rs` / `types.rs` |

## 2. 无 Neo4j 依赖

```bash
# 修改前（5ee8264）— 无输出
git show 5ee8264:backend-rust/Cargo.toml | findstr neo4

# 修改后（HEAD）— neo4rs = "0.8.0"
git show HEAD:backend-rust/Cargo.toml | findstr neo4
```

| 版本 | 结果 |
|---|---|
| 修改前 `5ee8264` | Cargo.toml 无 `neo4rs` 依赖 |
| 修改后 `cbe28ed` | `neo4rs = "0.8.0"` |

## 3. 无知识沉淀入口（bin 工具）

```bash
git ls-tree 5ee8264 -- backend-rust/src/bin/
git ls-tree HEAD -- backend-rust/src/bin/
```

| 版本 | bin 列表 |
|---|---|
| 修改前 `5ee8264` | `blind_validate` `llm_label` `prelabel` `server` `test_agents` `test_api_key` `test_llm` `test_rules`（8 个，无知识沉淀入口） |
| 修改后 `cbe28ed` | 上述 8 个 + **`graph_write` `knowledge_pipeline` `search_knowledge` `stage_a` `stage_b`**（13 个） |

## 4. 无入库开关 / 入库逻辑

```bash
# 修改前（5ee8264）— 无输出
git grep -n "AIBID_WRITE_NEO4J" 5ee8264 -- backend-rust

# 修改后（HEAD）
git grep -n "AIBID_WRITE_NEO4J" HEAD -- backend-rust
```

| 版本 | 结果 |
|---|---|
| 修改前 `5ee8264` | 无 `AIBID_WRITE_NEO4J` 开关，`main.rs` 无任何入库步骤 |
| 修改后 `cbe28ed` | `main.rs:1048-1049` 新增「8.5 知识沉淀：审核结果 → 挑精华 → 查重 → 写 Neo4j」，默认开启，可用 `AIBID_WRITE_NEO4J=0` 关闭 |

---

## 结论

修改前 `5ee8264`：审核结束后风险结论仅落盘为 `output/findings/*_findings.json`，**无任何知识沉淀模块 / Neo4j 依赖 / 流水线入口 / 入库逻辑**，知识无法沉淀、无法跨审核查重、无法检索复用。

修改后 `cbe28ed`：新增完整知识沉淀链路（`src/knowledge/` 6 文件 + 5 个 bin 入口 + `neo4rs` 依赖 + `main.rs` 入库开关），审核结果可自动写入 Neo4j 知识图谱。

> 对照实验 B（修改后实际运行）：同一输入跑 `knowledge_pipeline` 可写入 N 条并二次运行新增 0 条（查重生效），详见报告第 6.2 节。
