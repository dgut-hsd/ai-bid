//! 知识沉淀引擎 — 三人共用的接口契约。
//!

use serde::{Deserialize, Serialize};


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
    /// 案例引用 ID 列表
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

/// 风险实体（按 risk_type 确定性 ID 去重，跨审核可合并）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskEntity {
    /// 确定性 ID：deterministic_id("risk:" + risk_type)
    pub id: String,
    /// 风险类型名称（"品牌指定"）
    pub name: String,
    /// "high" / "medium" / "low" / "info"
    pub severity: String,
}

/// 法规元数据（从原始法条引用的文号解析，或按名称推断）。
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
}

/// 法规 / 条款实体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawArticleEntity {
    /// 确定性 ID：deterministic_id(law_name)
    pub law_id: String,
    /// 法律名（"政府采购法实施条例"）
    pub law_name: String,
    /// 条款 ID：deterministic_id(law_id + ":" + article_no)；无条款号时为 None
    pub article_id: Option<String>,
    /// 归一化条款号（"第20条"）；无条款号时为 None
    pub article_no: Option<String>,
    /// 法规元数据（效力层级 / 发文机关 / 文号 / 年份）
    #[serde(default)]
    pub meta: Option<LawMeta>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDecision {
    pub candidate_id: String,
    pub decision: Decision,
    pub risk: RiskEntity,
    pub laws: Vec<LawArticleEntity>,
    /// 证据摘录（组员 B 从 Candidate.source_quote 填入；写库时存到 Risk 节点，查询展示用）。
    #[serde(default)]
    pub snippet: String,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub risk: RiskEntity,
    /// 与该风险关联的法规/条款
    pub laws: Vec<LawArticleEntity>,
    /// 命中该风险的候选（来源审核）ID 列表
    pub candidate_ids: Vec<String>,
    /// 摘录片段，用于展示
    pub snippet: String,
}
