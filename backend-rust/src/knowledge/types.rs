//! G6 知识沉淀引擎 — 三人共用的接口契约。
//!
//! 图数据模型遵循《设计.md》§9.4.4 的 Long-term Memory / Knowledge Curator schema，并补充
//! Neo4j 约束下的经验节点：
//! 节点类型：Risk / Law / Article / Case / ProhibitionRule / RiskExperience / Dimension
//! 关系：cites / has_article / cited_in / exemplifies / based_on / applies_to / has_experience
//!
//! 说明：Neo4j 属性值只能是原始类型或原始类型数组，map 数组非法；故"每次审核的处置经验
//! （reason/suggestion/source_quote）"以独立 :RiskExperience 节点承载（(Risk)-[:has_experience]
//! ->(RiskExperience)），不内嵌为属性。

use serde::{Deserialize, Serialize};

/// 候选精华（组员 A 从审核结果中挑选，供组员 B 拆实体）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    /// 候选唯一标识，复用审核结果里的 risk_id（仅当次会话内使用，不保证跨审核唯一）。
    pub candidate_id: String,
    /// 来源审核结果的 risk_id，用于追溯。
    pub risk_id: String,
    /// "high" / "medium" / "low" / "info"
    pub severity: String,
    /// 风险类型标签（"品牌指定" / "地域歧视" / …）
    pub risk_type: String,
    /// 法条引用列表（如 ["《政府采购法实施条例》第二十条"]）
    pub legal_basis: Vec<String>,
    /// 案例引用 ID 列表（如 ["case_001","case_145"]）
    pub case_refs: Vec<String>,
    /// 原文摘录
    pub source_quote: String,
    /// 推理理由
    pub reason: String,
    /// 修改建议
    pub suggestion: String,
    /// 置信度 [0.0, 1.0]
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    /// 库里还没有 → 需要写入
    New,
    /// 库里已有 → 跳过写入
    Exists,
}

/// 处置经验节点（RiskExperience）：对某风险类型的一次审核的 事实锚点 + 论证理由 + 处置建议。
/// 独立节点承载（Neo4j 属性值禁止 map 数组），关系为 (Risk)-[:has_experience]->(RiskExperience)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskExperience {
    /// 确定性 ID：gen_experience_id(risk_id, candidate_id)
    pub experience_id: String,
    /// 来源候选编号（会话内 risk_id，如 "R_017"；跨审核需结合批次溯源）
    pub candidate_id: String,
    /// 事实锚点（原文摘录）
    pub source_quote: String,
    /// 论证理由（原始文本）
    pub reason: String,
    /// 处置建议
    pub suggestion: String,
    /// 置信度 [0.0, 1.0]
    pub confidence: f32,
}

/// 风险实体（审查记录，按 risk_type 确定性 ID 去重，跨审核可合并）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskEntity {
    /// 确定性 ID：gen_risk_id("risk:" + risk_type)
    pub id: String,
    /// 风险类型名称（"品牌指定"）
    pub name: String,
    /// "high" / "medium" / "low" / "info"
    pub severity: String,
}

/// 法规元数据（效力层级 / 发文机关 / 文号 / 年份 / 时效状态）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LawMeta {
    /// 效力层级：法律 / 行政法规 / 部门规章 / 规范性文件 / 未分类
    pub level: String,
    /// 发文机关（"财政部" / "国务院" / "国务院办公厅" …）
    pub issuing_body: String,
    /// 文号（"财政部令第94号" / "财库〔2019〕38号" …）；无文号时为空
    pub doc_number: String,
    /// 发布年份（从文号年份推断）；无则 None
    pub year: Option<String>,
    /// 生效日期（YYYY-MM-DD 或 YYYY；当前从文号年份推断，外部数据源可覆盖）
    #[serde(default)]
    pub effective_date: String,
    /// 时效状态：effective | amended | repealed（初始默认 effective）
    #[serde(default)]
    pub status: String,
}

/// 法规 / 条款实体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawArticleEntity {
    /// 确定性 ID：gen_law_id(law_name)
    pub law_id: String,
    /// 法律名（"政府采购法实施条例"）
    pub law_name: String,
    /// 法律简称（去掉"中华人民共和国"等前缀后的常用名，如"政府采购法"）
    #[serde(default)]
    pub short_name: String,
    /// 条款 ID：gen_article_id(law_id + ":" + article_no)；无条款号时为 None
    pub article_id: Option<String>,
    /// 归一化条款号（"第20条"）；无条款号时为 None
    pub article_no: Option<String>,
    /// 条款摘要（条款自身的归纳；无外部来源时留空，不承载单次审核的 reason）
    #[serde(default)]
    pub summary: String,
    /// 法规元数据（效力层级 / 发文机关 / 文号 / 年份 / 时效）
    #[serde(default)]
    pub meta: Option<LawMeta>,
}

/// 案例实体（外部案例引用，独立节点，对应设计文档 §9.4.4 的 Case 节点）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseEntity {
    /// 确定性 ID：gen_case_id(case_ref)
    pub case_id: String,
    /// 案例引用原文（"财政部投诉处理决定第XX号"）
    pub title: String,
    /// 发布机关（从 case_ref 推断，如 "财政部"）
    pub issuing_body: String,
    /// 年度（从 case_ref 推断年份，None 时无）
    pub year: Option<i64>,
    /// 案例类型（"投诉处理决定" / "行政处罚决定" / "司法判决" / "行政复议决定" / "未分类"）
    pub case_type: String,
    /// 结论摘要（取自该候选的 reason / suggestion）
    pub summary: String,
    /// 案例完整原文（当前提取管线无法获取，留空，后续接入案例库可填充）
    #[serde(default)]
    pub full_text: String,
    /// 关键词列表（取自 risk_type，供检索）
    pub keywords: Vec<String>,
}

/// 负面清单实体（ProhibitionRule 节点，从 risk_type + reason 抽取）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEntity {
    /// 确定性 ID：gen_rule_id(content)
    pub rule_id: String,
    /// 禁止内容（该候选 risk_type + reason 归纳）
    pub content: String,
    /// 来源文件（当前无结构化来源，留空）
    pub source: String,
    /// 所属维度类别（资格条件 / 评审标准 / …）
    pub category: String,
    /// 红线 / 黄线
    pub severity: String,
}

/// 审查维度（Dimension 节点，静态预置，对应文档 §9.4.4）。
/// 维度与审查 Agent 对应：资格条件 / 采购程序 / 评审标准 / 技术参数 / 合同条款 / 需求质量 / 文件格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionEntity {
    /// 确定性 ID（如 "dim_qualification"）
    pub dimension_id: String,
    /// 维度名（"资格条件"）
    pub name: String,
    /// 描述
    pub description: String,
}

/// 决策实体，包含从审核结果派生的所有必要字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDecision {
    /// 候选唯一标识，复用审核结果里的 risk_id（仅当次会话内使用，不保证跨审核唯一）。
    pub candidate_id: String,
    /// 写入决策：New 或 Exists
    pub decision: Decision,
    /// 风险实体（risk_type + severity + name）
    pub risk: RiskEntity,
    /// 法律/条款列表
    pub laws: Vec<LawArticleEntity>,
    /// 案例列表（独立 Case 节点）
    #[serde(default)]
    pub cases: Vec<CaseEntity>,
    /// 负面清单规则（ProhibitionRule 节点）
    #[serde(default)]
    pub rules: Vec<RuleEntity>,
    /// 风险所属审查维度
    #[serde(default)]
    pub dimension: Option<DimensionEntity>,
    /// 本次审核产生的一条处置经验（写库时追加到 Risk.experiences，任何审核必落）
    pub risk_experience: RiskExperience,
}

/// 查询命中项：风险 + 关联的法条 / 条款 / 案例 / 负面清单 / 维度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub risk: RiskEntity,
    /// 与该风险关联的法规/条款
    pub laws: Vec<LawArticleEntity>,
    /// 摘录片段（派生自首条经验 source_quote，仅展示用，非图属性）
    #[serde(default)]
    pub snippet: String,
    /// 该风险引用的案例（独立 Case 节点）
    #[serde(default)]
    pub cases: Vec<CaseEntity>,
    /// 该风险体现的负面清单规则
    #[serde(default)]
    pub rules: Vec<RuleEntity>,
    /// 该风险的处置经验节点（RiskExperience，每次审核一条）
    #[serde(default)]
    pub experiences: Vec<RiskExperience>,
    /// 该风险所属的审查维度
    #[serde(default)]
    pub dimension: Option<DimensionEntity>,
}

/// 预置审查维度（与 7 个审查 Agent 对应）。
pub const PRESET_DIMENSIONS: [(&str, &str, &str); 7] = [
    (
        "dim_qualification",
        "资格条件",
        "供应商资格门槛的公平性、合法性、必要性",
    ),
    ("dim_procedure", "采购程序", "采购程序合规性（时间节点、流程、公告）"),
    (
        "dim_evaluation",
        "评审标准",
        "评审因素、评分标准的公平性与合法性",
    ),
    (
        "dim_technical",
        "技术参数",
        "技术参数的必要性、倾向性、排他性",
    ),
    (
        "dim_contract",
        "合同条款",
        "合同条款的合法性与风险分配",
    ),
    ("dim_demand", "需求质量", "采购需求描述的完整性、可理解性"),
    ("dim_format", "文件格式", "投标文件格式、签字盖章等程序性要求"),
];