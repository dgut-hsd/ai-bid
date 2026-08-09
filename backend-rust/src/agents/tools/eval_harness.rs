//! Group A 真实 Agent→Tool LLM Eval Harness（Phase E1）。
//!
//! 链路：EvalCase clause → 真实 Agent system prompt → 真实 Agent tool list
//! → 真实 LlmClient::chat（生产 provider）→ tool_call → 真实 Tool.execute
//! → tool result（tool role, 同 tool_call_id 回传）→ final response。
//!
//! 复用生产组件：
//! - `crate::agents::react_loop::{LlmClient, ChatMessage, ToolCall, ToolChoice, execute_tool_calls}`
//! - `crate::agents::tools::ToolRegistry` + 真实 `AgentTool::execute`
//! - `crate::agents::registry::AgentRegistry::builtin()`（真实 AgentDefinition / system prompt / tool_names）
//!
//! 原则：
//! - ground truth（expected_tool / expected_key_args）绝不进入模型 messages（防泄漏）。
//! - 不强制 tool_choice（Auto，测真实 selection）。
//! - 不读取 / 不记录 Chain-of-Thought（仅记录 model content / tool_calls / usage）。
//! - 不修改生产 Tool 业务逻辑；不修改 ReActLoop。
//! - web_search 默认 Disabled（EvalEnvironment 记录），隔离 Group A deterministic selection。

use crate::agents::react_loop::{ChatMessage, LlmClient, ToolChoice};
use crate::agents::registry::AgentRegistry;
use crate::agents::tools::calculate_timeline::CalculateTimelineTool;
use crate::agents::tools::check_scoring_completeness::CheckScoringCompletenessTool;
use crate::agents::tools::check_imported_products::CheckImportedProductsTool;
use crate::agents::tools::detect_subjective_scoring::DetectSubjectiveScoringTool;
use crate::agents::tools::output_finding::OutputFindingTool;
use crate::agents::tools::validate_calculation::ValidateCalculationTool;
use crate::agents::tools::validate_scoring_formula::ValidateScoringFormulaTool;
use crate::agents::tools::validate_weight_distribution::ValidateWeightDistributionTool;
use crate::agents::tools::verify_announcement_period::VerifyAnnouncementPeriodTool;
use crate::agents::tools::verify_bid_deposit::VerifyBidDepositTool;
use crate::agents::tools::verify_bid_preparation_period::VerifyBidPreparationPeriodTool;
use crate::agents::tools::verify_consortium_rules::VerifyConsortiumRulesTool;
use crate::agents::tools::verify_procurement_method::VerifyProcurementMethodTool;
use crate::agents::tools::{AgentTool, ToolRegistry};
use crate::agents::types::{AgentDefinition, AgentId};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ─── EvalCase Schema（ground truth，仅供 evaluator，绝不下发模型）─────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expectation {
    Required,
    Preferred,
    Negative,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub case_id: String,
    /// 目标 Agent 名（"Procedure" / "Scoring" / "Demand"），不靠 expected_tool 猜。
    pub agent_id: String,
    pub clause: String,
    pub expectation: Expectation,
    /// Negative case 为 None。
    pub expected_tool: Option<String>,
    /// 期望出现的关键参数（key presence + 若有明确值则语义比对）。
    pub expected_key_args: Vec<ExpectedArg>,
    #[serde(default)]
    pub forbidden_tools: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// 期望参数：Presence（key 必须出现）+ 可选的 canonical value（语义比对）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedArg {
    pub key: String,
    pub value: Option<String>,
}

impl ExpectedArg {
    pub fn presence(key: &str) -> Self {
        Self { key: key.to_string(), value: None }
    }
    pub fn value(key: &str, value: &str) -> Self {
        Self { key: key.to_string(), value: Some(value.to_string()) }
    }
}

// ─── Sampling / Run Config ────────────────────────────────────────────────

/// web_search 在 Eval 环境中的策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchPolicy {
    /// 从下发 tools 中剔除 web_search（隔离外部 I/O 对 selection 的影响）。
    Disabled,
    /// 用 deterministic stub 替代（返回固定"disabled in eval"）。
    Stubbed,
    Enabled,
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    /// 记录用 model 名（实际由 provider 决定；当前生产 trait 不透传 model 参数）。
    pub model: String,
    /// 记录用 provider 名，如 "dashscope_native" / "openai_compatible" / "fake"。
    pub provider: String,
    pub repetitions: u32,
    /// 生产 LlmClient::chat 当前不透传 temperature / seed（provider 固定）。作为 run-level sampling 记录。
    pub temperature: Option<f64>,
    pub seed: Option<u64>,
    /// "CurrentProductionPrompt" 默认；未来 A/B 用 "BaselineWithoutToolGuidance" 等。
    pub prompt_variant: String,
    /// 覆盖 system prompt（variant 注入用）；None = 真实 AgentDefinition.system_prompt。
    pub system_prompt_override: Option<String>,
    /// 与生产一致（AgentDefinition.default_max_turns）。
    pub max_tool_rounds: usize,
    pub web_search: WebSearchPolicy,
    /// API transient failure 最大重试（不重试选错 Tool / 参数错误）。
    pub retry_limit: u32,
}

impl RunConfig {
    pub fn production_default() -> Self {
        Self {
            model: "qwen-plus".to_string(),
            provider: "dashscope_native".to_string(),
            repetitions: 1,
            temperature: None,
            seed: None,
            prompt_variant: "CurrentProductionPrompt".to_string(),
            system_prompt_override: None,
            max_tool_rounds: 12,
            web_search: WebSearchPolicy::Disabled,
            retry_limit: 2,
        }
    }
}

// ─── 指标记录结构 ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub sequence: usize,
    pub tool_name: String,
    pub raw_arguments: String,
    pub parsed_arguments: Option<serde_json::Value>,
    pub argument_json_valid: bool,
    /// 对照该 Tool definition 的 parameters.required 做字段存在性校验（近似 deserialize success）。
    pub schema_valid: bool,
    /// "success" | "error" | "unregistered"
    pub execution_status: String,
    pub tool_result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub run_id: String,
    pub case_id: String,
    pub agent_id: String,
    pub expectation: Expectation,
    pub expected_tool: Option<String>,
    /// expected_key_args 个数（aggregation 分母条件用）。
    pub expected_key_args_count: usize,
    pub model: String,
    pub prompt_variant: String,
    pub tool_calls: Vec<ToolCallRecord>,
    /// Required/Preferred case：至少一次调用 expected_tool。
    pub selected_expected_tool: bool,
    /// Required/Preferred case：调用过 Group A Tool 但从未调用 expected_tool。
    pub wrong_tool: bool,
    /// Negative case：调用过任何 Group A specialist Tool。
    pub false_call: bool,
    /// expected_key_args 中出现的 key 比例。
    pub argument_presence_score: f64,
    pub argument_schema_valid: bool,
    pub execution_success: bool,
    /// 是否有结构化结论且 final response 存在（可自动判断 adoption）。
    pub adoption_evaluable: bool,
    pub result_adopted: bool,
    /// Consistent / Ignored / Contradicted / Unknown。
    pub tool_result_consistency: String,
    pub final_response: Option<String>,
    pub tool_round_count: usize,
    pub max_tool_rounds_exceeded: bool,
    pub latency_ms: f64,
    /// token usage (input, output, total)。
    pub token_usage: Option<(u32, u32, u32)>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub numerator: usize,
    pub denominator: usize,
}

impl Metric {
    pub fn new(n: usize, d: usize) -> Self {
        Self { numerator: n, denominator: d }
    }
    pub fn rate(&self) -> f64 {
        if self.denominator == 0 {
            0.0
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub timestamp: String,
    pub model: String,
    pub provider: String,
    pub prompt_variant: String,
    pub prompt_hash: u64,
    pub repetitions: u32,
    pub temperature: Option<f64>,
    pub seed: Option<u64>,
    pub web_search: String,
    pub git_revision: String,
    pub case_count: usize,
    pub required_recall: Metric,
    pub preferred_use_rate: Metric,
    pub tool_precision: Metric,
    pub wrong_tool_rate: Metric,
    pub false_call_rate: Metric,
    pub argument_json_valid_rate: Metric,
    pub argument_deserialize_success_rate: Metric,
    pub expected_key_arg_recall: Metric,
    pub tool_execution_success_rate: Metric,
    pub result_adoption_rate: Metric,
    pub final_tool_consistency_rate: Metric,
}

// ─── Eval 环境说明 ────────────────────────────────────────────────────────

pub struct EvalEnvironment {
    pub web_search: String,
    pub registry_note: String,
}

pub fn eval_environment(cfg: &RunConfig) -> EvalEnvironment {
    EvalEnvironment {
        web_search: match cfg.web_search {
            WebSearchPolicy::Disabled => "disabled (removed from tools sent to model)".to_string(),
            WebSearchPolicy::Stubbed => "stubbed (deterministic stub)".to_string(),
            WebSearchPolicy::Enabled => "enabled (real external I/O)".to_string(),
        },
        registry_note: "Eval registry = Group A core tools + output_finding (real Tool.execute)".to_string(),
    }
}

// ─── 真实 Tool Registry（Eval 环境）──────────────────────────────────────

/// 构建 Eval 使用的真实 ToolRegistry：Group A 核心 + output_finding。
/// web_search / search_document / read_section 等外部/文档 infra 工具不纳入第一版
/// （记录在 EvalEnvironment.registry_note），避免外部 I/O 影响 deterministic selection。
pub fn build_eval_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(OutputFindingTool));
    reg.register(Box::new(VerifyProcurementMethodTool));
    reg.register(Box::new(VerifyBidDepositTool));
    reg.register(Box::new(VerifyAnnouncementPeriodTool));
    reg.register(Box::new(VerifyBidPreparationPeriodTool));
    reg.register(Box::new(CalculateTimelineTool));
    reg.register(Box::new(ValidateScoringFormulaTool));
    reg.register(Box::new(ValidateWeightDistributionTool));
    reg.register(Box::new(DetectSubjectiveScoringTool));
    reg.register(Box::new(CheckScoringCompletenessTool));
    reg.register(Box::new(VerifyConsortiumRulesTool));
    reg.register(Box::new(CheckImportedProductsTool));
    reg.register(Box::new(ValidateCalculationTool));
    reg
}

/// 从 AgentRegistry::builtin() 取真实 AgentDefinition（system prompt / tool_names / max_turns）。
pub fn agent_definition(agent_id: &str) -> Result<crate::agents::types::AgentDefinition> {
    let id = match agent_id {
        "Procedure" => AgentId::Procedure,
        "Scoring" => AgentId::Scoring,
        "Demand" => AgentId::Demand,
        "FactCheck" => AgentId::FactCheck,
        other => AgentId::parse(other).ok_or_else(|| anyhow!("未知 Agent: {}", other))?,
    };
    AgentRegistry::builtin()
        .get(id)
        .cloned()
        .ok_or_else(|| anyhow!("AgentDefinition 不存在: {}", agent_id))
}

// ─── 参数校验（对照真实 Tool definition schema）────────────────────────

fn tool_schema_required(registry: &ToolRegistry, name: &str) -> Vec<String> {
    let Some(tool) = registry.get(name) else { return vec![] };
    let def = tool.definition();
    def["function"]["parameters"]["required"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

/// schema_valid：raw JSON 可解析为 object 且 definition.required 字段全部存在。
fn schema_validate(name: &str, args: &serde_json::Value, registry: &ToolRegistry) -> bool {
    let Some(obj) = args.as_object() else { return false };
    tool_schema_required(registry, name).iter().all(|k| obj.contains_key(k))
}

// ─── Adoption Extractor（deterministic，基于结构化字段，非关键词猜测）────

/// 从 Tool Result 提取规范化结论状态。
/// 优先结构化字段：overall_status / status / verdict / compliant。
fn extract_tool_conclusion(result: &serde_json::Value) -> Option<String> {
    let status = result
        .get("overall_status")
        .or_else(|| result.get("status"))
        .or_else(|| result.get("verdict"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if status.is_some() {
        return status;
    }
    // 布尔字段 compliant / violation（部分工具输出）
    for key in ["compliant", "violation"] {
        if let Some(b) = result.get(key).and_then(|v| v.as_bool()) {
            if b {
                return Some(key.to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptionVerdict {
    Consistent,
    Ignored,
    Contradicted,
    Unknown,
}

/// 判断 final response 与 Tool 结论的一致性（deterministic 文本比对）。
fn judge_adoption(final_response: &str, conclusion: &str) -> AdoptionVerdict {
    let norm = conclusion.to_lowercase();
    let (positive, negative): (Vec<&str>, Vec<&str>) = match norm.as_str() {
        "compliant" | "ok" => (vec!["合规", "符合", "compliant"], vec!["违规", "不合规", "violation", "超标"]),
        "violation" => (vec!["违规", "不合规", "violation", "超标"], vec!["合规", "符合", "compliant"]),
        _ => (vec![], vec![]),
    };
    if positive.is_empty() {
        // 无法自动判断的结论类型（如 calculated / uncertain / not_applicable）
        return AdoptionVerdict::Unknown;
    }
    let text = final_response.to_lowercase();
    let has_positive = positive.iter().any(|k| text.contains(k));
    let has_negative = negative.iter().any(|k| text.contains(k));
    if has_negative {
        AdoptionVerdict::Contradicted
    } else if has_positive {
        AdoptionVerdict::Consistent
    } else {
        AdoptionVerdict::Ignored
    }
}

// ─── 单 Case 执行（复用生产组件）────────────────────────────────────────

/// 执行单个 EvalCase：真实 prompt + 真实 tool list + 真实 LLM + 真实 Tool.execute。
pub async fn run_case(
    run_id: &str,
    cfg: &RunConfig,
    case: &EvalCase,
    llm: &dyn LlmClient,
    registry: &ToolRegistry,
) -> Result<CaseResult> {
    let agent = agent_definition(&case.agent_id)?;
    let max_rounds = if cfg.max_tool_rounds > 0 {
        cfg.max_tool_rounds
    } else {
        agent.default_max_turns
    };

    let system_prompt = cfg
        .system_prompt_override
        .clone()
        .unwrap_or_else(|| agent.system_prompt.to_string());

    // 真实 tool list：AgentDefinition.tool_names ∩ eval registry（web_search 按 policy 剔除）
    let mut tool_names: Vec<String> = agent
        .tool_names
        .iter()
        .filter(|n| {
            if **n == "web_search" && cfg.web_search == WebSearchPolicy::Disabled {
                return false;
            }
            true
        })
        .map(|s| s.to_string())
        .collect();
    if cfg.web_search == WebSearchPolicy::Stubbed {
        tool_names.push("web_search".to_string());
    }
    let tools = registry.definitions_filtered(&tool_names);

    let mut messages = vec![
        ChatMessage::System { content: system_prompt },
        ChatMessage::User { content: case.clause.clone() },
    ];

    let mut tool_calls: Vec<ToolCallRecord> = Vec::new();
    let mut final_response: Option<String> = None;
    let mut token_usage: Option<(u32, u32, u32)> = None;
    let mut round = 0usize;
    let mut exceeded = false;
    let mut total_latency = Duration::ZERO;

    loop {
        if round >= max_rounds {
            exceeded = true;
            break;
        }
        round += 1;

        // 有限 retry（仅 transient API 错误；选错 Tool / 参数错误不重试）
        let t0 = Instant::now();
        let mut attempt = 0u32;
        let response = loop {
            match llm.chat(&messages, &tools, &ToolChoice::Auto).await {
                Ok(r) => break r,
                Err(e) => {
                    attempt += 1;
                    if attempt > cfg.retry_limit {
                        return Err(anyhow!("LLM 调用失败（重试 {} 次）: {}", cfg.retry_limit, e));
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        };
        total_latency += t0.elapsed();
        if let Some(u) = &response.usage {
            token_usage = Some((u.input_tokens, u.output_tokens, u.total_tokens));
        }

        if response.tool_calls.is_empty() {
            final_response = response.content.clone();
            break;
        }

        // capture tool calls + schema 校验
        for (i, tc) in response.tool_calls.iter().enumerate() {
            let json_valid = tc.arguments.is_object();
            let schema_ok = json_valid && schema_validate(&tc.name, &tc.arguments, registry);
            tool_calls.push(ToolCallRecord {
                sequence: tool_calls.len() + i + 1,
                tool_name: tc.name.clone(),
                raw_arguments: tc.arguments.to_string(),
                parsed_arguments: Some(tc.arguments.clone()),
                argument_json_valid: json_valid,
                schema_valid: schema_ok,
                execution_status: "pending".to_string(),
                tool_result: None,
                error: None,
                latency_ms: 0.0,
            });
        }

        // 生产 dispatch：与 execute_tool_calls 等价（registry.get → Tool.execute → Tool message）
        let t1 = Instant::now();
        let assistant_calls = response.tool_calls.clone();
        messages.push(ChatMessage::Assistant {
            content: response.content.clone(),
            tool_calls: Some(assistant_calls.clone()),
        });
        for tc in &assistant_calls {
            let (result, status, err) = if let Some(tool) = registry.get(&tc.name) {
                match tool.execute(tc.arguments.clone()).await {
                    Ok(val) => (val, "success", None),
                    Err(e) => (
                        serde_json::json!({ "error": format!("{}", e) }),
                        "error",
                        Some(format!("{}", e)),
                    ),
                }
            } else {
                (
                    serde_json::json!({ "error": format!("工具 '{}' 未注册", tc.name) }),
                    "unregistered",
                    Some(format!("工具 '{}' 未注册", tc.name)),
                )
            };
            // 回填该 tool call 的执行结果（按 sequence 顺序）
            let idx = tool_calls.iter().position(|r| {
                r.tool_name == tc.name && r.execution_status == "pending"
            });
            if let Some(idx) = idx {
                tool_calls[idx].execution_status = status.to_string();
                tool_calls[idx].tool_result = Some(result.clone());
                tool_calls[idx].error = err;
                tool_calls[idx].latency_ms = t1.elapsed().as_secs_f64() * 1000.0;
            }
            messages.push(ChatMessage::Tool {
                tool_call_id: tc.id.clone(),
                content: serde_json::to_string(&result).unwrap_or_default(),
            });
        }
        total_latency += t1.elapsed();
    }

    // ── 指标判定 ──
    let selected_expected_tool = case
        .expected_tool
        .as_ref()
        .map(|et| tool_calls.iter().any(|r| &r.tool_name == et))
        .unwrap_or(false);

    let called_group_a = !tool_calls.is_empty();
    let wrong_tool = (case.expectation == Expectation::Required || case.expectation == Expectation::Preferred)
        && called_group_a
        && !selected_expected_tool;

    let false_call = case.expectation == Expectation::Negative && called_group_a;

    // argument presence：expected_key_args 的 key 在 expected_tool 调用中的出现率
    let (arg_presence, arg_recall_ok) = {
        let et_call = case
            .expected_tool
            .as_ref()
            .and_then(|et| tool_calls.iter().find(|r| &r.tool_name == et))
            .and_then(|r| r.parsed_arguments.as_ref());
        let expected_args = &case.expected_key_args;
        if expected_args.is_empty() {
            (1.0, true)
        } else if let Some(args) = et_call {
            let present = expected_args
                .iter()
                .filter(|ea| {
                    args.get(&ea.key).is_some_and(|v| match &ea.value {
                        Some(expected) => v.as_str().map(|s| s == expected).unwrap_or(false),
                        None => true,
                    })
                })
                .count();
            let score = present as f64 / expected_args.len() as f64;
            (score, present == expected_args.len())
        } else {
            (0.0, false)
        }
    };

    let argument_schema_valid = tool_calls
        .iter()
        .filter(|r| r.argument_json_valid)
        .all(|r| r.schema_valid);

    let execution_success = !tool_calls.is_empty()
        && tool_calls.iter().all(|r| r.execution_status == "success");

    // adoption：deterministic per-case 判断
    let (adoption_evaluable, result_adopted, consistency) = if tool_calls.is_empty() {
        (false, false, "no_tool_call".to_string())
    } else {
        // 用第一个 Group A tool 的结论（第一版：单 tool 场景为主）
        let conclusion = tool_calls
            .iter()
            .find(|r| r.execution_status == "success")
            .and_then(|r| r.tool_result.as_ref())
            .and_then(extract_tool_conclusion);
        match (conclusion, final_response.as_ref()) {
            (Some(c), Some(fr)) => {
                let v = judge_adoption(fr, &c);
                match v {
                    AdoptionVerdict::Unknown => (false, false, "unknown".to_string()),
                    AdoptionVerdict::Consistent => (true, true, "consistent".to_string()),
                    AdoptionVerdict::Ignored => (true, false, "ignored".to_string()),
                    AdoptionVerdict::Contradicted => (true, false, "contradicted".to_string()),
                }
            }
            _ => (false, false, "no_conclusion_or_final".to_string()),
        }
    };

    Ok(CaseResult {
        run_id: run_id.to_string(),
        case_id: case.case_id.clone(),
        agent_id: case.agent_id.clone(),
        expectation: case.expectation,
        expected_tool: case.expected_tool.clone(),
        expected_key_args_count: case.expected_key_args.len(),
        model: cfg.model.clone(),
        prompt_variant: cfg.prompt_variant.clone(),
        tool_calls,
        selected_expected_tool,
        wrong_tool,
        false_call,
        argument_presence_score: arg_presence,
        argument_schema_valid,
        execution_success,
        adoption_evaluable,
        result_adopted,
        tool_result_consistency: consistency,
        final_response,
        tool_round_count: round,
        max_tool_rounds_exceeded: exceeded,
        latency_ms: total_latency.as_secs_f64() * 1000.0,
        token_usage,
        error: if exceeded {
            Some("max_tool_rounds exceeded".to_string())
        } else {
            None
        },
    })
}

// ─── 指标聚合 ─────────────────────────────────────────────────────────────

pub fn aggregate(cases: &[CaseResult]) -> RunSummary {
    let required: Vec<&CaseResult> = cases.iter().filter(|c| c.expectation == Expectation::Required).collect();
    let preferred: Vec<&CaseResult> = cases.iter().filter(|c| c.expectation == Expectation::Preferred).collect();
    let negative: Vec<&CaseResult> = cases.iter().filter(|c| c.expectation == Expectation::Negative).collect();
    let scored: Vec<&CaseResult> = required.iter().chain(preferred.iter()).cloned().collect();

    // Required Tool Recall
    let req_recall = Metric::new(
        required.iter().filter(|c| c.selected_expected_tool).count(),
        required.len(),
    );
    // Preferred Tool Use Rate
    let pref_use = Metric::new(
        preferred.iter().filter(|c| c.selected_expected_tool).count(),
        preferred.len(),
    );
    // Tool Precision: 正确 tool calls / 全部被评分 tool calls
    let (correct_calls, all_calls) = scored.iter().fold((0usize, 0usize), |(cc, ac), c| {
        let calls = c.tool_calls.len();
        let ok = c
            .tool_calls
            .iter()
            .filter(|r| c.expected_tool.as_deref() == Some(r.tool_name.as_str()))
            .count();
        (cc + ok, ac + calls)
    });
    let precision = Metric::new(correct_calls, all_calls);
    // Wrong Tool Rate
    let wrong = Metric::new(
        scored.iter().filter(|c| c.wrong_tool).count(),
        scored.len(),
    );
    // False Call Rate
    let false_call = Metric::new(
        negative.iter().filter(|c| c.false_call).count(),
        negative.len(),
    );
    // Argument JSON Valid / Deserialize
    let (json_ok, schema_ok, exec_ok, total) = cases.iter().fold(
        (0usize, 0usize, 0usize, 0usize),
        |(j, s, e, t), c| {
            (
                j + c.tool_calls.iter().filter(|r| r.argument_json_valid).count(),
                s + c.tool_calls.iter().filter(|r| r.schema_valid).count(),
                e + c.tool_calls.iter().filter(|r| r.execution_status == "success").count(),
                t + c.tool_calls.len(),
            )
        },
    );
    let arg_json = Metric::new(json_ok, total);
    let arg_deser = Metric::new(schema_ok, total);
    let exec_success = Metric::new(exec_ok, total);
    // Expected Key Arg Recall: 分母 = Required/Preferred 且调用了 expected_tool 且 expected_key_args 非空
    let (arg_n, arg_d) = scored.iter().fold((0usize, 0usize), |(n, d), c| {
        if c.selected_expected_tool && c.expected_key_args_count > 0 {
            (n + (c.argument_presence_score >= 1.0) as usize, d + 1)
        } else {
            (n, d)
        }
    });
    let arg_recall = Metric::new(arg_n, arg_d);
    // Result Adoption Rate
    let adopt = Metric::new(
        cases.iter().filter(|c| c.adoption_evaluable && c.result_adopted).count(),
        cases.iter().filter(|c| c.adoption_evaluable).count(),
    );
    // Final Tool-Consistency Rate: 分母 = 有 tool call 且有 final response 的 case
    let (con_n, con_d) = cases.iter().fold((0usize, 0usize), |(n, d), c| {
        if !c.tool_calls.is_empty() && c.final_response.is_some() {
            (n + (c.tool_result_consistency == "consistent") as usize, d + 1)
        } else {
            (n, d)
        }
    });
    let consistency = Metric::new(con_n, con_d);

    RunSummary {
        run_id: cases.first().map(|c| c.run_id.clone()).unwrap_or_default(),
        timestamp: String::new(),
        model: cases.first().map(|c| c.model.clone()).unwrap_or_default(),
        provider: String::new(),
        prompt_variant: cases.first().map(|c| c.prompt_variant.clone()).unwrap_or_default(),
        prompt_hash: 0,
        repetitions: 1,
        temperature: None,
        seed: None,
        web_search: String::new(),
        git_revision: "unavailable".to_string(),
        case_count: cases.len(),
        required_recall: req_recall,
        preferred_use_rate: pref_use,
        tool_precision: precision,
        wrong_tool_rate: wrong,
        false_call_rate: false_call,
        argument_json_valid_rate: arg_json,
        argument_deserialize_success_rate: arg_deser,
        expected_key_arg_recall: arg_recall,
        tool_execution_success_rate: exec_success,
        result_adoption_rate: adopt,
        final_tool_consistency_rate: consistency,
    }
}

impl CaseResult {
    // 占位（后续可扩展 case-level 辅助方法）
}

/// 完整 Eval：cases × repetitions → (summary, results)。
pub async fn run_eval(
    cfg: &RunConfig,
    llm: &dyn LlmClient,
    cases: &[EvalCase],
    registry: &ToolRegistry,
) -> Result<(RunSummary, Vec<CaseResult>)> {
    let run_id = format!(
        "eval_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let mut results = Vec::new();
    for _rep in 0..cfg.repetitions.max(1) {
        for case in cases {
            match run_case(&run_id, cfg, case, llm, registry).await {
                Ok(r) => results.push(r),
                Err(e) => results.push(CaseResult {
                    run_id: run_id.clone(),
                    case_id: case.case_id.clone(),
                    agent_id: case.agent_id.clone(),
                    expectation: case.expectation,
                    expected_tool: case.expected_tool.clone(),
                    expected_key_args_count: case.expected_key_args.len(),
                    model: cfg.model.clone(),
                    prompt_variant: cfg.prompt_variant.clone(),
                    tool_calls: vec![],
                    selected_expected_tool: false,
                    wrong_tool: false,
                    false_call: false,
                    argument_presence_score: 0.0,
                    argument_schema_valid: false,
                    execution_success: false,
                    adoption_evaluable: false,
                    result_adopted: false,
                    tool_result_consistency: "runner_error".to_string(),
                    final_response: None,
                    tool_round_count: 0,
                    max_tool_rounds_exceeded: false,
                    latency_ms: 0.0,
                    token_usage: None,
                    error: Some(format!("{}", e)),
                }),
            }
        }
    }
    let mut summary = aggregate(&results);
    summary.run_id = run_id.clone();
    summary.model = cfg.model.clone();
    summary.provider = cfg.provider.clone();
    summary.prompt_variant = cfg.prompt_variant.clone();
    summary.prompt_hash = hash_str(&cfg.prompt_variant);
    summary.repetitions = cfg.repetitions.max(1);
    summary.temperature = cfg.temperature;
    summary.seed = cfg.seed;
    summary.web_search = eval_environment(cfg).web_search;
    Ok((summary, results))
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

// ─── 结果保存（机器可读 JSON / JSONL）───────────────────────────────────

/// Smoke baseline 数据集：与 `eval_test::tool_selection_eval::all_cases()` 同步的 14 条
/// harness schema cases（case_id 集合一致性由 `dataset_14_cases_alignment` 测试锁定）。
pub fn production_smoke_cases() -> Vec<EvalCase> {
    vec![
        EvalCase {
            case_id: "proc_001".to_string(),
            agent_id: "Procedure".to_string(),
            clause: "投标人须在投标截止时间前提交投标保证金人民币壹拾万元整（¥100,000.00），以现金形式缴纳，中标通知书发出后10个工作日内退还。合同估算金额为人民币300万元。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("verify_bid_deposit".to_string()),
            expected_key_args: vec![ExpectedArg::value("deposit_type", "bid")],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "proc_002a".to_string(),
            agent_id: "Procedure".to_string(),
            clause: "招标公告于2025年6月2日发布，公告期至2025年6月6日结束，共4个工作日。本项目采用公开招标方式，采购货物。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("verify_announcement_period".to_string()),
            expected_key_args: vec![
                ExpectedArg::value("procurement_method", "公开招标"),
                ExpectedArg::value("period_type", "notice_publication"),
            ],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "proc_002b".to_string(),
            agent_id: "Procedure".to_string(),
            clause: "招标文件自2025年6月1日起发出，投标截止时间为2025年6月14日。本项目采用公开招标方式，采购货物。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("verify_bid_preparation_period".to_string()),
            expected_key_args: vec![ExpectedArg::value("procurement_method", "公开招标")],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "proc_003".to_string(),
            agent_id: "Procedure".to_string(),
            clause: "公告发布之日起至投标截止之日止，投标准备时间仅为8个日历日，本项目采用竞争性谈判方式。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("verify_bid_preparation_period".to_string()),
            expected_key_args: vec![ExpectedArg::value("procurement_method", "竞争性谈判")],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "proc_004".to_string(),
            agent_id: "Procedure".to_string(),
            clause: "本项目预算金额为人民币500万元，采购货物一批，拟采用竞争性磋商方式进行采购。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("verify_procurement_method".to_string()),
            expected_key_args: vec![
                ExpectedArg::value("procurement_category", "货物"),
                ExpectedArg::value("budget_amount_wan", "500"),
            ],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "proc_005".to_string(),
            agent_id: "Procedure".to_string(),
            clause: "2025年6月1日发布公告，2025年6月25日开标，2025年6月20日发售招标文件，2025年6月30日签订合同。请计算各节点之间的日期关系。".to_string(),
            expectation: Expectation::Preferred,
            expected_tool: Some("calculate_timeline".to_string()),
            expected_key_args: vec![],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "proc_006".to_string(),
            agent_id: "Procedure".to_string(),
            clause: "合同签订后30日内支付合同金额的50%作为预付款，余款在验收合格后15日内付清。".to_string(),
            expectation: Expectation::Negative,
            expected_tool: None,
            expected_key_args: vec![],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "score_001".to_string(),
            agent_id: "Scoring".to_string(),
            clause: "价格分权重占比70%，采用最低价法计算价格分，基准价为所有有效投标报价的算术平均值。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("validate_scoring_formula".to_string()),
            expected_key_args: vec![
                ExpectedArg::value("procurement_object", "goods"),
                ExpectedArg::value("procurement_method", "open_tender"),
                ExpectedArg::value("price_weight", "70"),
            ],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "score_002".to_string(),
            agent_id: "Scoring".to_string(),
            clause: "评审因素权重分配如下：价格40分，技术50分，商务5分，服务5分，总分100分。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("validate_weight_distribution".to_string()),
            expected_key_args: vec![ExpectedArg::value("procurement_category", "货物")],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "score_003".to_string(),
            agent_id: "Scoring".to_string(),
            clause: "技术方案评分区间为0-20分，评委根据投标人的综合表现和满意程度酌情打分。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("detect_subjective_scoring".to_string()),
            expected_key_args: vec![],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "score_004".to_string(),
            agent_id: "Scoring".to_string(),
            clause: "评审因素表：价格分30分，技术分50分，商务分15分。总分应为100分但表格合计95分。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("check_scoring_completeness".to_string()),
            expected_key_args: vec![ExpectedArg::value("procurement_category", "货物")],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "score_005".to_string(),
            agent_id: "Scoring".to_string(),
            clause: "本项目接受联合体投标。联合体成员资质可以叠加计算，牵头方须具备施工总承包一级资质。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("verify_consortium_rules".to_string()),
            expected_key_args: vec![],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "score_006".to_string(),
            agent_id: "Demand".to_string(),
            clause: "本项目核心设备须为某指定品牌原装进口产品，不接受其他品牌，且投标人须提供厂家授权书。".to_string(),
            expectation: Expectation::Negative,
            expected_tool: None,
            expected_key_args: vec![],
            forbidden_tools: vec![],
            notes: None,
        },
        EvalCase {
            case_id: "demand_001".to_string(),
            agent_id: "Demand".to_string(),
            clause: "本项目拟采购进口医疗影像设备，采购前须经省级以上财政部门审核同意。".to_string(),
            expectation: Expectation::Required,
            expected_tool: Some("check_imported_products".to_string()),
            expected_key_args: vec![ExpectedArg::value("procurement_category", "货物")],
            forbidden_tools: vec![],
            notes: None,
        },
    ]
}

pub fn save_results(
    dir: &std::path::Path,
    summary: &RunSummary,
    cases: &[CaseResult],
) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let summary_path = dir.join("run_summary.json");
    std::fs::write(&summary_path, serde_json::to_string_pretty(summary)?)?;
    let cases_path = dir.join("case_results.jsonl");
    let mut lines = String::new();
    for c in cases {
        lines.push_str(&serde_json::to_string(c)?);
        lines.push('\n');
    }
    std::fs::write(&cases_path, lines)?;
    Ok(())
}

// ─── 测试 ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::react_loop::{ChatMessage, LlmResponse, ToolCall};
    use crate::agents::tools::eval_test::tool_selection_eval;
    use std::sync::Mutex;

    // ── FakeModel / MockProvider（scripted LlmResponse 序列）──────────

    struct FakeLlmClient {
        turns: Mutex<std::collections::VecDeque<LlmResponse>>,
        /// 收到的全部 messages（防泄漏测试用）。
        received: Mutex<Vec<Vec<ChatMessage>>>,
    }

    impl FakeLlmClient {
        fn new(turns: Vec<LlmResponse>) -> Self {
            Self {
                turns: Mutex::new(turns.into()),
                received: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for FakeLlmClient {
        async fn chat(
            &self,
            messages: &[ChatMessage],
            _tools: &[serde_json::Value],
            _tool_choice: &ToolChoice,
        ) -> Result<LlmResponse> {
            self.received
                .lock()
                .unwrap()
                .push(messages.to_vec());
            self.turns
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow!("FakeLlmClient turns 耗尽"))
        }
    }

    fn tc(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: format!("call_{}", name),
            name: name.to_string(),
            arguments: args,
        }
    }

    fn text_response(s: &str) -> LlmResponse {
        LlmResponse {
            content: Some(s.to_string()),
            thought: None,
            tool_calls: vec![],
            usage: None,
        }
    }

    fn tc_response(calls: Vec<ToolCall>) -> LlmResponse {
        LlmResponse {
            content: None,
            thought: None,
            tool_calls: calls,
            usage: None,
        }
    }

    fn mk_case(
        case_id: &str,
        agent: &str,
        clause: &str,
        expectation: Expectation,
        expected_tool: Option<&str>,
        expected_key_args: Vec<ExpectedArg>,
    ) -> EvalCase {
        EvalCase {
            case_id: case_id.to_string(),
            agent_id: agent.to_string(),
            clause: clause.to_string(),
            expectation,
            expected_tool: expected_tool.map(|s| s.to_string()),
            expected_key_args,
            forbidden_tools: vec![],
            notes: None,
        }
    }

    fn deposit_args() -> serde_json::Value {
        serde_json::json!({
            "deposit_amount": 30.0,
            "budget_amount": 1500.0,
            "deposit_form": "保函",
            "deposit_type": "bid",
            "procurement_category": "货物"
        })
    }

    fn deposit_case(clause: &str) -> EvalCase {
        mk_case(
            "offline_001",
            "Procedure",
            clause,
            Expectation::Required,
            Some("verify_bid_deposit"),
            vec![ExpectedArg::presence("deposit_type")],
        )
    }

    async fn run_single(case: &EvalCase, llm: &FakeLlmClient) -> CaseResult {
        let cfg = RunConfig::production_default();
        let reg = build_eval_registry();
        run_case("test_run", &cfg, case, llm, &reg).await.expect("run_case 应成功")
    }

    // ── A-G 离线场景 ──────────────────────────────────────────────

    #[tokio::test]
    async fn offline_correct_tool_call_adopted() {
        // A: 正确 tool call → recall + execution success + adoption consistent
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("投标保证金比例合规，未超过法定上限。"),
        ]);
        let r = run_single(&deposit_case("投标保证金30万，预算1500万。"), &llm).await;
        assert!(r.selected_expected_tool, "应选中 expected_tool");
        assert!(r.execution_success, "真实 Tool.execute 应成功");
        assert_eq!(r.tool_calls.len(), 1);
        assert!(r.tool_calls[0].argument_json_valid);
        assert!(r.tool_calls[0].schema_valid, "合法 args 应通过 schema 校验");
        assert!(r.adoption_evaluable, "有结构化结论 + final response");
        assert!(r.result_adopted, "final '合规' 应与 tool compliant 一致");
        assert_eq!(r.tool_result_consistency, "consistent");
        assert_eq!(r.argument_presence_score, 1.0);
    }

    #[tokio::test]
    async fn offline_wrong_tool_call() {
        // B: wrong tool → recall=0, wrong_tool=true（真实执行 scoring tool，args 不完整 → error 但不影响判定）
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc("validate_scoring_formula", serde_json::json!({"price_weight": 70.0}))]),
            text_response("评分权重需复核。"),
        ]);
        let r = run_single(&deposit_case("投标保证金30万，预算1500万。"), &llm).await;
        assert!(!r.selected_expected_tool, "错误工具不应计入 recall");
        assert!(r.wrong_tool, "Required case 调用了非 expected tool 且未调用 expected → wrong");
    }

    #[tokio::test]
    async fn offline_negative_no_call() {
        // C: negative case 零调用 → 非 runner failure，false_call=false
        let llm = FakeLlmClient::new(vec![text_response("该条款仅涉及付款安排，不涉及保证金。")]);
        let case = mk_case(
            "offline_neg",
            "Procedure",
            "合同签订后30日支付50%预付款。",
            Expectation::Negative,
            None,
            vec![],
        );
        let r = run_single(&case, &llm).await;
        assert!(r.tool_calls.is_empty());
        assert!(!r.false_call, "负例零调用不算 false call");
        assert!(r.final_response.is_some());
        assert!(!r.adoption_evaluable);
    }

    #[tokio::test]
    async fn offline_malformed_args() {
        // D: 非 object args → argument_json_valid=false
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", serde_json::Value::Null)]),
            text_response("无法校验。"),
        ]);
        let r = run_single(&deposit_case("投标保证金30万。"), &llm).await;
        assert!(!r.tool_calls[0].argument_json_valid, "非 object args 应判 JSON invalid");
        assert!(!r.tool_calls[0].schema_valid);
    }

    #[tokio::test]
    async fn offline_tool_error() {
        // E: 非法公式字符 → validate_calculation 真实 execute 返回 Err → execution_status=error
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc(
                "validate_calculation",
                serde_json::json!({
                    "formula": "a + @",
                    "values": {"a": 1.0}
                }),
            )]),
            text_response("公式解析失败。"),
        ]);
        let case = mk_case(
            "offline_err",
            "Procedure",
            "重算数值。",
            Expectation::Required,
            Some("validate_calculation"),
            vec![],
        );
        let r = run_single(&case, &llm).await;
        assert_eq!(
            r.tool_calls[0].execution_status, "error",
            "非法公式字符应导致 Tool.execute Err"
        );
        assert!(!r.execution_success);
    }

    #[tokio::test]
    async fn offline_result_contradicted() {
        // G: final response 与 tool 结论矛盾 → adopted=false, contradicted
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("投标保证金比例严重超标，构成违规。"),
        ]);
        let r = run_single(&deposit_case("投标保证金30万。"), &llm).await;
        assert!(r.adoption_evaluable);
        assert!(!r.result_adopted, "反转 Tool 结论 → 不算 adopted");
        assert_eq!(r.tool_result_consistency, "contradicted");
    }

    // ── Ground Truth 防泄漏 ───────────────────────────────────────

    #[tokio::test]
    async fn ground_truth_not_injected_into_messages() {
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc(
                "verify_announcement_period",
                serde_json::json!({
                    "procurement_method": "公开招标",
                    "period_type": "notice_publication",
                    "procurement_object": "goods",
                    "is_government_procurement": true,
                    "notice_start_date_str": "2025-03-03",
                    "notice_end_date_str": "2025-03-10"
                }),
            )]),
            text_response("公告期满足要求。"),
        ]);
        let case = mk_case(
            "leak_001",
            "Procedure",
            "招标公告于2025年3月3日发布，3月10日结束。",
            Expectation::Required,
            Some("verify_announcement_period"),
            vec![ExpectedArg::value("period_type", "notice_publication")],
        );
        let _r = run_single(&case, &llm).await;
        let received = llm.received.lock().unwrap();
        assert!(!received.is_empty());
        let all: String = received
            .iter()
            .flat_map(|msgs| msgs.iter())
            .map(|m| match m {
                ChatMessage::System { content } => content.clone(),
                ChatMessage::User { content } => content.clone(),
                ChatMessage::Assistant { content, .. } => content.clone().unwrap_or_default(),
                ChatMessage::Tool { content, .. } => content.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        // evaluator 元数据结构绝不下发
        assert!(!all.contains("expected_tool"), "ground truth 字段泄漏进 messages");
        assert!(!all.contains("expected_key_args"), "ground truth 字段泄漏进 messages");
        assert!(!all.contains("ground_truth"), "ground truth 字段泄漏进 messages");
        // 注意：真实 Procedure system prompt 本身含工具名/参数名，属生产正常内容，非泄漏。
    }

    // ── Metrics Aggregation 真实测试（替代旧零值占位）─────────────

    #[tokio::test]
    async fn metrics_aggregation_real() {
        // 3 cases: 1 Required correct / 1 Required wrong / 1 Negative no-call
        let mut results = Vec::new();
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("保证金合规。"),
            tc_response(vec![tc("validate_scoring_formula", serde_json::json!({"price_weight": 70.0}))]),
            text_response("完成。"),
            text_response("纯信息陈述。"),
        ]);
        results.push(run_single(&deposit_case("保证金30万。"), &llm).await);
        let wrong_case = mk_case(
            "offline_002",
            "Procedure",
            "保证金30万。",
            Expectation::Required,
            Some("verify_bid_deposit"),
            vec![ExpectedArg::presence("deposit_type")],
        );
        results.push(run_single(&wrong_case, &llm).await);
        let neg_case = mk_case(
            "offline_003",
            "Procedure",
            "纯信息陈述。",
            Expectation::Negative,
            None,
            vec![],
        );
        results.push(run_single(&neg_case, &llm).await);

        let s = aggregate(&results);
        assert_eq!(s.required_recall.numerator, 1);
        assert_eq!(s.required_recall.denominator, 2);
        assert_eq!(s.wrong_tool_rate.numerator, 1);
        assert_eq!(s.wrong_tool_rate.denominator, 2);
        assert_eq!(s.false_call_rate.numerator, 0);
        assert_eq!(s.false_call_rate.denominator, 1);
        // 2 次 tool calls（correct 1 + wrong 1），correct 1 → precision 1/2
        assert_eq!(s.tool_precision.numerator, 1);
        assert_eq!(s.tool_precision.denominator, 2);
    }

    // ── Mutation / Sabotage 测试（M1-M4）─────────────────────────

    #[tokio::test]
    async fn sabotage_m1_wrong_tool_reduces_recall() {
        let cfg = RunConfig::production_default();
        let reg = build_eval_registry();
        let case = deposit_case("保证金30万。");

        // 正确工具 → recall 1/1
        let llm_ok = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("保证金合规。"),
        ]);
        let r_ok = run_case("m1_ok", &cfg, &case, &llm_ok, &reg).await.unwrap();

        // 换成 wrong tool → recall 0/1
        let llm_bad = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_announcement_period", serde_json::json!({"period_type": "notice_publication"}))]),
            text_response("完成。"),
        ]);
        let r_bad = run_case("m1_bad", &cfg, &case, &llm_bad, &reg).await.unwrap();

        let s_ok = aggregate(&[r_ok]);
        let s_bad = aggregate(&[r_bad]);
        assert_eq!(s_ok.required_recall.rate(), 1.0);
        assert_eq!(s_bad.required_recall.rate(), 0.0, "M1: 错误工具必须降低 Required Recall");
    }

    #[tokio::test]
    async fn sabotage_m2_negative_injection_increases_false_call() {
        let cfg = RunConfig::production_default();
        let reg = build_eval_registry();
        let neg = mk_case(
            "m2_neg",
            "Procedure",
            "合同付款安排。",
            Expectation::Negative,
            None,
            vec![],
        );
        let llm_clean = FakeLlmClient::new(vec![text_response("无工具需求。")]);
        let r_clean = run_case("m2_clean", &cfg, &neg, &llm_clean, &reg).await.unwrap();
        let llm_inject = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("完成。"),
        ]);
        let r_inject = run_case("m2_inject", &cfg, &neg, &llm_inject, &reg).await.unwrap();

        assert!(!r_clean.false_call, "负例零调用不得触发 false_call");
        assert_eq!(
            aggregate(&[r_clean]).false_call_rate.rate(),
            0.0,
            "负例零调用 → False Call Rate 0"
        );
        assert!(r_inject.false_call, "M2: 负例注入 tool call 必须触发 false_call");
    }

    #[tokio::test]
    async fn sabotage_m3_missing_key_reduces_arg_recall() {
        let cfg = RunConfig::production_default();
        let reg = build_eval_registry();
        let case = mk_case(
            "m3",
            "Procedure",
            "公告期检查。",
            Expectation::Required,
            Some("verify_announcement_period"),
            vec![ExpectedArg::value("period_type", "notice_publication")],
        );
        // 缺 period_type → arg recall 0
        let llm_bad = FakeLlmClient::new(vec![
            tc_response(vec![tc(
                "verify_announcement_period",
                serde_json::json!({"procurement_method": "公开招标"}),
            )]),
            text_response("完成。"),
        ]);
        let r_bad = run_case("m3_bad", &cfg, &case, &llm_bad, &reg).await.unwrap();
        assert_eq!(r_bad.argument_presence_score, 0.0, "M3: 缺 required key → presence 0");
        assert_eq!(aggregate(&[r_bad]).expected_key_arg_recall.rate(), 0.0);

        // 带 period_type → arg recall 1
        let llm_ok = FakeLlmClient::new(vec![
            tc_response(vec![tc(
                "verify_announcement_period",
                serde_json::json!({
                    "procurement_method": "公开招标",
                    "period_type": "notice_publication",
                    "procurement_object": "goods",
                    "is_government_procurement": true,
                    "notice_start_date_str": "2025-03-03",
                    "notice_end_date_str": "2025-03-10"
                }),
            )]),
            text_response("公告期合规。"),
        ]);
        let r_ok = run_case("m3_ok", &cfg, &case, &llm_ok, &reg).await.unwrap();
        assert_eq!(r_ok.argument_presence_score, 1.0);
        assert_eq!(aggregate(&[r_ok]).expected_key_arg_recall.rate(), 1.0);
    }

    #[tokio::test]
    async fn sabotage_m4_reversed_final_reduces_adoption() {
        let cfg = RunConfig::production_default();
        let reg = build_eval_registry();
        let case = deposit_case("保证金30万。");

        let llm_consistent = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("保证金合规。"),
        ]);
        let r_c = run_case("m4_c", &cfg, &case, &llm_consistent, &reg).await.unwrap();
        assert!(r_c.result_adopted);

        let llm_reversed = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("保证金严重违规超标。"),
        ]);
        let r_r = run_case("m4_r", &cfg, &case, &llm_reversed, &reg).await.unwrap();
        assert!(!r_r.result_adopted, "M4: 反转 final 结论必须降低 adoption");
        assert_eq!(r_r.tool_result_consistency, "contradicted");
    }

    // ── 结果保存 roundtrip ────────────────────────────────────────

    #[tokio::test]
    async fn save_results_roundtrip() {
        let llm = FakeLlmClient::new(vec![
            tc_response(vec![tc("verify_bid_deposit", deposit_args())]),
            text_response("保证金合规。"),
        ]);
        let case = deposit_case("保证金30万。");
        let cfg = RunConfig::production_default();
        let reg = build_eval_registry();
        let (summary, results) = run_eval(&cfg, &llm, &[case], &reg).await.unwrap();
        let dir = std::env::temp_dir().join(format!("eval_harness_test_{}", std::process::id()));
        save_results(&dir, &summary, &results).unwrap();
        let sum: RunSummary =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run_summary.json")).unwrap())
                .unwrap();
        assert_eq!(sum.case_count, 1);
        let lines = std::fs::read_to_string(dir.join("case_results.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 现有 14 条 dataset 与 harness schema 对齐 ─────────────────

    fn to_harness_case(c: &tool_selection_eval::EvalCase) -> EvalCase {
        let expectation = match c.should_call {
            tool_selection_eval::CallRequirement::Required => Expectation::Required,
            tool_selection_eval::CallRequirement::Preferred => Expectation::Preferred,
            tool_selection_eval::CallRequirement::Optional => Expectation::Preferred,
            tool_selection_eval::CallRequirement::Negative => Expectation::Negative,
        };
        let expected_key_args = c
            .expected_key_args
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    ExpectedArg::presence(k)
                } else {
                    ExpectedArg::value(k, v)
                }
            })
            .collect();
        EvalCase {
            case_id: c.case_id.to_string(),
            agent_id: c.expected_agent.to_string(),
            clause: c.clause.to_string(),
            expectation,
            expected_tool: if c.expected_tool == "NONE" {
                None
            } else {
                Some(c.expected_tool.to_string())
            },
            expected_key_args,
            forbidden_tools: vec![],
            notes: None,
        }
    }

    #[test]
    fn dataset_14_cases_alignment() {
        let raw = tool_selection_eval::all_cases();
        assert_eq!(raw.len(), 14, "数据集保持 14 条");
        let cases: Vec<EvalCase> = raw.iter().map(|c| to_harness_case(c)).collect();

        let required = cases.iter().filter(|c| c.expectation == Expectation::Required).count();
        let preferred = cases.iter().filter(|c| c.expectation == Expectation::Preferred).count();
        let negative = cases.iter().filter(|c| c.expectation == Expectation::Negative).count();
        assert_eq!((required, preferred, negative), (11, 1, 2));

        // agent_id 必须显式存在且可解析
        for c in &cases {
            assert!(agent_definition(&c.agent_id).is_ok(), "case {} agent 不可解析", c.case_id);
        }

        // proc_002a：period_type=notice_publication（公告期，非投标准备期）
        let p2a = cases.iter().find(|c| c.case_id == "proc_002a").unwrap();
        assert!(p2a.expected_key_args.iter().any(|a| a.key == "period_type" && a.value.as_deref() == Some("notice_publication")));

        // proc_005：calculate_timeline 不得携带旧 constraints/min_days/legal_basis 期待
        let p5 = cases.iter().find(|c| c.case_id == "proc_005").unwrap();
        assert_eq!(p5.expected_tool.as_deref(), Some("calculate_timeline"));
        for a in &p5.expected_key_args {
            assert!(
                a.key != "constraints" && a.key != "min_days" && a.key != "legal_basis",
                "proc_005 不得期待旧字段: {}",
                a.key
            );
        }

        // Negative case expected_tool 必须为 None
        for c in cases.iter().filter(|c| c.expectation == Expectation::Negative) {
            assert!(c.expected_tool.is_none(), "negative case {} 不得有 expected_tool", c.case_id);
        }

        // 已知关键词泄漏风险（Phase E2 处理，不在本轮改 dataset）
        let leaky = cases
            .iter()
            .filter(|c| {
                (c.case_id == "proc_001" && c.clause.contains("投标保证金"))
                    || (c.case_id == "score_002" && c.clause.contains("权重分配"))
            })
            .count();
        assert_eq!(leaky, 2, "已知泄漏风险 case: proc_001 / score_002 (EVAL_DATASET_LEAKAGE_RISK)");

        // production_smoke_cases 与 eval_test 数据集 case_id 集合必须一致（防双源漂移）
        let smoke: Vec<String> = production_smoke_cases()
            .iter()
            .map(|c| c.case_id.clone())
            .collect();
        let legacy: Vec<String> = raw.iter().map(|c| c.case_id.to_string()).collect();
        assert_eq!(smoke.len(), legacy.len(), "smoke 数据集条数应与 eval_test 一致");
        for id in &legacy {
            assert!(smoke.contains(id), "smoke 数据集缺少 case {}", id);
        }
        for id in &smoke {
            assert!(legacy.contains(id), "smoke 数据集多出未知 case {}", id);
        }
    }
}
