//! =====================================================================
//! 规则引擎（第 3 组）— 人工可读的验证测试
//! =====================================================================
//!
//! 被测对象：`backend-rust/src/rules/engine.rs` 里两个纯函数：
//!
//!   1. `candidate_categories(text)`  —— 喂一条条款文本，看它命中哪些风险类别
//!   2. `normalize_finding(&mut f)`    —— 喂一条风险发现，看它是否被判为 Critical 红线
//!
//! 特点：
//!   * 完全不依赖 LLM / Agent / 网络 / 主审核链路，只测规则引擎本身。
//!   * 规则库 `conditions.yaml` / `catalog.yaml` 从 `CARGO_MANIFEST_DIR` 绝对路径加载，
//!     所以在任何目录下运行都能找到。
//!
//! 运行方式（加 --nocapture 才会在每条通过时也打印中文明细）：
//!
//!   cargo test --test rule_engine_verify -- --nocapture
//!
//! 每个 case 都带一句中文说明，失败时断言消息直接告诉你「哪一条条款、
//! 应命中什么、实际命中了什么」，方便人肉眼看懂。

use ai_bid::agents::types::{FindingRole, RiskFinding, RiskSeverity, RiskTier};
use ai_bid::rules::catalog::{display_name, owner_agent};
use ai_bid::rules::engine::{candidate_categories, normalize_finding};

/// 把一个命中结果打印成一句人能直接读懂的中文。
fn describe(text: &str, hits: &[&str]) {
    println!("  条款：{}", text);
    if hits.is_empty() {
        println!("    → 未命中任何风险（合规）");
        return;
    }
    for code in hits {
        let name = display_name(code).unwrap_or(code);
        let agent = owner_agent(code);
        println!("    → 命中 {code}（{name}，责任 Agent = {agent}）");
    }
}

/// 构造一个最小 `RiskFinding`，只关心 category_code（编码）和 source_quote（证据原文）。
fn finding(code: &str, quote: &str) -> RiskFinding {
    RiskFinding {
        risk_id: "R_VERIFY".into(),
        clause_ids: vec!["ch_verify".into()],
        block_ids: vec![],
        highlight_rects: Vec::new(),
        agent: "rule_engine_verify".into(),
        no_risk: false,
        severity: RiskSeverity::High,
        is_critical: false,
        critical_reason: String::new(),
        risk_type: code.into(),
        category_code: code.into(),
        source_quote: quote.into(),
        legal_basis: vec![],
        case_refs: vec![],
        reason: String::new(),
        suggestion: String::new(),
        confidence: 0.9,
        initial_tier: RiskTier::Medium,
        final_tier: RiskTier::Medium,
        tier_escalated: false,
        truncated: false,
        suggested_agent: None,
        citations: vec![],
        finding_role: FindingRole::Verified,
        knowledge_source: String::new(),
        verification_required: vec![],
        hypothesized_by: vec![],
        verified_by: vec![],
        evidence_verdict: None,
        verifier_reason: None,
        page_number: None,
        section_path: None,
        context: None,
    }
}

// ---------------------------------------------------------------------
// 测试 1：15 个风险类别，每个都有一条能命中的真实条款。
// 这一步验证「规则引擎确实认得全所有类别」。
// ---------------------------------------------------------------------

/// 每条案例 = (中文说明, 条款原文, 期望至少命中的类别编码)
const ALL_15_CASES: &[(&str, &str, &str)] = &[
    ("地域注册限制", "投标人须在本市注册成立三年以上，且在本市设有分支机构。", "LOCAL_REGISTRATION"),
    ("指定品牌且不接受同等产品", "本项目仅接受指定品牌XYZ型号的投标，其他品牌投标无效。", "BRAND_LOCK"),
    ("设置与履约无关的资格条件", "供应商须提供诚信示范企业荣誉证书，否则资格审查不通过。", "UNRELATED_CERT"),
    ("特定区域业绩限制", "供应商须提供本市同类项目业绩案例，外地业绩不作为有效业绩。", "REGIONAL_PERFORMANCE"),
    ("以经营规模设置资格门槛", "投标人注册资本不得低于5000万元，且近三年营业收入不少于1亿元。", "SCALE_THRESHOLD"),
    // ↓ 这两条硬编码抓不到，靠 YAML 规则补上（详见测试 3）
    ("投标准备期不足", "仅5日递交投标文件", "SHORT_DEADLINE"),
    ("投标保证金比例过高", "投标人应缴纳采购预算总额6%的保证金", "EXCESSIVE_DEPOSIT"),
    ("将厂家授权作为资格条件", "投标人必须提交生产厂家针对本项目出具的授权函，否则投标无效。", "OEM_AUTHORIZATION"),
    ("主观评分未细化量化", "技术方案由评委酌情打分。", "SUBJECTIVE_SCORING"),
    ("本地奖项加分", "本市获奖企业加2分。", "LOCAL_AWARD"),
    ("验收标准模糊", "验收由采购人满意为准。", "VAGUE_ACCEPTANCE"),
    ("知识产权责任无限扩大", "供应商对采购人承担全部知识产权侵权责任，无限赔偿，无上限。", "UNBOUNDED_IP"),
    ("采购人可单方无限变更需求", "采购人有权单方无限变更需求，供应商不得调整合同费用和工期。", "UNILATERAL_CHANGE"),
    ("关键日期相互矛盾", "投标截止日期与开标日期矛盾。", "CONFLICTING_DATES"),
    ("违约责任口径不清", "违约金由采购人自行决定。", "UNCLEAR_PENALTY"),
];

#[test]
fn candidate_categories_covers_all_15_risk_types() {
    println!("\n===== 测试 1：15 个类别逐条命中检查 =====");
    for (label, text, expected) in ALL_15_CASES {
        let hits = candidate_categories(text);
        describe(text, &hits);
        assert!(
            hits.contains(&expected),
            "【{label}】应命中 {expected}，实际命中 {hits:?}\n  原文：{text}"
        );
    }
}

// ---------------------------------------------------------------------
// 测试 2：合规条款 / 空串不应误报。
// 这一步验证「规则引擎不会乱开枪」——少报是坏，误报同样是坏。
// ---------------------------------------------------------------------

const BENIGN_CLAUSES: &[&str] = &[
    "",                                                  // 空串
    "这是一般商务条款，不涉及风险。",                       // 纯商务话术
    "本项目的招标文件获取时间为2026年8月1日至2026年8月10日。", // 只是陈述时间，不构成风险
];

#[test]
fn benign_clauses_are_not_flagged() {
    println!("\n===== 测试 2：合规条款不应误报 =====");
    for text in BENIGN_CLAUSES {
        let hits = candidate_categories(text);
        describe(text, &hits);
        assert!(
            hits.is_empty(),
            "合规条款不应命中任何风险，却命中了 {hits:?}\n  原文：{text:?}"
        );
    }
}

// ---------------------------------------------------------------------
// 测试 3：硬编码抓不到、只有 YAML 规则才抓得到的「补位」案例。
// 这正是第 3 组报告里的核心卖点——regex / field_compare / absence
// 三类新匹配器补上了硬编码的盲区。
// ---------------------------------------------------------------------

const YAML_ONLY_CASES: &[(&str, &str, &str)] = &[
    // 硬编码要求句子含“投标保证金”关键词，这句没有 → 只能靠 YAML regex
    ("YAML regex：6%的保证金（硬编码抓不到）", "投标人应缴纳采购预算总额6%的保证金", "EXCESSIVE_DEPOSIT"),
    // 硬编码要求“投标截止/开标”等词，这句没有 → 只能靠 YAML regex
    ("YAML regex：仅5日递交（硬编码抓不到）", "仅5日递交投标文件", "SHORT_DEADLINE"),
    // absence 缺失型：施工资质章节“没提安全生产许可证”即命中（硬编码只能匹配“出现”，匹配不了“缺失”）
    ("YAML absence：施工资质章节未提安全生产许可证", "安全生产施工资质要求：施工单位须具备相关施工资质。", "VAGUE_ACCEPTANCE"),
    // field_compare 数值型：抽取 5% 保证金比例，判定 > 2% 上限
    ("YAML field_compare：保证金5%超过2%上限", "投标保证金不得超过估算价的5%", "EXCESSIVE_DEPOSIT"),
];

#[test]
fn yaml_only_cases_fill_hardcoded_blind_spots() {
    println!("\n===== 测试 3：YAML 规则补位（硬编码盲区）=====");
    for (label, text, expected) in YAML_ONLY_CASES {
        let hits = candidate_categories(text);
        describe(text, &hits);
        assert!(
            hits.contains(&expected),
            "【{label}】应命中 {expected}，实际命中 {hits:?}\n  原文：{text}"
        );
    }
}

// ---------------------------------------------------------------------
// 测试 4：Critical 红线判定（normalize_finding → is_critical）。
// 8 个 critical_default=true 的类别命中证据后必须标 Critical；
// 7 个 critical_default=false 的类别即使命中也不应标 Critical。
// ---------------------------------------------------------------------

#[test]
fn critical_red_line_judgement() {
    println!("\n===== 测试 4：Critical 红线判定 =====");

    // —— 应标 Critical 的 8 个类别（catalog.yaml critical_default: true）——
    let should_be_critical: &[(&str, &str)] = &[
        ("LOCAL_REGISTRATION", "投标人须在本市注册成立三年以上，且在本市设有分支机构。"),
        ("BRAND_LOCK", "本项目仅接受指定品牌XYZ型号的投标，其他品牌投标无效。"),
        ("UNRELATED_CERT", "供应商须提供诚信示范企业荣誉证书，否则资格审查不通过。"),
        ("REGIONAL_PERFORMANCE", "供应商须提供本市同类项目业绩案例，外地业绩不作为有效业绩。"),
        ("SCALE_THRESHOLD", "投标人注册资本不得低于5000万元，且近三年营业收入不少于1亿元。"),
        ("OEM_AUTHORIZATION", "投标人必须提交生产厂家针对本项目出具的专项授权函，否则投标无效。"),
        ("UNBOUNDED_IP", "供应商对采购人承担全部知识产权侵权责任，无限赔偿，无上限。"),
        ("UNILATERAL_CHANGE", "采购人有权单方无限变更需求，供应商不得调整合同费用和工期。"),
    ];

    for (code, quote) in should_be_critical {
        let mut f = finding(code, quote);
        normalize_finding(&mut f);
        println!("  [{code}] Critical={}  原文：{quote}", f.is_critical);
        assert!(
            f.is_critical,
            "【{code}】应判定为 Critical 红线，但实际 is_critical=false\n  原文：{quote}"
        );
    }

    // —— 不应标 Critical 的 7 个类别（critical_default: false）——
    let should_not_be_critical: &[(&str, &str)] = &[
        ("SHORT_DEADLINE", "投标人须在3日内递交投标文件。"),
        ("EXCESSIVE_DEPOSIT", "投标保证金不得超过估算价的5%。"),
        ("SUBJECTIVE_SCORING", "技术方案由评委酌情打分。"),
        ("LOCAL_AWARD", "本市获奖企业加2分。"),
        ("VAGUE_ACCEPTANCE", "验收由采购人满意为准。"),
        ("CONFLICTING_DATES", "投标截止日期与开标日期矛盾。"),
        ("UNCLEAR_PENALTY", "违约金由采购人自行决定。"),
    ];

    for (code, quote) in should_not_be_critical {
        let mut f = finding(code, quote);
        normalize_finding(&mut f);
        println!("  [{code}] Critical={}  原文：{quote}", f.is_critical);
        assert!(
            !f.is_critical,
            "【{code}】不应判定为 Critical，但实际 is_critical=true\n  原文：{quote}"
        );
    }
}