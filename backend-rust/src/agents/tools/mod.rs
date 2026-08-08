//! Agent 工具集 — ReAct 循环中 LLM 可调用的工具。
//!
//! ## 通用工具（4 个 — 已实现）
//!
//! - [`search_knowledge`] / `web_search` — 搜索外部知识库（法规/案例/负面清单）
//! - [`search_document`] — 在标书内部做语义搜索
//! - [`read_section`] — 按 chunk_id 精读条款原文
//! - [`output_finding`] — 输出审查结论（终端工具，触发循环退出）
//!
//! ## 专用审查工具（8 个）
//!
//! ### MVP 零依赖（3 个 — 已实现）
//! - [`validate_calculation`] — 数值计算验证（公式求值 + 法定阈值比对）
//! - [`check_cross_reference`] — 交叉引用完整性检查（"详见附件X"→是否存在）
//! - [`calculate_timeline`] — 时间线计算与校验（日期差 + 法定时限 + 矛盾检测）
//!
//! ### V1 模板依赖（3 个 — 已实现）
//! - [`compare_with_template`] — 模板比对（发现"没写什么"）
//! - [`search_contradiction`] — 矛盾检测（隐性升级/悬空引用/数据矛盾/逻辑冲突）
//! - [`extract_obligations`] — 投标人义务聚合（发现分散排斥）
//!
//! ### V2+（待实现）
//! - `compare_versions` — 标书版本 Diff
//! - `detect_boilerplate` — 模板残骸识别
//!
//! ## 架构
//!
//! 每个工具实现 [`AgentTool`] trait，注册到 [`ToolRegistry`]。
//! ReAct 循环通过 ToolRegistry 获取 tool definitions（发送给 LLM）
//! 和分发 tool calls（执行实际工具调用）。

use anyhow::Result;
use std::collections::HashMap;

pub mod answer_user;
pub mod calculate_timeline;
pub mod check_cross_reference;
pub mod compare_with_template;
pub mod extract_obligations;
pub mod output_finding;
pub mod output_verification_batch;
pub mod read_section;
pub mod search_contradiction;
pub mod search_document;
pub mod search_knowledge;
pub mod search_knowledge_base;
pub mod validate_calculation;

// ─── AgentTool trait ──────────────────────────────────────────

/// Agent 工具的统一 trait。
///
/// 每个工具提供三个能力：
/// 1. `name()` — LLM 调用时的函数名
/// 2. `definition()` — 发送给 LLM 的 JSON Schema 工具定义
/// 3. `execute()` — 执行工具调用，接收 JSON 参数，返回 JSON 结果
#[async_trait::async_trait]
pub trait AgentTool: Send + Sync {
    /// 工具名称（LLM 在 tool_call.name 中使用）。
    fn name(&self) -> &str;

    /// 工具定义（OpenAI/Anthropic 兼容的 function definition JSON）。
    fn definition(&self) -> serde_json::Value;

    /// 执行工具调用。
    ///
    /// * `args` — LLM 传入的参数 JSON（已从 tool_call.arguments 解析）
    ///
    /// 返回工具执行结果 JSON，将被作为 tool result 追加到对话历史。
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value>;
}

// ─── ToolRegistry ─────────────────────────────────────────────

/// 工具注册表 — 管理所有 Agent 可用的工具。
///
/// 提供 O(1) 按名称查找工具、批量获取 tool definitions 的能力。
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn AgentTool>>,
}

impl ToolRegistry {
    /// 创建空的工具注册表。
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// 注册一个工具。
    pub fn register(&mut self, tool: Box<dyn AgentTool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// 获取所有工具的 definitions（发送给 LLM）。
    pub fn definitions(&self) -> Vec<serde_json::Value> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    /// 获取指定名称列表的 tools definitions（按 AgentDefinition.tool_names 过滤）。
    /// 未在 tool_names 中列出的工具不会暴露给 LLM。
    pub fn definitions_filtered(&self, tool_names: &[String]) -> Vec<serde_json::Value> {
        self.tools
            .iter()
            .filter(|(name, _)| tool_names.contains(name))
            .map(|(_, t)| t.definition())
            .collect()
    }

    /// 获取指定名称的工具引用。
    pub fn get(&self, name: &str) -> Option<&dyn AgentTool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// 检查工具是否存在。
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// 只保留指定名称的工具，删除其余。
    /// 用于 Scout 等精简工具集的 Agent（如只保留 read_section + output_finding）。
    pub fn retain_only(&mut self, names: &[&str]) {
        self.tools.retain(|name, _| names.contains(&name.as_str()));
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
