//! Multi-Agent 合规审查框架 — 核心类型
//!
//! 本模块定义了 Agent 框架中所有共享数据类型：
//! - [`RiskTier`] — 条款风险分级 (L1/L2/L3)
//! - [`ReviewClause`] — Coordinator 路由给 Agent 的审查单元
//! - [`RiskFinding`] — Agent output_finding 工具输出的结构化风险发现
//! - [`AgentConfig`] — Agent 配置（名称、system prompt、max_turns）
//! - [`AgentId`] — Agent 身份枚举 (8 种 Agent + BlindSpot)
//! - [`AgentDefinition`] — Agent 静态定义 (Strategy 模式)
//! - [`SessionGraph`] 相关类型 — ChunkNode, RiskNode, LinkedChunk, ClauseContext, GraphSnapshot
//! - [`TextSelection`] / [`ChatResponse`] / [`BlockRef`] / [`KnowledgeRef`] — ChatAgent 类型
//! - [`ChatAgentConfig`] — ChatAgent 运行时配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

// ─── 风险分级 ──────────────────────────────────────────────────

/// 条款风险分级，决定 max_turns 和路由策略。
///
/// 分级通过关键词扫描实现（零 LLM 成本），在 Coordinator 路由前完成。
/// 审查过程中支持动态升降级（turn 2 检测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
pub enum RiskTier {
    /// L1：低风险，纯信息/格式条款。max_turns=5，仅 FactCheckAgent。
    #[serde(rename = "L1")]
    Low,
    /// L2：中等风险，标准审查。max_turns=8，按路由矩阵分配 Agent。
    #[serde(rename = "L2")]
    #[default]
    Medium,
    /// L3：高风险，含品牌/地域/排他性关键词。max_turns=14，深度 ReAct。
    #[serde(rename = "L3")]
    High,
}

/// 从 `AIBID_TIER_MAX_TURNS` 解析各档轮次上限（"low:N,medium:N,high:N"）。
/// 任一档被设置则整体生效，未设置档用内置默认；未设置或格式错误返回 None。
fn tier_max_turns_from_env() -> Option<(usize, usize, usize)> {
    let raw = std::env::var("AIBID_TIER_MAX_TURNS").ok()?;
    let mut caps = (5usize, 8usize, 14usize);
    let mut any = false;
    for part in raw.split(',') {
        let (key, value) = part.split_once(':')?;
        let n: usize = value.trim().parse().ok()?;
        match key.trim() {
            "low" => {
                caps.0 = n;
                any = true;
            }
            "medium" => {
                caps.1 = n;
                any = true;
            }
            "high" => {
                caps.2 = n;
                any = true;
            }
            _ => {}
        }
    }
    if any {
        Some(caps)
    } else {
        None
    }
}

impl RiskTier {
    /// 返回该级别的默认 max_turns。
    ///
    /// 可用环境变量 `AIBID_TIER_MAX_TURNS="low:N,medium:N,high:N"` 覆盖各档上限，
    /// 用于「降轮次」实验 A/B 对比；未设置时用内置默认 5/8/14。
    pub fn max_turns(&self) -> usize {
        if let Some((low, medium, high)) = tier_max_turns_from_env() {
            return match self {
                RiskTier::Low => low,
                RiskTier::Medium => medium,
                RiskTier::High => high,
            };
        }
        match self {
            RiskTier::Low => 5,
            RiskTier::Medium => 8,
            RiskTier::High => 14,
        }
    }

    /// 从条款文本的关键词扫描自动分级。
    ///
    /// L3 触发词：品牌、型号、指定、专利、原厂、本地、地域、排他、唯一等
    /// L1 触发词：格式、装订、密封、签字、盖章等
    /// 其余 → L2
    pub fn from_clause_text(text: &str) -> Self {
        let text_lower = text.to_lowercase();

        let l3_keywords = [
            "品牌",
            "型号",
            "指定",
            "必须采用",
            "原厂",
            "专利",
            "专有技术",
            "本地",
            "东莞",
            "深圳",
            "本市",
            "所在地",
            "分支机构",
            "常驻",
            "唯一",
            "独家",
            "排他",
            "不接受替代",
            "原厂商授权",
            "制造商授权函",
            "项目授权",
            "★",
        ];
        let l1_keywords = [
            "格式",
            "装订",
            "密封",
            "签字",
            "盖章",
            "份数",
            "封面",
            "目录",
            "页码",
            "字体",
            "字号",
            "行距",
            // 采购元数据/文件头（纯标识信息，无实质性要求）
            "采购计划编号",
            "采购项目编号",
            "竞争性磋商文件",
            "磋商邀请",
            "投标邀请函",
        ];

        for kw in &l3_keywords {
            if text_lower.contains(&kw.to_lowercase()) {
                return RiskTier::High;
            }
        }
        for kw in &l1_keywords {
            if text_lower.contains(&kw.to_lowercase()) {
                return RiskTier::Low;
            }
        }
        RiskTier::Medium
    }
}

impl std::fmt::Display for RiskTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskTier::Low => write!(f, "L1"),
            RiskTier::Medium => write!(f, "L2"),
            RiskTier::High => write!(f, "L3"),
        }
    }
}

// ─── 审查条款 ──────────────────────────────────────────────────

/// Coordinator 路由给 Agent 的最小审查单元。
///
/// 一条 ReviewClause 对应一个 Chunk。Agent 在 ReAct 循环中审查它，
/// 可输出零到多条 RiskFinding。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewClause {
    /// Chunk ID（如 "ch_042"），对应原始 Chunk.chunk_id
    pub chunk_id: String,
    /// 从根章节到当前节点的标题链（与 Chunk.section_path 一致）
    pub section_path: Vec<String>,
    /// 条款完整文本（含标题行）
    pub text: String,
    /// 起始页码 (0-based)
    pub page_start: usize,
    /// 结束页码 (0-based)
    pub page_end: usize,
    /// 风险分级（关键词扫描）
    pub tier: RiskTier,
    /// 该级别的最大 ReAct 轮次
    pub tier_max_turns: usize,
    /// 来源 block ID（透传 Chunk.source_block_ids，用于流式阶段补发 block_ids）
    pub source_block_ids: Vec<String>,
}

impl ReviewClause {
    /// 从 Chunk 构建 ReviewClause，自动执行关键词扫描分级。
    pub fn from_chunk(
        chunk: &crate::domain::chunk::Chunk,
        embed_ctx_depth: usize,
        embed_path_max_len: usize,
    ) -> Self {
        let text = chunk.embed_text(embed_ctx_depth, embed_path_max_len);
        let tier = RiskTier::from_clause_text(&text);
        let tier_max_turns = tier.max_turns();
        Self {
            chunk_id: chunk.chunk_id.clone(),
            section_path: chunk.section_path.clone(),
            text,
            page_start: chunk.page_start,
            page_end: chunk.page_end,
            tier,
            tier_max_turns,
            source_block_ids: chunk.source_block_ids.clone(),
        }
    }

    /// 计算有效 max_turns。
    ///
    /// Agent 默认值作为天花板，tier 建议值在 agent 能力范围内调节。
    /// 例如 FactCheckAgent(default=4) 审查 L3 条款 → min(12, 4) = 4 轮；
    /// 后续 ProcedureAgent(default=8) 审查同一条款 → min(12, 8) = 8 轮。
    /// 这样不同 Agent 的轮数上限由自身职责决定，不被 tier 无限制膨胀。
    pub fn effective_max_turns(&self, agent_default: usize) -> usize {
        self.tier_max_turns.min(agent_default)
    }
}

// ─── 风险严重程度 ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
pub enum RiskSeverity {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}

impl std::fmt::Display for RiskSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskSeverity::High => write!(f, "🔴 high"),
            RiskSeverity::Medium => write!(f, "🟡 medium"),
            RiskSeverity::Low => write!(f, "🟢 low"),
            RiskSeverity::Info => write!(f, "ℹ️ info"),
        }
    }
}

impl RiskSeverity {
    /// 返回不含 emoji 的纯字符串表示，用于 SSE 事件与 Java 侧映射。
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskSeverity::High => "high",
            RiskSeverity::Medium => "medium",
            RiskSeverity::Low => "low",
            RiskSeverity::Info => "info",
        }
    }
}

/// 从 `clause_ids` 按顺序聚合各条款的 `source_block_ids`（去重、保序、上限防爆）。
///
/// 用于流式 `finding_added` 阶段补发 block_ids——LLM 输出的是 clause_ids，
/// block_ids 由框架从 clause.source_block_ids 确定性聚合，无需等待 /result
/// 的 source_quote 反查。配合「块序回退」策略，保证正确页面上能画出 bbox。
pub fn collect_block_ids_for_clause_ids(
    clause_ids: &[String],
    clause_blocks: &HashMap<String, Vec<String>>,
    max_blocks: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for cid in clause_ids {
        if out.len() >= max_blocks {
            break;
        }
        if let Some(ids) = clause_blocks.get(cid) {
            for id in ids {
                if out.len() >= max_blocks {
                    break;
                }
                if !out.contains(id) {
                    out.push(id.clone());
                }
            }
        }
    }
    out
}

// ─── 风险发现 ──────────────────────────────────────────────────

/// Agent output_finding 工具输出的结构化风险发现。
///
/// 设计文档 §6.4 定义的完整输出格式。每个字段都有明确的用途。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RiskFinding {
    /// 风险唯一标识，格式 "R_XXX"
    /// 框架会自动填充，LLM 无需输出此字段。
    #[serde(default)]
    pub risk_id: String,
    /// 关联的条款 chunk_id 列表（支持跨条款组合风险）
    /// 框架会自动填充，LLM 无需输出此字段。
    #[serde(default)]
    pub clause_ids: Vec<String>,
    /// 关联的原始 block_id 列表（从 chunk.source_block_ids 框架自动填充）。
    /// 前端用于 bbox-based PDF 精确高亮。
    #[serde(default)]
    pub block_ids: Vec<String>,
    /// 输出此发现的 Agent 名称
    /// 框架会自动填充，LLM 无需输出此字段。
    #[serde(default)]
    pub agent: String,

    // ── 核心判定 ──
    /// 是否判定为无风险（true = 该条款合规）
    pub no_risk: bool,
    /// 风险严重程度
    pub severity: RiskSeverity,
    /// 是否属于必须优先处置的重大/红线问题。
    ///
    /// 该字段与四级 severity 正交：重大问题仍使用 severity=high，
    /// Critical Recall 通过此标志计算，避免引入第五套严重度枚举。
    #[serde(default)]
    pub is_critical: bool,
    /// 重大问题判定依据；is_critical=false 时应为空字符串。
    #[serde(default)]
    pub critical_reason: String,
    /// 风险类型标签（"地域歧视" / "品牌指定" / "程序违规" / …）
    pub risk_type: String,
    /// 稳定风险分类编码，用于同一条款内的多问题去重。
    ///
    /// 推荐使用大写英文编码（如 LOCAL_REGISTRATION / BRAND_LOCK）。
    /// 旧响应未提供时保持空字符串，框架回退到 risk_type。
    #[serde(default)]
    pub category_code: String,

    // ── 证据 ──
    /// 从原文逐字摘录的违规文本（来自 read_section 返回的原始文本）
    pub source_quote: String,
    /// 法条引用列表（如 "《政府采购法》第5条"）
    pub legal_basis: Vec<String>,
    /// 案例引用 ID 列表（如 "case_001"）
    #[serde(default)]
    pub case_refs: Vec<String>,

    // ── 推理 ──
    /// 完整推理链（读了什么 → 搜了什么 → 为什么这样判定）
    pub reason: String,
    /// 修改建议
    pub suggestion: String,

    // ── 置信度 ──
    /// 整体置信度 [0.0, 1.0]
    pub confidence: f32,

    // ── 分级追踪 ──
    /// 审查开始时的初始分级（框架填充，LLM 无需输出）
    #[serde(rename = "_initial_tier", default)]
    pub initial_tier: RiskTier,
    /// 审查结束时的最终分级（框架填充，LLM 无需输出）
    #[serde(rename = "_final_tier", default)]
    pub final_tier: RiskTier,
    /// 是否发生过动态升级 (L1/L2 → L3)（框架填充，LLM 无需输出）
    #[serde(rename = "_tier_escalated", default)]
    pub tier_escalated: bool,
    /// 是否因 max_turns 耗尽而截断输出（框架填充，LLM 无需输出）
    #[serde(rename = "_truncated", default)]
    pub truncated: bool,
    /// BlindSpot 建议生成的新 Agent（Phase E 动态 Agent 生成器）
    #[serde(default)]
    pub suggested_agent: Option<SuggestedAgent>,
    /// 搜索来源引用列表（框架从 search_cache 自动填充，LLM 无需输出）
    /// 每条 Citation 对应一个唯一的搜索来源 URL
    #[serde(default)]
    pub citations: Vec<Citation>,

    // ── STS 架构字段 ──
    /// 此发现的角色（Scout=Hypothesis, Phase 2 Agent=Verified）
    #[serde(default)]
    pub finding_role: FindingRole,
    /// 知识来源（Scout 填 "training_knowledge"，Phase 2 填 "search_verified"）
    #[serde(default)]
    pub knowledge_source: String,
    /// Scout 标记的待验证法规列表（引导 Phase 2 Agent 搜索）
    #[serde(default)]
    pub verification_required: Vec<String>,
    /// 哪些 Agent 参与了初筛（Hypothesis 来源）
    #[serde(default)]
    pub hypothesized_by: Vec<String>,
    /// 哪些 Agent 验证了此发现（Verified 来源）
    #[serde(default)]
    pub verified_by: Vec<String>,
    /// 证据核验器结论（support/refute/insufficient），EvidenceVerifier 阶段回写
    #[serde(default)]
    pub evidence_verdict: Option<String>,
    /// 证据核验器的理由
    #[serde(default)]
    pub verifier_reason: Option<String>,

    // ── 框架自动填充的定位字段（用于 Java 侧映射 AuditIssueEntity） ──
    /// 起始页码 (0-based)，框架从关联 ReviewClause 自动填充
    #[serde(default)]
    pub page_number: Option<usize>,
    /// 章节路径（从根到当前节点的标题链），框架自动填充
    #[serde(default)]
    pub section_path: Option<Vec<String>>,
    /// 条款原文上下文（截取前 500 字符），框架自动填充
    #[serde(default)]
    pub context: Option<String>,
    /// 词级精确高亮矩形（按 source_quote 命中的词逐行合并）。
    /// 非空时前端优先渲染这些紧致框，跳过段落级 block 高亮与文本层收敛。
    /// 框架在审核完成阶段自动填充。
    #[serde(default)]
    pub highlight_rects: Vec<HighlightRect>,
}

/// 词级精确高亮矩形。
///
/// 坐标与 `RawBlock.bbox` / `/blocks` 端点的 `BBoxDto` 一致：PDF points，
/// 原点在页面左上角，Y 轴向下。`page` 为 0-based 页码。`page_width` 为
/// 该页原生宽度 (pt)，供前端计算 scale = renderedWidth / pageWidth。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HighlightRect {
    /// 所在页码 (0-based)
    pub page: usize,
    /// 矩形左上角 X（PDF points）
    pub x0: f64,
    /// 矩形上边界 Y（距页面顶部距离）
    pub top: f64,
    /// 矩形右下角 X
    pub x1: f64,
    /// 矩形下边界 Y
    pub bottom: f64,
    /// 原始 PDF 页面宽度 (pt)
    pub page_width: f64,
}

// ─── 发现角色 ──────────────────────────────────────────────────

/// 发现的角色：区分初筛假设和已验证结论。
///
/// Scout 产出 Hypothesis（待验证假设，不进入最终 findings），
/// Phase 2 专业 Agent 产出 Verified（已验证结论，进入最终输出）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, ToSchema)]
pub enum FindingRole {
    /// Scout 产出: 待验证假设（不进最终 findings, 不参与 LegalVerify/Debate/Triage）
    Hypothesis,
    /// 专业 Agent 产出: 已验证结论
    #[default]
    Verified,
}

/// 搜索来源引用 — 前端渲染推理过程时，可用此字段将法条/案例文本
/// 转为可点击的超链接。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Citation {
    /// 来源标题（如 "《政府采购法》第5条"）
    pub title: String,
    /// 来源 URL
    pub url: String,
    /// 来源站点名（如 "pkulaw.com"），可选
    #[serde(default)]
    pub site_name: String,
}

impl RiskFinding {
    /// 统一四级严重度与重大问题标志的约束。
    ///
    /// - 无风险结论不能标记为重大问题；
    /// - 重大问题在四级严重度中必须是 high；
    /// - 非重大问题不保留 critical_reason，避免下游误读。
    pub fn normalize_criticality(&mut self) {
        if self.no_risk {
            self.is_critical = false;
            self.critical_reason.clear();
            return;
        }
        if self.is_critical {
            self.severity = RiskSeverity::High;
            if self.critical_reason.trim().is_empty() {
                self.critical_reason =
                    format!("{}属于重大/红线问题，需优先人工复核。", self.risk_type);
            }
        } else {
            self.critical_reason.clear();
        }
    }

    /// 下游验证或辩论将问题降级时，清除重大标志及其依据。
    pub fn clear_criticality(&mut self) {
        self.is_critical = false;
        self.critical_reason.clear();
    }

    /// 创建一个 "无风险" 的快捷构造。
    pub fn no_risk_finding(
        risk_id: String,
        clause_id: String,
        agent: &str,
        initial_tier: RiskTier,
        final_tier: RiskTier,
    ) -> Self {
        Self {
            risk_id,
            clause_ids: vec![clause_id],
            block_ids: Vec::new(),
            highlight_rects: Vec::new(),
            agent: agent.to_string(),
            no_risk: true,
            severity: RiskSeverity::Info,
            is_critical: false,
            critical_reason: String::new(),
            risk_type: "无风险".to_string(),
            category_code: String::new(),
            source_quote: String::new(),
            legal_basis: Vec::new(),
            case_refs: Vec::new(),
            reason: "经审查，该条款未发现合规风险。".to_string(),
            suggestion: String::new(),
            confidence: 0.95,
            initial_tier,
            final_tier,
            tier_escalated: false,
            truncated: false,
            suggested_agent: None,
            citations: Vec::new(),
            finding_role: FindingRole::default(),
            knowledge_source: String::new(),
            verification_required: Vec::new(),
            hypothesized_by: Vec::new(),
            verified_by: Vec::new(),
            evidence_verdict: None,
            verifier_reason: None,
            page_number: None,
            section_path: None,
            context: None,
        }
    }

    /// 创建一个因 max_turns 耗尽而强制输出的 truncated finding。
    pub fn truncated_finding(
        risk_id: String,
        clause_id: String,
        agent: &str,
        initial_tier: RiskTier,
        final_tier: RiskTier,
        conversation_summary: &str,
    ) -> Self {
        Self {
            risk_id,
            clause_ids: vec![clause_id],
            block_ids: Vec::new(),
            highlight_rects: Vec::new(),
            agent: agent.to_string(),
            no_risk: true,
            severity: RiskSeverity::Info,
            is_critical: false,
            critical_reason: String::new(),
            risk_type: "审查截断".to_string(),
            category_code: String::new(),
            source_quote: String::new(),
            legal_basis: Vec::new(),
            case_refs: Vec::new(),
            reason: format!(
                "max_turns 耗尽，审查不完整。已完成的审查步骤：{}",
                conversation_summary
            ),
            suggestion: "建议人工复核此条款。".to_string(),
            confidence: 0.3,
            initial_tier,
            final_tier,
            tier_escalated: false,
            truncated: true,
            suggested_agent: None,
            citations: Vec::new(),
            finding_role: FindingRole::default(),
            knowledge_source: String::new(),
            verification_required: Vec::new(),
            hypothesized_by: Vec::new(),
            verified_by: Vec::new(),
            evidence_verdict: None,
            verifier_reason: None,
            page_number: None,
            section_path: None,
            context: None,
        }
    }
}

// ─── BlindSpot 动态 Agent 建议 ─────────────────────────────────

/// BlindSpot Agent 在发现遗漏风险后，可建议生成一个新的审查 Agent。
///
/// Phase E 动态 Agent 生成器的核心数据类型。Coordinator 在 TRIAGE 前
/// 扫描所有 findings 中的 suggested_agent，写入 dynamic_agents.json。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SuggestedAgent {
    /// 语义名，如 "品牌组合排他检测"
    pub agent_name: String,
    /// 完整的 system prompt
    pub agent_prompt: String,
    /// 路由触发词
    pub section_keywords: Vec<String>,
    /// 为什么需要这个 Agent
    pub reason: String,
}

// ─── SessionGraph 节点类型 ─────────────────────────────────────

/// SessionGraph 中的 Law（法规）节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawNode {
    /// 稳定 key，如 "政府采购法§5"
    pub law_id: String,
    /// 可读引用，如 "《政府采购法》第5条"
    pub article_no: String,
    /// 法规名称
    pub title: String,
}

/// SessionGraph 中的 Case（案例）节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseNode {
    pub case_id: String,
    pub title: String,
    pub summary: String,
}

/// SessionGraph 中的 Agent 节点（记录参与审查的 Agent 元信息）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub agent_id: AgentId,
    pub display_name: String,
    pub role: String,
}

// ─── Agent 配置 ────────────────────────────────────────────────

/// 单个 Agent 的静态配置。
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Agent 名称（如 "FactCheckAgent"）
    pub name: String,
    /// System prompt 全文（定义 Agent 的审查职责、工具使用规则、输出格式）
    pub system_prompt: String,
    /// Agent 类型的默认 max_turns（条款级 tier_max_turns 优先）
    pub default_max_turns: usize,
    /// 该 Agent 可使用的工具名称列表
    pub tool_names: Vec<String>,
}

// ─── 审查计划 ──────────────────────────────────────────────────

/// Coordinator 输出的审查计划。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPlan {
    /// 文档 UUID
    pub document_id: String,
    /// 文档各部分的路由计划
    pub parts: Vec<DocumentPart>,
}

/// 文档的一个 Part 及其路由决策。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentPart {
    /// Part 名称（如 "第一部分 投标邀请函"）
    pub part_title: String,
    /// 页码范围 (start, end)，0-based
    pub page_range: (usize, usize),
    /// 该 Part 下的审查条款列表
    pub clauses: Vec<ReviewClause>,
    /// 分配给哪些 Agent
    pub assigned_agents: Vec<String>,
    /// 跳过哪些 Agent（及原因）
    pub skip_agents: Vec<String>,
    /// 路由理由（用于审查追溯）
    pub route_reason: String,
}

// ─── 法律领域分类 ─────────────────────────────────────────────

/// 法律领域标签 — 用于 LegalVerify 批量分组。
///
/// 审查发现中的 `legal_basis` 被自动分类到对应领域，
/// 同领域的 finding 共享法规搜索上下文，批量验证。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LegalDomain {
    /// 采购程序 — 采购方式、公告、开标评标
    ProcurementProcedure,
    /// 供应商资格 — 资质、业绩、准入条件
    SupplierQualification,
    /// 地域限制 — 本地化、分支机构、常驻
    GeographicRestriction,
    /// 品牌/型号指定 — 专利、原厂、独家
    BrandDesignation,
    /// 评分/评审 — 分值、权重、评审因素
    ScoringEvaluation,
    /// 合同条款 — 付款、验收、质保、违约
    ContractTerms,
    /// 保证金/时限 — 保证金、截止日期、期限
    BidBondTimeline,
    /// 技术要求 — 参数、规格、认证、国产
    TechnicalRequirements,
    /// 无法归类（回退到逐条模式）
    Other,
}

impl LegalDomain {
    /// 从 finding 的 legal_basis + risk_type 自动推断法律领域。
    ///
    /// 纯规则匹配（零 LLM 成本）。
    /// 返回 `(主领域, 置信度)`，置信度用于决定是否走规则直通。
    pub fn classify(risk_type: &str, legal_basis: &[String]) -> (Self, f32) {
        let combined: String = risk_type.to_lowercase()
            + " "
            + &legal_basis
                .iter()
                .map(|s| s.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

        let checks: &[(LegalDomain, &[&str], f32)] = &[
            // 地域限制 — 最强信号
            (
                LegalDomain::GeographicRestriction,
                &[
                    "地域",
                    "本地",
                    "所在地",
                    "分支机构",
                    "常驻",
                    "排斥",
                    "歧视",
                    "地区",
                    "第5条",
                    "第二十条第七项",
                ],
                0.95,
            ),
            // 品牌指定
            (
                LegalDomain::BrandDesignation,
                &[
                    "品牌",
                    "型号",
                    "专利",
                    "原厂",
                    "独家",
                    "排他",
                    "指定",
                    "唯一",
                    "第二十条第二项",
                    "第二十二条",
                ],
                0.90,
            ),
            // 采购程序
            (
                LegalDomain::ProcurementProcedure,
                &[
                    "采购方式",
                    "公开招标",
                    "邀请招标",
                    "竞争性",
                    "公告",
                    "公示",
                    "开标",
                    "评标",
                    "废标",
                    "流标",
                    "第二十七条",
                    "第二十三条",
                    "第十三条",
                    "87号令",
                ],
                0.85,
            ),
            // 评分评审
            (
                LegalDomain::ScoringEvaluation,
                &[
                    "评分",
                    "分值",
                    "权重",
                    "价格分",
                    "技术分",
                    "商务分",
                    "评审因素",
                    "评审标准",
                    "第三十四条",
                    "第55条",
                    "第64条",
                ],
                0.90,
            ),
            // 合同条款
            (
                LegalDomain::ContractTerms,
                &[
                    "付款", "验收", "质保", "违约", "售后", "保修", "合同", "第43条", "第50条",
                    "第47条", "第52条",
                ],
                0.85,
            ),
            // 保证金/时限
            (
                LegalDomain::BidBondTimeline,
                &[
                    "保证金",
                    "截止",
                    "期限",
                    "工作日",
                    "日历日",
                    "第三十三条",
                    "第二十九条",
                    "第三十一条",
                    "第20条",
                ],
                0.90,
            ),
            // 供应商资格
            (
                LegalDomain::SupplierQualification,
                &[
                    "资格",
                    "资质",
                    "业绩",
                    "条件",
                    "准入",
                    "特定",
                    "第二十二条",
                    "第二十三条",
                    "第十七条",
                    "第二十条",
                ],
                0.85,
            ),
            // 技术要求
            (
                LegalDomain::TechnicalRequirements,
                &[
                    "技术",
                    "参数",
                    "规格",
                    "认证",
                    "国产",
                    "性能",
                    "功能",
                    "配置",
                    "第二十条",
                ],
                0.80,
            ),
        ];

        for (domain, keywords, confidence) in checks {
            for kw in *keywords {
                if combined.contains(&kw.to_lowercase()) {
                    return (domain.clone(), *confidence);
                }
            }
        }

        (LegalDomain::Other, 0.5)
    }

    /// 该领域是否支持规则预筛（已知法规可直接匹配，不需要 LLM）。
    pub fn supports_rule_prefilter(&self) -> bool {
        matches!(
            self,
            LegalDomain::ProcurementProcedure
                | LegalDomain::GeographicRestriction
                | LegalDomain::BidBondTimeline
                | LegalDomain::ScoringEvaluation
        )
    }
}

impl std::fmt::Display for LegalDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LegalDomain::ProcurementProcedure => write!(f, "采购程序"),
            LegalDomain::SupplierQualification => write!(f, "供应商资格"),
            LegalDomain::GeographicRestriction => write!(f, "地域限制"),
            LegalDomain::BrandDesignation => write!(f, "品牌指定"),
            LegalDomain::ScoringEvaluation => write!(f, "评分评审"),
            LegalDomain::ContractTerms => write!(f, "合同条款"),
            LegalDomain::BidBondTimeline => write!(f, "保证金/时限"),
            LegalDomain::TechnicalRequirements => write!(f, "技术要求"),
            LegalDomain::Other => write!(f, "其他"),
        }
    }
}

// ─── 批量法条验证 ─────────────────────────────────────────────

/// 批量验证中的单条结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchVerificationEntry {
    pub risk_id: String,
    /// 法条引用是否有效
    pub is_valid: bool,
    /// 修正后的法条引用（如果 is_valid=false）
    #[serde(default)]
    pub corrected_legal_basis: Vec<String>,
    /// 验证置信度 [0.0, 1.0]
    pub confidence: f32,
    /// 验证理由
    pub reason: String,
}

/// 批量验证的完整输出 — 对应 `output_verification_batch` 工具的 arguments。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchVerificationOutput {
    pub verifications: Vec<BatchVerificationEntry>,
}

// ─── Legal Verify 结果 ─────────────────────────────────────────

/// Adversarial Legal Verify 的输出（逐条模式保留兼容）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalVerifyResult {
    /// 法条引用是否成立
    pub verified: bool,
    /// 如果 verified=false，提供修正后的法条引用
    pub correction: Option<String>,
    /// 验证理由
    pub reason: String,
}

// ─── Agent 身份 ──────────────────────────────────────────────────

/// Agent 身份枚举 — 8 种审查 Agent + BlindSpot。
///
/// 用于 SessionGraph 的 reviewed_by 边、AgentBus 消息路由、
/// AgentRegistry 查找等所有需要标识 Agent 的场景。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentId {
    /// 事实核查 — 提取结构化事实，与法规阈值对照
    FactCheck,
    /// 采购程序合规 — 审查采购方式、公告期限、保证金等程序
    Procedure,
    /// 规则引擎 — 硬性规则匹配（如"必须"条款的强制执行判定）
    RuleEngine,
    /// 隐性风险 — 语义分析发现品牌指向、地域偏好、隐性排他
    SemanticRisk,
    /// 评分合规 — 评审因素权重、评分标准合规性
    Scoring,
    /// 需求合规 — 技术需求是否存在倾向性、排他性参数
    Demand,
    /// 合同合规 — 合同条款合规性审查
    Contract,
    /// 盲点复查 — 所有 Agent 完成后扫描遗漏
    BlindSpot,
    /// 对抗法条验证 — 验证法条引用是否真实、适用
    LegalVerify,
    /// 正反辩论 — 对 High + 低置信度发现做 Defender/Challenger/Arbiter 辩论
    Debate,
    /// Scout 初筛 Agent — 快速扫描全文档，产出 Hypothesis 引导 Phase 2 审查
    Scout,
    /// 动态 Agent — BlindSpot 生成的补充审查 Agent
    Dynamic(String),
}

impl AgentId {
    /// 所有非 BlindSpot 的 Agent 列表（用于默认 coordinator 配置）。
    pub fn all_reviewers() -> Vec<AgentId> {
        vec![
            AgentId::FactCheck,
            AgentId::Procedure,
            AgentId::RuleEngine,
            AgentId::SemanticRisk,
            AgentId::Scoring,
            AgentId::Demand,
            AgentId::Contract,
        ]
    }

    /// 从字符串匹配 AgentId（用于 env var 等场景）。
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "factcheck" | "fact_check" | "factcheckagent" => Some(AgentId::FactCheck),
            "procedure" | "procedureagent" => Some(AgentId::Procedure),
            "ruleengine" | "rule_engine" | "ruleengineagent" => Some(AgentId::RuleEngine),
            "semanticrisk" | "semantic_risk" | "semanticriskagent" => Some(AgentId::SemanticRisk),
            "scoring" | "scoringagent" => Some(AgentId::Scoring),
            "demand" | "demandagent" => Some(AgentId::Demand),
            "contract" | "contractagent" => Some(AgentId::Contract),
            "blindspot" | "blind_spot" | "blindspotagent" => Some(AgentId::BlindSpot),
            "legalverify" | "legal_verify" | "legalverifyagent" => Some(AgentId::LegalVerify),
            "debate" | "debateagent" => Some(AgentId::Debate),
            "scout" | "scoutagent" => Some(AgentId::Scout),
            _ => {
                // Dynamic_ 前缀识别
                if s.starts_with("dynamic_") {
                    Some(AgentId::Dynamic(s.to_string()))
                } else {
                    None
                }
            }
        }
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentId::FactCheck => write!(f, "FactCheckAgent"),
            AgentId::Procedure => write!(f, "ProcedureAgent"),
            AgentId::RuleEngine => write!(f, "RuleEngineAgent"),
            AgentId::SemanticRisk => write!(f, "SemanticRiskAgent"),
            AgentId::Scoring => write!(f, "ScoringAgent"),
            AgentId::Demand => write!(f, "DemandAgent"),
            AgentId::Contract => write!(f, "ContractAgent"),
            AgentId::BlindSpot => write!(f, "BlindSpotAgent"),
            AgentId::LegalVerify => write!(f, "LegalVerifyAgent"),
            AgentId::Debate => write!(f, "DebateAgent"),
            AgentId::Scout => write!(f, "ScoutAgent"),
            AgentId::Dynamic(name) => write!(f, "{name}"),
        }
    }
}

// ─── Agent 复杂度 ────────────────────────────────────────────────

/// Agent 复杂度分级，影响默认 max_turns 和资源分配。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentComplexity {
    /// 轻量 Agent（如 FactCheck on L1），default_max_turns ≤ 4
    Low,
    /// 标准 Agent，default_max_turns 6-8
    Medium,
    /// 深度 Agent（SemanticRisk, BlindSpot），default_max_turns ≥ 10
    High,
}

// ─── Agent 静态定义 (Strategy 模式) ───────────────────────────────

/// Agent 的静态定义 — Strategy 模式的核心。
///
/// 8 个 Agent 是同一接口 (`ReActLoop`) 的不同策略，
/// 差异 = `system_prompt` + `default_max_turns` + `section_keywords` (路由用)。
#[derive(Debug, Clone)]
pub struct AgentDefinition {
    /// Agent 身份标识
    pub id: AgentId,
    /// 显示名称（如 "事实核查Agent"）
    pub display_name: &'static str,
    /// System prompt 全文
    pub system_prompt: &'static str,
    /// 该 Agent 类型的默认 max_turns
    pub default_max_turns: usize,
    /// 复杂度分级
    pub complexity: AgentComplexity,
    /// 路由关键词：含有这些关键词的条款应分配给此 Agent
    pub section_keywords: &'static [&'static str],
    /// 可使用的工具名称列表
    pub tool_names: &'static [&'static str],
}

impl AgentDefinition {
    /// 从 AgentDefinition 构建 AgentConfig（用于 ReActLoop 构造）。
    pub fn to_agent_config(&self) -> AgentConfig {
        AgentConfig {
            name: self.id.to_string(),
            system_prompt: self.system_prompt.to_string(),
            default_max_turns: self.default_max_turns,
            tool_names: self.tool_names.iter().map(|s| s.to_string()).collect(),
        }
    }
}

// ─── Coordinator 配置 ────────────────────────────────────────────

/// Coordinator 的运行时配置。
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    /// 启用的 Agent 列表（默认全部 7 个 reviewer）
    pub enabled_agents: Vec<AgentId>,
    /// 是否启用 Legal Verify 对抗法条验证
    pub enable_legal_verify: bool,
    /// 是否启用 Evidence Verifier 证据核验（证伪导向 NLI 三分类，Triage 前）
    pub enable_evidence_verify: bool,
    /// Legal Verify 的最大 ReAct 轮次
    pub legal_verify_max_turns: usize,
    /// BlindSpot ReAct 的最大轮次
    pub blind_spot_max_turns: usize,
    /// BlindSpot ReAct 失败时是否回退到静态 fallback
    pub blind_spot_fallback_enabled: bool,
    /// 最大并行审查条款数（同一 Agent 内并行处理的条款上限）
    pub max_parallel_clauses: usize,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            enabled_agents: AgentId::all_reviewers(),
            enable_legal_verify: false, // 成本优化：关闭 LLM 法条验证
            enable_evidence_verify: true, // 证据核验：离线实验 precision 100%，默认开启
            legal_verify_max_turns: 3,
            blind_spot_max_turns: 10,
            blind_spot_fallback_enabled: true,
            max_parallel_clauses: 3,
        }
    }
}

// ─── 动态 Agent 持久化 ──────────────────────────────────────────

/// BlindSpot 生成的动态 Agent 定义（可序列化到 JSON 文件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicAgentDefinition {
    pub id: String,
    pub display_name: String,
    pub system_prompt: String,
    pub default_max_turns: usize,
    pub complexity: AgentComplexity,
    pub section_keywords: Vec<String>,
    pub tool_names: Vec<String>,
    pub created_at: String,
    pub created_by: String,
    pub reason: String,
    #[serde(default)]
    pub active: bool,
}

/// 动态 Agent 清单文件（agents/dynamic_agents.json）的根结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicAgentManifest {
    pub version: u32,
    pub agents: Vec<DynamicAgentDefinition>,
}

// ─── Coordinator 输出 ────────────────────────────────────────────

/// Coordinator 主执行阶段的最终状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewExecutionStatus {
    Completed,
    PartialFailed,
}

impl ReviewExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::PartialFailed => "partial_failed",
        }
    }
}

/// 整个 Agent 未能产出任何成功条款时的失败信息。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentExecutionFailure {
    pub agent_id: String,
    pub message: String,
}

/// 单条条款执行失败的信息。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClauseExecutionFailure {
    pub agent_id: String,
    pub clause_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StageExecutionFailure {
    pub stage: String,
    pub message: String,
}

/// Coordinator 执行完整性摘要。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExecutionSummary {
    pub status: ReviewExecutionStatus,
    pub successful_agents: usize,
    pub failed_agents: Vec<AgentExecutionFailure>,
    pub failed_clauses: Vec<ClauseExecutionFailure>,
    #[serde(default)]
    pub failed_stages: Vec<StageExecutionFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<crate::agents::execution_control::BudgetUsage>,
}

impl ExecutionSummary {
    pub fn completed(successful_agents: usize) -> Self {
        Self {
            status: ReviewExecutionStatus::Completed,
            successful_agents,
            failed_agents: Vec::new(),
            failed_clauses: Vec::new(),
            failed_stages: Vec::new(),
            budget: None,
        }
    }
}

impl Default for ExecutionSummary {
    fn default() -> Self {
        Self::completed(0)
    }
}

/// Coordinator 审查管线的最终输出。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CoordinatorOutput {
    /// 所有 Agent 的风险发现（已去重合并）
    pub findings: Vec<RiskFinding>,
    /// 路由摘要（各 Agent 分配条款数等）
    pub routing_summary: RoutingSummary,
    /// SessionGraph 快照（审计追溯用）
    pub graph_snapshot: Option<GraphSnapshot>,
    /// Agent 与条款执行是否完整，供 HTTP/SSE 区分完成与部分失败。
    #[serde(default)]
    pub execution_summary: ExecutionSummary,
}

/// Coordinator 的路由与审查统计摘要。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RoutingSummary {
    /// 审查条款总数
    pub total_clauses: usize,
    /// 各 Agent 分配的条款数
    pub agent_clause_counts: HashMap<String, usize>,
    /// 高风险发现数量
    pub high_risk_count: usize,
    /// Legal Verify 执行的验证次数
    pub legal_verify_count: usize,
    /// BlindSpot 发现的新风险数
    pub blind_spot_findings: usize,
}

// ─── SessionGraph 相关类型 ───────────────────────────────────────

/// SessionGraph 中的条款节点。
///
/// 在 Coordinator PRELOAD 阶段写入，供 Agent 查询"谁审过这条？"。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkNode {
    pub chunk_id: String,
    pub section_path: Vec<String>,
    pub page_start: usize,
    pub page_end: usize,
    /// 条款文本前 200 字符（预览用，避免在图中存储完整文本）
    pub text_preview: String,
    /// 条款风险分级（关键词扫描，L1=格式/信息，L2=标准，L3=高风险）
    pub tier: RiskTier,
}

/// SessionGraph 中的风险节点（封装 RiskFinding + 法条引用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskNode {
    pub finding: RiskFinding,
    /// 从 finding.legal_basis 提取的法条引用
    pub law_refs: Vec<String>,
}

/// linked_to 边的目标 Chunk + 关联原因。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedChunk {
    pub chunk_id: String,
    /// 关联原因（如 "共同指向品牌 X" / "资格+评分形成隐性升级"）
    pub reason: String,
}

/// SessionGraph 对某个 Chunk 的查询结果（Agent 每轮 ReAct 拉取）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClauseContext {
    pub chunk_id: String,
    /// 已审查过此条款的 Agent 列表
    pub reviewed_by: Vec<AgentId>,
    /// 已发现的关联风险
    pub risks: Vec<RiskFinding>,
    /// 与此条款关联的其他条款
    pub linked_chunks: Vec<LinkedChunk>,
    /// 引用相同法条的其他条款 chunk_id
    pub same_law_chunks: Vec<String>,
    /// 与此条款存在矛盾的其他条款
    #[serde(default)]
    pub contradictions: Vec<LinkedChunk>,
}

impl ClauseContext {
    /// 是否存在已知风险。
    pub fn has_prior_risks(&self) -> bool {
        !self.risks.is_empty()
    }

    /// 生成 "已审查 Agent 摘要" 文本（注入 conversation）。
    pub fn reviewed_by_summary(&self) -> String {
        if self.reviewed_by.is_empty() {
            return "（无）".to_string();
        }
        self.reviewed_by
            .iter()
            .map(|a| format!("- {} 已审查此条款", a))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 生成 "已知风险摘要" 文本（注入 conversation）。
    ///
    /// 区分 Hypothesis（Scout 初筛假设）和 Verified（Phase 2 已验证结论），
    /// 并展示 verification_required（引导 Phase 2 Agent 搜索）和 legal_basis。
    pub fn risk_summary(&self) -> String {
        if self.risks.is_empty() {
            return "（无）".to_string();
        }
        self.risks
            .iter()
            .map(|r| {
                let role_label = if r.finding_role == FindingRole::Hypothesis {
                    "[Scout 假设, 待验证]"
                } else {
                    "[已验证]"
                };
                let mut line = format!(
                    "- {} [{}] {} (confidence={:.2}): {}",
                    role_label, r.severity, r.risk_type, r.confidence, r.reason
                );

                // Scout Hypothesis: 展示待验证法规作为 Phase 2 Agent 的搜索起点
                if r.finding_role == FindingRole::Hypothesis && !r.verification_required.is_empty()
                {
                    line.push_str(&format!(
                        "\n  🔍 建议搜索验证: {}",
                        r.verification_required.join(", ")
                    ));
                }

                // 展示法规依据（Hypothesis 是推测，Verified 是确认）
                if !r.legal_basis.is_empty() {
                    line.push_str(&format!("\n  法规依据: {}", r.legal_basis.join(", ")));
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// SessionGraph 的完整快照（BlindSpot 审查 + 审计追溯用）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GraphSnapshot {
    /// 所有条款节点
    pub chunks: HashMap<String, ChunkNode>,
    /// 所有风险节点
    pub risks: HashMap<String, RiskNode>,
    /// has_risk 边: chunk_id → Vec<risk_id>
    pub has_risk: HashMap<String, Vec<String>>,
    /// reviewed_by 边: chunk_id → Vec<AgentId>
    pub reviewed_by: HashMap<String, Vec<AgentId>>,
    /// linked_to 边: chunk_id → Vec<LinkedChunk>
    pub linked_to: HashMap<String, Vec<LinkedChunk>>,
    /// cites 边: risk_id → Vec<law_ref>
    pub cites: HashMap<String, Vec<String>>,
    /// cited_by 反向索引: law_ref → Vec<risk_id>
    pub cited_by: HashMap<String, Vec<String>>,
    /// Agent 节点: agent_id → AgentNode
    pub agents: HashMap<AgentId, AgentNode>,
    /// Law 节点: law_id → LawNode
    pub laws: HashMap<String, LawNode>,
    /// Case 节点: case_id → CaseNode
    pub cases: HashMap<String, CaseNode>,
    /// contradicts 边: chunk_id → Vec<(other_chunk_id, reason)>
    pub contradicts: HashMap<String, Vec<(String, String)>>,
    /// same_law 物化边: chunk_id → Vec<other_chunk_id>
    pub same_law: HashMap<String, Vec<String>>,
}

impl GraphSnapshot {
    /// 创建空的快照。
    pub fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            risks: HashMap::new(),
            has_risk: HashMap::new(),
            reviewed_by: HashMap::new(),
            linked_to: HashMap::new(),
            cites: HashMap::new(),
            cited_by: HashMap::new(),
            agents: HashMap::new(),
            laws: HashMap::new(),
            cases: HashMap::new(),
            contradicts: HashMap::new(),
            same_law: HashMap::new(),
        }
    }
}

impl Default for GraphSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

// ─── ChatAgent 类型 ──────────────────────────────────────────────

/// 用户在 PDF 上划词选中的文本。可为 None（纯对话模式）。
///
/// 对标 AI 编程工具的"划词 + 提问"交互。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TextSelection {
    /// 选中的原文
    pub text: String,
    /// 选中文本对应的 block_id 列表
    pub block_ids: Vec<String>,
    /// 起始页码 (0-based)
    pub page: usize,
    /// 选区包围盒坐标（前端高亮定位用，复用 RawDocument 的 BBox）
    pub bbox: Option<BBox>,
}

/// 选区包围盒（简化版，用于 ChatAgent 类型。完整版见 domain::raw_document::BBox）
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BBox {
    pub x0: f64,
    pub top: f64,
    pub x1: f64,
    pub bottom: f64,
}

/// ChatAgent 的返回结构。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChatResponse {
    /// 自然语言回答（含 [b_xxx] 标记 → 前端渲染链接）
    pub answer: String,
    /// 推理链（按 ReAct turn 顺序，每条为 LLM 在该轮的 thought）
    pub reasoning: Vec<String>,
    /// 原文引用（前端按 block_id 高亮 PDF）
    pub references: Vec<BlockRef>,
    /// 法规/案例引用
    pub knowledge_refs: Vec<KnowledgeRef>,
    /// 置信度（仅合规判断时）
    pub confidence: Option<f32>,
    /// 建议下一步操作
    pub suggested_actions: Vec<String>,
}

/// 原文引用 — 前端用 block_id 查询 bbox 渲染 PDF 高亮。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BlockRef {
    /// 前端查询 bbox 渲染高亮
    pub block_id: String,
    /// 精确引用的文字片段
    pub quote: String,
    /// 上下文（200 字）
    pub snippet: String,
    /// 所在页码 (0-based)
    pub page: usize,
}

/// 外部知识引用。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeRef {
    /// "law" | "case" | "negative_list"
    pub ref_type: String,
    /// "《政府采购法实施条例》第20条"
    pub title: String,
    /// 摘录
    pub excerpt: String,
    /// 来源 URL
    pub source_url: Option<String>,
}

/// ChatAgent 运行时配置。
#[derive(Debug, Clone)]
pub struct ChatAgentConfig {
    /// 最大 ReAct 轮次，默认 12（对话常需多轮搜索）
    pub max_turns: usize,
    /// 对话压缩阈值（tokens），默认 150_000。★ 实现推迟到 Phase 3
    pub compaction_threshold: usize,
    /// 压缩后保留最近轮次，默认 5。★ 实现推迟到 Phase 3
    pub compaction_keep_recent: usize,
    /// 用户偏好文件路径
    pub preferences_path: String,
    /// 项目配置文件路径
    pub project_config_path: String,
}

impl Default for ChatAgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 12,
            compaction_threshold: 150_000,
            compaction_keep_recent: 5,
            preferences_path: ".ai-bid/user-preferences.md".to_string(),
            project_config_path: ".ai-bid/review-config.md".to_string(),
        }
    }
}

/// ChatAgent 调用的性能 profile（`_profile` 前缀，不暴露给 LLM）。
#[derive(Debug, Clone, Serialize)]
pub struct ChatProfile {
    /// 实际执行轮次
    pub total_turns: usize,
    /// 总耗时 (ms)
    pub total_duration_ms: u64,
    /// 调用的工具列表
    pub tool_calls: Vec<String>,
    /// web_search 次数
    pub search_count: usize,
    /// search_document 次数
    pub document_search_count: usize,
    /// read_section 次数
    pub read_section_count: usize,
}

// ─── ChatAgent 流式事件 ─────────────────────────────────────────

/// SSE streaming events emitted by [`ChatAgent::chat_stream()`].
///
/// Sent over an mpsc channel from the agent to the HTTP handler,
/// which maps each variant to an SSE event with matching event type
/// and inner data payload.
#[derive(Debug, Clone)]
pub enum ChatStreamEvent {
    /// Agent starts processing or encounters a milestone.
    Thinking { message: String },

    /// Agent is about to execute a tool call.
    ToolCall { name: String, args: String },

    /// Agent produced the final answer.
    Answer(ChatResponse),

    /// Stream complete — contains the final ChatResponse.
    Done(ChatResponse),

    /// An error occurred during streaming.
    Error(String),
}

// ─── 预搜索结果缓存 ──────────────────────────────────────────────

/// 预搜索结果条目，存储于 SessionGraph.search_results。
///
/// Coordinator 批量搜索阶段产出的结构化缓存，
/// 在 Execute Phase 注入 Agent prompt 供直接引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCacheEntry {
    /// 搜索查询词
    pub query: String,
    /// 搜索类别（"法规" / "案例" / "负面清单"）
    pub category: String,
    /// AI 生成的搜索摘要
    pub answer: String,
    /// 来源 URL 列表
    pub sources: Vec<Citation>,
}

// ─── 测试 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RiskTier 分级逻辑 ──────────────────────────────────────

    #[test]
    fn test_risk_tier_l3_keywords() {
        // L3 高风险触发词
        let l3_cases = [
            "本项目指定采用华为品牌设备",
            "投标人须在东莞地区设有常驻服务机构",
            "须取得原厂商针对本项目的唯一授权函",
            "★ 核心产品不接受替代方案",
            "本项目指定品牌为某厂商",
            "须提供制造商授权函原件",
            "不接受替代方案的专利技术",
            "本市注册企业优先",
        ];
        for case in &l3_cases {
            assert_eq!(
                RiskTier::from_clause_text(case),
                RiskTier::High,
                "应返回 L3 但未返回: {}",
                case
            );
        }
    }

    #[test]
    fn test_risk_tier_l1_keywords() {
        let l1_cases = [
            "投标文件封面格式要求见附件一",
            "响应文件须密封并在封口处加盖公章",
            "竞争性磋商文件 采购项目编号: ZC2024-001",
            "投标邀请函 致各潜在供应商",
            "文件份数要求：正本一份副本三份",
            "字体字号：正文采用小四号宋体",
        ];
        for case in &l1_cases {
            assert_eq!(
                RiskTier::from_clause_text(case),
                RiskTier::Low,
                "应返回 L1 但未返回: {}",
                case
            );
        }
    }

    #[test]
    fn test_risk_tier_l2_default() {
        let l2_cases = [
            "供应商须具备依法缴纳税收和社会保障资金的良好记录",
            "项目工期为合同签订后60个日历日内完成",
            "验收标准按照国家和行业相关规范执行",
        ];
        for case in &l2_cases {
            assert_eq!(
                RiskTier::from_clause_text(case),
                RiskTier::Medium,
                "应返回 L2 (默认) 但未返回: {}",
                case
            );
        }
    }

    #[test]
    fn test_risk_tier_l3_priority_over_l1() {
        // 同时含 L1 和 L3 关键词 → L3 优先
        assert_eq!(
            RiskTier::from_clause_text("本项目指定品牌为华为，封面格式见附件"),
            RiskTier::High,
            "L3 应优先于 L1"
        );
    }

    #[test]
    fn test_risk_tier_max_turns() {
        assert_eq!(RiskTier::Low.max_turns(), 5);
        assert_eq!(RiskTier::Medium.max_turns(), 8);
        assert_eq!(RiskTier::High.max_turns(), 14);
    }

    #[test]
    fn test_risk_tier_display() {
        assert_eq!(RiskTier::Low.to_string(), "L1");
        assert_eq!(RiskTier::Medium.to_string(), "L2");
        assert_eq!(RiskTier::High.to_string(), "L3");
    }

    #[test]
    fn test_risk_tier_default_is_medium() {
        assert_eq!(RiskTier::default(), RiskTier::Medium);
    }

    // ── AgentId 完备性 ────────────────────────────────────────

    #[test]
    fn test_agent_id_display_all_variants() {
        assert_eq!(AgentId::FactCheck.to_string(), "FactCheckAgent");
        assert_eq!(AgentId::Procedure.to_string(), "ProcedureAgent");
        assert_eq!(AgentId::RuleEngine.to_string(), "RuleEngineAgent");
        assert_eq!(AgentId::SemanticRisk.to_string(), "SemanticRiskAgent");
        assert_eq!(AgentId::Scoring.to_string(), "ScoringAgent");
        assert_eq!(AgentId::Demand.to_string(), "DemandAgent");
        assert_eq!(AgentId::Contract.to_string(), "ContractAgent");
        assert_eq!(AgentId::BlindSpot.to_string(), "BlindSpotAgent");
        assert_eq!(AgentId::LegalVerify.to_string(), "LegalVerifyAgent");
        assert_eq!(AgentId::Debate.to_string(), "DebateAgent");
    }

    #[test]
    fn test_agent_id_dynamic_display_uses_name_only() {
        let id = AgentId::Dynamic("BrandComboDetector".into());
        assert_eq!(id.to_string(), "BrandComboDetector");
    }

    #[test]
    fn test_agent_id_from_str_all_builtin() {
        // 主名
        assert_eq!(AgentId::parse("factcheck"), Some(AgentId::FactCheck));
        assert_eq!(AgentId::parse("FactCheckAgent"), Some(AgentId::FactCheck));
        // 别名
        assert_eq!(AgentId::parse("fact_check"), Some(AgentId::FactCheck));
        assert_eq!(AgentId::parse("blindspot"), Some(AgentId::BlindSpot));
        assert_eq!(AgentId::parse("blind_spot"), Some(AgentId::BlindSpot));
        assert_eq!(AgentId::parse("legalverify"), Some(AgentId::LegalVerify));
        assert_eq!(AgentId::parse("debate"), Some(AgentId::Debate));
    }

    #[test]
    fn test_agent_id_from_str_dynamic_prefix() {
        assert_eq!(
            AgentId::parse("dynamic_BrandDetector"),
            Some(AgentId::Dynamic("dynamic_BrandDetector".into()))
        );
    }

    #[test]
    fn test_agent_id_from_str_unknown_returns_none() {
        assert_eq!(AgentId::parse("nonexistent"), None);
        assert_eq!(AgentId::parse(""), None);
    }

    #[test]
    fn test_agent_id_hash_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(AgentId::FactCheck);
        set.insert(AgentId::FactCheck); // 重复插入
        assert_eq!(set.len(), 1);

        // Dynamic 同值等号
        let a = AgentId::Dynamic("Test".into());
        let b = AgentId::Dynamic("Test".into());
        assert_eq!(a, b);
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 2); // FactCheck + Dynamic("Test")
    }

    #[test]
    fn test_agent_id_all_reviewers_count() {
        let reviewers = AgentId::all_reviewers();
        assert_eq!(reviewers.len(), 7);
        // BlindSpot / LegalVerify / Debate 不在 reviewers 中
        assert!(!reviewers.contains(&AgentId::BlindSpot));
        assert!(!reviewers.contains(&AgentId::LegalVerify));
        assert!(!reviewers.contains(&AgentId::Debate));
    }

    // ── RiskFinding 字段完整性 ────────────────────────────────

    #[test]
    fn test_no_risk_finding_fields() {
        let f = RiskFinding::no_risk_finding(
            "R_001".into(),
            "ch_001".into(),
            "TestAgent",
            RiskTier::Medium,
            RiskTier::Medium,
        );
        assert!(f.no_risk);
        assert_eq!(f.severity, RiskSeverity::Info);
        assert_eq!(f.risk_type, "无风险");
        assert!(f.legal_basis.is_empty());
        assert!(f.case_refs.is_empty());
        assert!(f.suggested_agent.is_none());
        assert!(f.citations.is_empty());
        assert!(!f.truncated);
        assert!(!f.tier_escalated);
        assert!(f.confidence > 0.9);
    }

    #[test]
    fn test_truncated_finding_fields() {
        let f = RiskFinding::truncated_finding(
            "R_002".into(),
            "ch_002".into(),
            "TestAgent",
            RiskTier::High,
            RiskTier::High,
            "已搜索法规但未完成分析",
        );
        assert!(f.truncated);
        assert!(f.no_risk);
        assert!(f.confidence < 0.5);
        assert_eq!(f.risk_type, "审查截断");
        assert!(f.reason.contains("已搜索法规但未完成分析"));
        assert!(f.case_refs.is_empty());
        assert!(f.suggested_agent.is_none());
    }

    #[test]
    fn test_risk_finding_serialization_round_trip() {
        let f = RiskFinding {
            risk_id: "R_003".into(),
            clause_ids: vec!["ch_003".into()],
            block_ids: Vec::new(),
            highlight_rects: Vec::new(),
            agent: "SemanticRiskAgent".into(),
            no_risk: false,
            severity: RiskSeverity::High,
            is_critical: true,
            critical_reason: "唯一品牌且拒绝同等产品".into(),
            risk_type: "品牌指定".into(),
            category_code: "BRAND_LOCK".into(),
            source_quote: "须采用XX品牌".into(),
            legal_basis: vec!["《政府采购法》第5条".into()],
            case_refs: vec!["case_001".into()],
            reason: "存在品牌指向性".into(),
            suggestion: "修改为性能参数".into(),
            confidence: 0.85,
            initial_tier: RiskTier::High,
            final_tier: RiskTier::High,
            tier_escalated: false,
            truncated: false,
            suggested_agent: Some(SuggestedAgent {
                agent_name: "测试Agent".into(),
                agent_prompt: "你是测试Agent".into(),
                section_keywords: vec!["测试".into()],
                reason: "测试原因".into(),
            }),
            citations: vec![Citation {
                title: "测试来源".into(),
                url: "https://example.com".into(),
                site_name: "example".into(),
            }],
            finding_role: FindingRole::default(),
            knowledge_source: "search_verified".into(),
            verification_required: vec!["《政府采购法》".into()],
            hypothesized_by: vec!["ScoutAgent".into()],
            verified_by: vec!["SemanticRiskAgent".into()],
            evidence_verdict: None,
            verifier_reason: None,
            page_number: Some(0),
            section_path: Some(vec!["测试章节".into()]),
            context: Some("须采用XX品牌 测试上下文".into()),
        };

        let json = serde_json::to_string(&f).expect("序列化失败");
        let f2: RiskFinding = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(f.risk_id, f2.risk_id);
        assert_eq!(f.case_refs, f2.case_refs);
        assert!(f2.is_critical);
        assert_eq!(f2.critical_reason, "唯一品牌且拒绝同等产品");
        assert!(f2.suggested_agent.is_some());
        assert_eq!(f2.suggested_agent.unwrap().agent_name, "测试Agent");
        assert_eq!(f2.citations.len(), 1);
    }

    #[test]
    fn test_normalize_criticality_enforces_four_level_contract() {
        let mut finding = RiskFinding::no_risk_finding(
            "R_critical".into(),
            "ch_critical".into(),
            "RuleEngineAgent",
            RiskTier::High,
            RiskTier::High,
        );
        finding.no_risk = false;
        finding.severity = RiskSeverity::Medium;
        finding.is_critical = true;
        finding.risk_type = "地域注册限制".into();
        finding.normalize_criticality();

        assert_eq!(finding.severity, RiskSeverity::High);
        assert!(finding.is_critical);
        assert!(!finding.critical_reason.is_empty());

        finding.no_risk = true;
        finding.normalize_criticality();
        assert!(!finding.is_critical);
        assert!(finding.critical_reason.is_empty());
    }

    // ── GraphSnapshot 新字段 ──────────────────────────────────

    #[test]
    fn test_graph_snapshot_new_includes_all_fields() {
        let snap = GraphSnapshot::new();
        assert!(snap.chunks.is_empty());
        assert!(snap.risks.is_empty());
        assert!(snap.has_risk.is_empty());
        assert!(snap.reviewed_by.is_empty());
        assert!(snap.linked_to.is_empty());
        assert!(snap.cites.is_empty());
        assert!(snap.cited_by.is_empty());
        // 新增字段
        assert!(snap.agents.is_empty());
        assert!(snap.laws.is_empty());
        assert!(snap.cases.is_empty());
        assert!(snap.contradicts.is_empty());
        assert!(snap.same_law.is_empty());
    }

    // ── RiskSeverity 排序 ─────────────────────────────────────

    #[test]
    fn test_risk_severity_ordering() {
        assert!(RiskSeverity::High > RiskSeverity::Medium);
        assert!(RiskSeverity::Medium > RiskSeverity::Low);
        assert!(RiskSeverity::Low > RiskSeverity::Info);
    }

    // ── ReviewClause effective_max_turns ──────────────────────

    #[test]
    fn test_effective_max_turns_caps_at_agent_default() {
        let clause = ReviewClause {
            chunk_id: "ch_test".into(),
            section_path: vec![],
            text: "测试文本".into(),
            page_start: 0,
            page_end: 0,
            tier: RiskTier::High,
            tier_max_turns: 14,
            source_block_ids: vec![],
        };
        // agent 能力只有 4 轮 → 取 4
        assert_eq!(clause.effective_max_turns(4), 4);
        // agent 能力有 20 轮 → 取 14 (tier max)
        assert_eq!(clause.effective_max_turns(20), 14);
        // 刚好相等
        assert_eq!(clause.effective_max_turns(14), 14);
    }

    // ── ClauseContext ─────────────────────────────────────────

    #[test]
    fn test_clause_context_has_prior_risks() {
        let ctx = ClauseContext {
            chunk_id: "ch_001".into(),
            reviewed_by: vec![],
            risks: vec![],
            linked_chunks: vec![],
            same_law_chunks: vec![],
            contradictions: vec![],
        };
        assert!(!ctx.has_prior_risks());

        let ctx_with_risk = ClauseContext {
            risks: vec![RiskFinding::no_risk_finding(
                "R_001".into(),
                "ch_001".into(),
                "T",
                RiskTier::Medium,
                RiskTier::Medium,
            )],
            ..ctx
        };
        assert!(ctx_with_risk.has_prior_risks());
    }

    #[test]
    fn test_clause_context_reviewed_by_summary() {
        let ctx = ClauseContext {
            chunk_id: "ch_001".into(),
            reviewed_by: vec![],
            risks: vec![],
            linked_chunks: vec![],
            same_law_chunks: vec![],
            contradictions: vec![],
        };
        assert_eq!(ctx.reviewed_by_summary(), "（无）");
    }

    // ── DynamicAgentDefinition 序列化 ─────────────────────────

    #[test]
    fn test_dynamic_agent_definition_active_defaults_false() {
        let json = r#"{
            "id": "Dynamic_Test",
            "display_name": "测试",
            "system_prompt": "prompt",
            "default_max_turns": 8,
            "complexity": "Medium",
            "section_keywords": ["test"],
            "tool_names": ["web_search"],
            "created_at": "2026-01-01T00:00:00Z",
            "created_by": "BlindSpotAgent",
            "reason": "test"
        }"#;
        let def: DynamicAgentDefinition = serde_json::from_str(json).expect("反序列化失败");
        assert!(!def.active, "active 字段默认应为 false");
    }

    #[test]
    fn test_dynamic_agent_manifest_deserialization() {
        let json = r#"{"version": 1, "agents": []}"#;
        let manifest: DynamicAgentManifest = serde_json::from_str(json).expect("反序列化失败");
        assert_eq!(manifest.version, 1);
        assert!(manifest.agents.is_empty());
    }

    // ── CoordinatorConfig 默认值 ──────────────────────────────

    #[test]
    fn test_coordinator_config_defaults() {
        let config = CoordinatorConfig::default();
        assert_eq!(config.enabled_agents.len(), 7);
        assert!(!config.enable_legal_verify); // 成本优化：默认关闭 LLM 法条验证
        assert_eq!(config.legal_verify_max_turns, 3);
        assert_eq!(config.blind_spot_max_turns, 10);
        assert!(config.blind_spot_fallback_enabled);
        assert_eq!(config.max_parallel_clauses, 3);
    }
}
