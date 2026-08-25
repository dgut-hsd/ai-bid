//! 规则分类目录 — CATEGORIES 常量、display_name、owner_agent、aliases。
//!
//! 从 `agents/risk_taxonomy.rs` 迁入。15 个 canonical code 保持不变，保证
//! `risk_taxonomy.rs` facade 的 5 个 public 函数签名向后兼容。
//!
//! `critical_default` 从 `data/catalog.yaml` 读取（单一事实源），
//! 加载失败时回退到内置硬编码默认值（与 YAML 内容一致），保证不崩溃。

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 15 个稳定风险分类（code, display_name）。
/// 与 `benchmark/risk_policy.py` 的 `CATEGORY_NAMES` 保持一致。
pub const CATEGORIES: &[(&str, &str)] = &[
    ("LOCAL_REGISTRATION", "地域注册限制"),
    ("BRAND_LOCK", "指定品牌且不接受同等产品"),
    ("UNRELATED_CERT", "设置与履约无关的资格条件"),
    ("REGIONAL_PERFORMANCE", "特定区域业绩限制"),
    ("SCALE_THRESHOLD", "以经营规模设置资格门槛"),
    ("SHORT_DEADLINE", "投标准备期不足"),
    ("EXCESSIVE_DEPOSIT", "投标保证金比例过高"),
    ("OEM_AUTHORIZATION", "将厂家授权作为资格条件"),
    ("SUBJECTIVE_SCORING", "主观评分未细化量化"),
    ("LOCAL_AWARD", "本地奖项加分"),
    ("VAGUE_ACCEPTANCE", "验收标准模糊"),
    ("UNBOUNDED_IP", "知识产权责任无限扩大"),
    ("UNILATERAL_CHANGE", "采购人可单方无限变更需求"),
    ("CONFLICTING_DATES", "关键日期相互矛盾"),
    ("UNCLEAR_PENALTY", "违约责任口径不清"),
];

/// 按 canonical code 查中文展示名。
pub fn display_name(code: &str) -> Option<&'static str> {
    CATEGORIES
        .iter()
        .find_map(|(candidate, name)| (*candidate == code).then_some(*name))
}

/// 按 category code 查责任 Agent。
pub fn owner_agent(code: &str) -> &'static str {
    match code {
        "SHORT_DEADLINE" | "EXCESSIVE_DEPOSIT" | "OEM_AUTHORIZATION" => "ProcedureAgent",
        "SUBJECTIVE_SCORING" | "LOCAL_AWARD" => "ScoringAgent",
        "BRAND_LOCK" | "UNRELATED_CERT" | "SCALE_THRESHOLD" => "DemandAgent",
        "VAGUE_ACCEPTANCE" | "UNBOUNDED_IP" | "UNILATERAL_CHANGE" | "UNCLEAR_PENALTY" => {
            "ContractAgent"
        }
        "LOCAL_REGISTRATION" | "REGIONAL_PERFORMANCE" => "SemanticRiskAgent",
        "CONFLICTING_DATES" => "FactCheckAgent",
        _ => "RuleEngineAgent",
    }
}

// ── catalog.yaml 读取（critical_default 单一事实源）──────────────────────

/// catalog.yaml 的 categories 条目结构。
#[derive(Debug, Deserialize, Serialize)]
struct CategoryDef {
    code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    owner_agent: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    critical_default: bool,
}

/// catalog.yaml 顶层结构。
#[derive(Debug, Deserialize, Serialize)]
struct CatalogFile {
    #[serde(default)]
    categories: Vec<CategoryDef>,
}

/// catalog.yaml 路径：相对 crate 根（CARGO_MANIFEST_DIR = backend-rust/）。
const CATALOG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/rules/data/catalog.yaml"
);

/// 加载 catalog.yaml；失败时回退到内置默认 critical 类别（与 YAML 内容一致）。
fn load_catalog() -> &'static [CategoryDef] {
    static CATALOG: OnceLock<Option<Vec<CategoryDef>>> = OnceLock::new();
    if let Some(cached) = CATALOG.get() {
        return cached.as_deref().unwrap_or(&[]);
    }
    match std::fs::read_to_string(CATALOG_PATH) {
        Ok(raw) => match serde_yaml::from_str::<CatalogFile>(&raw) {
            Ok(parsed) => {
                let _ = CATALOG.set(Some(parsed.categories));
                CATALOG.get().and_then(|c| c.as_deref()).unwrap_or(&[])
            }
            Err(e) => {
                eprintln!("[rules] WARNING: catalog.yaml parse failed: {e}");
                eprintln!("[rules]       path = {CATALOG_PATH}");
                eprintln!("[rules]       falling back to built-in critical defaults.");
                let _ = CATALOG.set(None);
                &[]
            }
        },
        Err(e) => {
            eprintln!("[rules] WARNING: catalog.yaml not readable: {e}");
            eprintln!("[rules]       path = {CATALOG_PATH}");
            eprintln!("[rules]       falling back to built-in critical defaults.");
            let _ = CATALOG.set(None);
            &[]
        }
    }
}

/// 内置默认 critical 类别（与 catalog.yaml 初始内容一致，作为加载失败回退）。
const FALLBACK_CRITICAL_DEFAULT: &[&str] = &[
    "LOCAL_REGISTRATION",
    "BRAND_LOCK",
    "UNRELATED_CERT",
    "REGIONAL_PERFORMANCE",
    "SCALE_THRESHOLD",
    "OEM_AUTHORIZATION",
    "UNBOUNDED_IP",
    "UNILATERAL_CHANGE",
];

/// 该类别在 catalog.yaml 中是否 `critical_default: true`。
/// engine.rs 的 Critical 判定依赖此函数（取代硬编码类别清单，消灭死数据）。
pub fn is_critical_default(code: &str) -> bool {
    let defs = load_catalog();
    if defs.is_empty() {
        return FALLBACK_CRITICAL_DEFAULT.contains(&code);
    }
    defs.iter()
        .find(|d| d.code == code)
        .map(|d| d.critical_default)
        .unwrap_or(false)
}
