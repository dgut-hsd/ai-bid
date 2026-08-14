//! YAML 规则反序列化结构 — 六要素规则模型。
//!
//! 与 `rules/data/*.yaml` 对应。Day 1 先定义结构，Day 2 填充匹配器字段语义。

use serde::{Deserialize, Serialize};

/// 一条 YAML 规则（六要素模型）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// ① 唯一 ID（如 `LOCAL_REGISTRATION_CITY_REGISTER`）
    pub id: String,
    /// ② 问题分类（必须是 15 个 canonical code 之一）
    pub category: String,
    /// ③ 行业（决定何时触发，防误报）
    #[serde(default = "default_industry")]
    pub industry: String,
    /// ④ 严重程度
    #[serde(default = "default_severity")]
    pub severity: String,
    /// ⑤ 来源追溯
    #[serde(default)]
    pub source: RuleSource,
    /// ⑥ 适用条件
    #[serde(default)]
    pub conditions: Conditions,
    /// ⑦ 匹配模式（多模式互补）
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    /// ⑧ 判定逻辑：any_match / all_match
    #[serde(default = "default_check")]
    pub check: String,
    /// 启用状态
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 法条引用
    #[serde(default)]
    pub law_ref: String,
}

fn default_industry() -> String {
    "GENERAL".into()
}
fn default_severity() -> String {
    "High".into()
}
fn default_check() -> String {
    "any_match".into()
}
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleSource {
    #[serde(default)]
    pub law: String,
    #[serde(default)]
    pub article: String,
    #[serde(default)]
    pub excerpt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Conditions {
    #[serde(default)]
    pub document_type: Option<String>,
    #[serde(default)]
    pub project_types: Vec<String>,
    #[serde(default)]
    pub trigger: Option<Trigger>,
    #[serde(default)]
    pub exclude: Option<Exclude>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trigger {
    #[serde(default)]
    pub chapter_keywords: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Exclude {
    #[serde(default)]
    pub project_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Pattern {
    /// 正则匹配
    #[serde(rename = "regex")]
    Regex { value: String },
    /// 关键词匹配（OR/AND 组合）
    #[serde(rename = "keyword")]
    Keyword {
        value: Vec<String>,
        #[serde(default = "default_keyword_mode")]
        mode: String,
        #[serde(default)]
        match_mode: Option<String>,
    },
    /// 字段比较
    #[serde(rename = "field_compare")]
    FieldCompare {
        left: String,
        operator: String,
        right: String,
    },
}

fn default_keyword_mode() -> String {
    "any".into()
}

/// 规则库（顶层 YAML 结构）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleBook {
    #[serde(default)]
    pub rules: Vec<Rule>,
}
