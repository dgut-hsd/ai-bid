//! 规则引擎核心 — 分类归一化、候选预检、证据准入、Critical 判定。
//!
//! 从 `agents/risk_taxonomy.rs` 迁入全部私有逻辑。`risk_taxonomy.rs` 作为 facade
//! 委托给本模块，保持 5 个 public 函数签名字节级不变。
//!
//! Day 1：迁移现有硬编码逻辑（行为兼容）。
//! Day 2+：逐步迁到 YAML 规则（data/*.yaml），本模块改为加载 YAML 后求值。

use crate::agents::types::{RiskFinding, RiskSeverity};
use crate::rules::catalog::{owner_agent, CATEGORIES};
use crate::rules::metrics::DocumentMetrics;
use crate::rules::schema::RuleBook;
use crate::rules::validator::{evaluate_rulebook, load_rulebook};
use std::sync::OnceLock;

/// 关键词 OR 匹配（从 risk_taxonomy.rs 迁入）。
fn contains_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}

/// 类别码归一化（从 risk_taxonomy.rs 迁入）。
fn normalized_code(value: &str) -> String {
    let upper: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .flat_map(char::to_uppercase)
        .collect();
    if let Some((prefix, rest)) = upper.split_once('_')
        && prefix.len() >= 2
        && prefix.starts_with(|c: char| c.is_ascii_alphabetic())
        && prefix[1..].chars().all(|c| c.is_ascii_digit())
    {
        return rest.to_string();
    }
    upper
}

/// 优先依据逐字证据分类（从 risk_taxonomy.rs 迁入）。
fn category_from_evidence(text: &str) -> Option<&'static str> {
    let regional = contains_any(
        text,
        &[
            "本市",
            "本区",
            "本县",
            "本省",
            "当地",
            "所在地",
            "所在区县",
            "采购人所在地",
            "所在市",
            "所在省份",
            "省外",
            "异地",
            "采购人所在",
        ],
    );
    if regional
        && contains_any(text, &["注册", "分公司", "分支机构", "经营满", "营业执照", "纳税"])
        && contains_any(text, &["须", "必须", "仅限", "不接受", "资格", "应", "直接退回"])
    {
        return Some("LOCAL_REGISTRATION");
    }
    if regional
        && contains_any(text, &["业绩", "案例", "合同"])
        && contains_any(text, &["须", "必须", "仅限", "不认可", "不予认可", "不得", "只统计", "不计分"])
    {
        return Some("REGIONAL_PERFORMANCE");
    }
    if regional
        && contains_any(text, &["奖项", "荣誉", "获奖", "证书", "诚信企业"])
        && contains_any(text, &["加分", "得分", "评分", "分"])
    {
        return Some("LOCAL_AWARD");
    }
    if contains_any(text, &["注册资本", "营业收入", "资产总额", "净资产", "实缴资本", "实收资本", "业务收入", "主营", "规模"])
        && contains_any(text, &["不得低于", "不少于", "以上", "门槛", "资格", "不低于", "达到", "未达到"])
    {
        return Some("SCALE_THRESHOLD");
    }
    if contains_any(text, &["品牌", "商标", "型号", "厂牌", "系列", "原装", "机型", "指定机型"])
        && contains_any(text, &["仅", "只能", "唯一", "指定", "不接受", "不得偏离", "不予响应", "只准", "不得"])
    {
        return Some("BRAND_LOCK");
    }
    if contains_any(text, &["原厂", "厂家", "制造商"])
        && contains_any(text, &["授权", "承诺函", "证明"])
        && contains_any(text, &["资格", "无效", "废标", "必须", "须", "必备材料", "终止审查"])
    {
        return Some("OEM_AUTHORIZATION");
    }
    if contains_any(text, &["认证", "证书", "荣誉", "示范企业"])
        && contains_any(
            text,
            &["资格", "无效", "废标", "不通过", "必须", "须提供", "无关"],
        )
    {
        return Some("UNRELATED_CERT");
    }
    if text.contains("投标保证金")
        && contains_any(
            text,
            &[
                "3%",
                "4%",
                "5%",
                "百分之三",
                "百分之四",
                "百分之五",
                "比例过高",
            ],
        )
    {
        return Some("EXCESSIVE_DEPOSIT");
    }
    if contains_any(text, &["投标截止", "开标", "投标准备", "获取招标文件"])
        && contains_any(text, &["3日", "5日", "不足", "少于", "仅有", "仅"])
    {
        return Some("SHORT_DEADLINE");
    }
    if contains_any(text, &["评分", "得分", "评委"])
        && contains_any(
            text,
            &["酌情", "自行掌握", "主观", "优良", "满意程度", "综合判断"],
        )
    {
        return Some("SUBJECTIVE_SCORING");
    }
    if text.contains("验收")
        && contains_any(
            text,
            &["满意", "自行判断", "无异议", "未明确", "不明确", "无需说明"],
        )
    {
        return Some("VAGUE_ACCEPTANCE");
    }
    if contains_any(text, &["知识产权", "侵权", "专利", "既有软件", "权利"])
        && contains_any(
            text,
            &["全部责任", "一切责任", "无限", "无上限", "既有", "永久归", "不设最高限额"],
        )
    {
        return Some("UNBOUNDED_IP");
    }
    if contains_any(text, &["单方", "新增需求", "任意变更", "采购人有权变更"])
        && contains_any(
            text,
            &["不得调整", "不调整", "无条件", "原合同范围", "费用", "工期"],
        )
    {
        return Some("UNILATERAL_CHANGE");
    }
    if contains_any(text, &["日期", "截止", "开标时间"])
        && contains_any(text, &["矛盾", "不一致", "另一处", "分别为", "同时规定"])
    {
        return Some("CONFLICTING_DATES");
    }
    if contains_any(text, &["违约金", "违约责任", "处罚"])
        && contains_any(
            text,
            &["重复", "累计", "无上限", "自行决定", "不明确", "不清"],
        )
    {
        return Some("UNCLEAR_PENALTY");
    }
    None
}

/// 别名表归一化（从 risk_taxonomy.rs 迁入）。
fn category_from_alias(value: &str) -> Option<&'static str> {
    let code = normalized_code(value);
    CATEGORIES
        .iter()
        .find_map(|(canonical, _)| (code == *canonical).then_some(*canonical))
        .or_else(|| match code.as_str() {
            "UNRELATED_CERTIFICATE"
            | "UNRELATED_CERTIFICATION"
            | "UNRELATED_QUALIFICATION"
            | "IRRELEVANT_CERTIFICATE" => Some("UNRELATED_CERT"),
            "SHORT_PREPARATION_PERIOD"
            | "UNREASONABLE_TIME_LIMIT"
            | "UNREASONABLE_PREPARATION_TIME"
            | "SHORT_BIDDING_PERIOD"
            | "TIME_LIMIT" => Some("SHORT_DEADLINE"),
            "ASSET_THRESHOLD" | "ASSET_REQUIREMENT" | "CAPITAL_THRESHOLD" => {
                Some("SCALE_THRESHOLD")
            }
            "MANUFACTURER_AUTHORIZATION" | "FACTORY_AUTHORIZATION" => Some("OEM_AUTHORIZATION"),
            "SCORING_DISCRETION" | "UNSPECIFIED_SCORING" | "UNQUANTIFIED_ASSESSMENT" => {
                Some("SUBJECTIVE_SCORING")
            }
            "LOCAL_CERTIFICATE_BONUS" | "LOCAL_HONOR_BONUS" => Some("LOCAL_AWARD"),
            "UNCLEAR_ACCEPTANCE_CRITERIA"
            | "ACCEPTANCE_CRITERIA"
            | "AMBIGUOUS_ACCEPTANCE"
            | "UNCLEAR_ACCEPTANCE" => Some("VAGUE_ACCEPTANCE"),
            "UNLIMITED_IP_LIABILITY" | "IP_LIABILITY" => Some("UNBOUNDED_IP"),
            "UNDEFINED_PENALTY" | "UNLIMITED_PENALTY" | "UNCLEAR_CONTRACTUAL_RESPONSIBILITY" => {
                Some("UNCLEAR_PENALTY")
            }
            "DATE_CONFLICT" | "关键日期矛盾" => Some("CONFLICTING_DATES"),
            _ => None,
        })
}

/// Critical 红线判定。
///
/// 类别级开关来自 `catalog.yaml` 的 `critical_default`（单一事实源，engine
/// 实际读取，不再是死数据）；仅在类别默认 Critical 时，才继续用证据关键词
/// 判断该条款是否真正触发红线。
fn critical_evidence(code: &str, quote: &str) -> bool {
    if !crate::rules::catalog::is_critical_default(code) {
        return false;
    }
    match code {
        "LOCAL_REGISTRATION" => {
            contains_any(quote, &["注册", "分公司", "分支机构", "营业执照", "纳税"])
                && contains_any(quote, &["本市", "本区", "本县", "所在地", "外地", "所在市", "异地", "采购人所在"])
        }
        "BRAND_LOCK" => {
            contains_any(quote, &["品牌", "商标", "型号", "厂牌", "系列", "指定产品", "原装", "原装产品", "机型"])
                && contains_any(quote, &["仅", "只能", "唯一", "不接受", "指定", "不得偏离", "不予响应", "只准", "不得"])
        }
        "UNRELATED_CERT" => {
            contains_any(quote, &["认证", "证书", "荣誉", "示范企业", "名牌产品", "驰名商标"])
                && contains_any(
                    quote,
                    &["资格", "无效", "废标", "不通过", "必须", "须", "无关"],
                )
        }
        "REGIONAL_PERFORMANCE" => {
            contains_any(quote, &["业绩", "案例", "合同"])
                && contains_any(quote, &["本市", "本区", "本县", "本省", "当地", "所在区县", "所在省", "省外", "采购人所在"])
        }
        "SCALE_THRESHOLD" => {
            contains_any(quote, &["注册资本", "实缴资本", "营业收入", "资产总额", "净资产", "业务收入", "主营", "规模"])
                && contains_any(quote, &["不得低于", "不少于", "以上", "资格", "不低于", "达到", "未达到"])
        }
        "OEM_AUTHORIZATION" => {
            contains_any(quote, &["原厂", "厂家", "制造商", "总代理商", "原厂商"])
                && contains_any(quote, &["授权", "承诺函", "专项授权书", "项目授权书", "代理证明"])
                && contains_any(quote, &["资格", "无效", "废标", "必须", "须", "作为资格条件", "必备材料", "终止审查"])
        }
        "UNBOUNDED_IP" => {
            contains_any(quote, &["知识产权", "侵权", "专利", "著作权", "软件著作权"])
                && contains_any(quote, &["全部责任", "一切责任", "无限", "无上限", "永久归", "不设最高限额"])
        }
        "UNILATERAL_CHANGE" => {
            contains_any(quote, &["单方", "采购人有权变更", "任意调整需求", "新增需求由供应商"])
                && contains_any(quote, &["不得调整", "不调整", "无条件", "费用不变", "工期不变", "原合同范围"])
        }
        // 其余类别（catalog critical_default: false）已在上方被过滤，不会到达这里
        _ => false,
    }
}

// ── YAML 规则库接入（离线缓存 + 主链路集成）─────────────────────────────

/// 相对 crate 根（CARGO_MANIFEST_DIR = backend-rust/）定位规则库，
/// 不依赖运行目录（CWD），从任意目录运行都能加载。
const RULEBOOK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/rules/data/conditions.yaml"
);

/// 惰性加载规则库。
///
/// - 成功：缓存到 `OnceLock<Option<RuleBook>>`，后续调用直接复用；
/// - 失败：**不缓存失败状态**，打印告警并返回 `None`，下一次调用会重新尝试，
///   因此修复规则文件/CWD 后无需重启进程即可恢复 YAML 规则。
fn get_rulebook() -> Option<&'static RuleBook> {
    static RULEBOOK: OnceLock<Option<RuleBook>> = OnceLock::new();
    // 已成功缓存 → 直接返回
    if let Some(cached) = RULEBOOK.get() {
        return cached.as_ref();
    }
    match load_rulebook(RULEBOOK_PATH) {
        Ok((book, warnings)) => {
            if !warnings.is_empty() {
                eprintln!("[rules] Rulebook warnings: {}", warnings.len());
            }
            let _ = RULEBOOK.set(Some(book));
            RULEBOOK.get().and_then(|b| b.as_ref())
        }
        Err(e) => {
            // 不 set：下次调用重试；同时输出可操作的错误信息（含路径与运行目录提示）
            eprintln!("[rules] ERROR: YAML rulebook failed to load: {e}");
            eprintln!("[rules]       path = {RULEBOOK_PATH}");
            eprintln!(
                "[rules]       YAML rules are DISABLED for this run. Fix the file, no restart needed."
            );
            None
        }
    }
}

/// 将 YAML category String 映射回 `&'static str` canonical code。
fn category_to_static(cat: &str) -> Option<&'static str> {
    CATEGORIES
        .iter()
        .find_map(|(code, _)| (*code == cat).then_some(*code))
}

/// 对条款文本执行 YAML 规则求值，返回去重后的 canonical category 列表。
fn yaml_candidate_categories(text: &str) -> Vec<&'static str> {
    let Some(book) = get_rulebook() else {
        return vec![];
    };
    let metrics = DocumentMetrics::extract_from_clause_text(text);
    let hits = evaluate_rulebook(book, text, &metrics);
    let mut result: Vec<&'static str> = Vec::new();
    for (_, category) in hits {
        if let Some(static_cat) = category_to_static(&category) {
            if !result.contains(&static_cat) {
                result.push(static_cat);
            }
        }
    }
    result
}

/// 检查文本是否匹配 YAML 中 severity=Critical 的规则，**且命中规则属于
/// 当前 finding 的类别**（与 `critical_evidence` 的类别收敛语义一致，
/// 避免跨类别误判——例如 SHORT_DEADLINE 文本里出现其他 Critical 规则的关键词）。
fn yaml_is_critical(quote: &str, category: &str) -> bool {
    let Some(book) = get_rulebook() else {
        return false;
    };
    let metrics = DocumentMetrics::extract_from_clause_text(quote);
    let hits = evaluate_rulebook(book, quote, &metrics);
    hits.iter().any(|(rule_id, hit_category)| {
        hit_category == category
            && book
                .rules
                .iter()
                .any(|r| r.id == *rule_id && r.severity == "Critical")
    })
}

// ── Public API（被 risk_taxonomy.rs facade 委托）──────────────────────

/// 对应 risk_taxonomy::canonical_category —— 签名不变。
pub fn canonical_category(finding: &RiskFinding) -> String {
    category_from_evidence(finding.source_quote.trim())
        .or_else(|| category_from_alias(&finding.category_code))
        .or_else(|| category_from_alias(&finding.risk_type))
        .map(str::to_string)
        .unwrap_or_else(|| {
            let fallback = if finding.category_code.trim().is_empty() {
                &finding.risk_type
            } else {
                &finding.category_code
            };
            normalized_code(fallback)
        })
}

/// 对应 risk_taxonomy::candidate_categories —— 签名不变。
pub fn candidate_categories(text: &str) -> Vec<&'static str> {
    let mut result = Vec::new();
    // ── 硬编码分支（已有，行为兼容） ──
    for segment in text.split(['\n', '。', '；']) {
        if let Some(category) = category_from_evidence(segment)
            && !result.contains(&category)
        {
            result.push(category);
        }
    }
    if let Some(category) = category_from_evidence(text)
        && !result.contains(&category)
    {
        result.push(category);
    }
    // ── YAML 分支（新增，取并集） ──
    for category in yaml_candidate_categories(text) {
        if !result.contains(&category) {
            result.push(category);
        }
    }
    result
}

/// 对应 risk_taxonomy::review_candidates_for_agent —— 签名不变。
pub fn review_candidates_for_agent(text: &str, agent: &str) -> Vec<&'static str> {
    candidate_categories(text)
        .into_iter()
        .filter(|category| owner_agent(category) == agent || agent == "RuleEngineAgent")
        .collect()
}

/// 对应 risk_taxonomy::is_actionable —— 签名不变。
pub fn is_actionable(finding: &RiskFinding) -> bool {
    if finding.no_risk {
        return true;
    }
    let quote = finding.source_quote.trim();
    if quote.is_empty() {
        return false;
    }
    let looks_like_heading = quote.chars().count() < 20
        && !contains_any(
            quote,
            &[
                "须",
                "必须",
                "不得",
                "不接受",
                "不予",
                "否则",
                "仅限",
                "无上限",
                "永久归",
                "承担",
                "得分",
                "加分",
            ],
        );
    if looks_like_heading {
        return false;
    }
    let negative = contains_any(
        quote,
        &[
            "未提及",
            "未发现",
            "未说明",
            "需要进一步确认",
            "建议进一步审查",
        ],
    );
    !(finding.severity == RiskSeverity::Info && negative)
}

/// 对应 risk_taxonomy::normalize_finding —— 签名不变。
pub fn normalize_finding(finding: &mut RiskFinding) {
    let category = canonical_category(finding);
    finding.category_code = category.clone();
    if let Some(name) = crate::rules::catalog::display_name(&category) {
        finding.risk_type = name.to_string();
    }

    let is_critical = !finding.no_risk
        && !finding.source_quote.trim().is_empty()
        && (critical_evidence(&category, finding.source_quote.trim())
            || yaml_is_critical(finding.source_quote.trim(), &category));
    finding.is_critical = is_critical;
    if is_critical {
        finding.severity = RiskSeverity::High;
        finding.critical_reason = format!(
            "命中重大问题分类 {}，且原文证据满足红线判定条件。",
            category
        );
    } else {
        finding.critical_reason.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::types::{FindingRole, RiskTier};

    fn finding(code: &str, quote: &str) -> RiskFinding {
        RiskFinding {
            risk_id: "R_001".into(),
            clause_ids: vec!["ch_1".into()],
            block_ids: vec![],
            highlight_rects: Vec::new(),
            agent: "test".into(),
            no_risk: false,
            severity: RiskSeverity::High,
            is_critical: true,
            critical_reason: "model decision".into(),
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

    #[test]
    fn alias_and_evidence_are_canonicalized() {
        let mut f = finding(
            "UNRELATED_CERTIFICATE",
            "供应商必须提供诚信示范企业荣誉证书，否则资格审查不通过。",
        );
        normalize_finding(&mut f);
        assert_eq!(f.category_code, "UNRELATED_CERT");
        assert_eq!(f.risk_type, "设置与履约无关的资格条件");
        assert!(f.is_critical);
    }

    #[test]
    fn ordinary_high_is_not_critical() {
        let mut f = finding(
            "EXCESSIVE_DEPOSIT",
            "供应商须缴纳相当于采购预算5%的投标保证金。",
        );
        normalize_finding(&mut f);
        assert!(!f.is_critical);
        assert!(f.critical_reason.is_empty());
    }

    #[test]
    fn empty_evidence_is_not_actionable() {
        let f = finding("OTHER", "");
        assert!(!is_actionable(&f));
    }

    #[test]
    fn heading_only_is_not_actionable_evidence() {
        let f = finding("OEM_AUTHORIZATION", "将厂家授权作为资格条件");
        assert!(!is_actionable(&f));
    }

    #[test]
    fn multi_issue_chunk_routes_candidates_to_owners() {
        let text = "供应商须提供采购人所在区县的同类服务案例，跨区域案例不作为有效业绩。\n\
                    投标人必须提交生产厂家针对本项目出具的授权函，否则投标无效。";
        assert_eq!(
            review_candidates_for_agent(text, "SemanticRiskAgent"),
            vec!["REGIONAL_PERFORMANCE"]
        );
        assert_eq!(
            review_candidates_for_agent(text, "ProcedureAgent"),
            vec!["OEM_AUTHORIZATION"]
        );
    }

    #[test]
    fn date_and_time_aliases_use_canonical_codes() {
        let mut date = finding(
            "DATE_CONFLICT",
            "投标截止时间为[日期]9时，同时规定[日期]17时后提交的文件一律拒收。",
        );
        normalize_finding(&mut date);
        assert_eq!(date.category_code, "CONFLICTING_DATES");

        let mut deadline = finding(
            "TIME_LIMIT",
            "供应商须在获取本条款后10日内递交投标文件，该期限不作顺延。",
        );
        normalize_finding(&mut deadline);
        assert_eq!(deadline.category_code, "SHORT_DEADLINE");
    }

    // ── normalize_finding 增强：覆盖 15 个类别的模拟数据 ───────────────
    //
    // 8 个应标 Critical（catalog.yaml critical_default: true）
    // 7 个不应标 Critical（catalog.yaml critical_default: false）

    fn critical_case(code: &str, quote: &str) -> (String, String, bool) {
        let mut f = finding(code, quote);
        normalize_finding(&mut f);
        (
            f.category_code.clone(),
            f.risk_type.clone(),
            f.is_critical,
        )
    }

    #[test]
    fn critical_coverage_8_should_be_critical() {
        // 5 个已覆盖（基线）
        let (c, _, crit) = critical_case(
            "LOCAL_REGISTRATION",
            "投标人须在本市注册成立三年以上，且在本市设有分支机构。",
        );
        assert_eq!(c, "LOCAL_REGISTRATION");
        assert!(crit);

        let (c, _, crit) = critical_case(
            "BRAND_LOCK",
            "本项目仅接受指定品牌XYZ型号的投标，其他品牌投标无效。",
        );
        assert_eq!(c, "BRAND_LOCK");
        assert!(crit);

        let (c, _, crit) = critical_case(
            "UNRELATED_CERT",
            "供应商须提供诚信示范企业荣誉证书，否则资格审查不通过。",
        );
        assert_eq!(c, "UNRELATED_CERT");
        assert!(crit);

        let (c, _, crit) = critical_case(
            "REGIONAL_PERFORMANCE",
            "供应商须提供本市同类项目业绩案例，外地业绩不作为有效业绩。",
        );
        assert_eq!(c, "REGIONAL_PERFORMANCE");
        assert!(crit);

        let (c, _, crit) = critical_case(
            "SCALE_THRESHOLD",
            "投标人注册资本不得低于5000万元，且近三年营业收入不少于1亿元。",
        );
        assert_eq!(c, "SCALE_THRESHOLD");
        assert!(crit);

        // 3 个 Day 3 增强目标：catalog 标了 critical_default: true 但当前漏标
        let (c, _, crit) = critical_case(
            "OEM_AUTHORIZATION",
            "投标人必须提交生产厂家针对本项目出具的专项授权函，否则投标无效。",
        );
        assert_eq!(c, "OEM_AUTHORIZATION");
        assert!(crit, "OEM_AUTHORIZATION 应为 Critical（catalog critical_default: true）");

        let (c, _, crit) = critical_case(
            "UNBOUNDED_IP",
            "供应商对采购人承担全部知识产权侵权责任，无限赔偿，无上限。",
        );
        assert_eq!(c, "UNBOUNDED_IP");
        assert!(crit, "UNBOUNDED_IP 应为 Critical（catalog critical_default: true）");

        let (c, _, crit) = critical_case(
            "UNILATERAL_CHANGE",
            "采购人有权单方无限变更需求，供应商不得调整合同费用和工期。",
        );
        assert_eq!(c, "UNILATERAL_CHANGE");
        assert!(crit, "UNILATERAL_CHANGE 应为 Critical（catalog critical_default: true）");
    }

    #[test]
    fn critical_coverage_7_should_not_be_critical() {
        // 7 个非 Critical 类别（catalog critical_default: false）
        let cases = vec![
            ("SHORT_DEADLINE", "投标人须在3日内递交投标文件。"),
            ("EXCESSIVE_DEPOSIT", "投标保证金不得超过估算价的5%。"),
            ("SUBJECTIVE_SCORING", "技术方案由评委酌情打分。"),
            ("LOCAL_AWARD", "本市获奖企业加2分。"),
            ("VAGUE_ACCEPTANCE", "验收由采购人满意为准。"),
            ("CONFLICTING_DATES", "投标截止日期与开标日期矛盾。"),
            ("UNCLEAR_PENALTY", "违约金由采购人自行决定。"),
        ];
        for (code, quote) in cases {
            let (c, _, crit) = critical_case(code, quote);
            assert_eq!(c, code, "{} 归一化失败", code);
            assert!(!crit, "{} 不应标 Critical（catalog critical_default: false）", code);
        }
    }

    /// 集成测试：模拟完整审核流程，验证 YAML 规则在 candidate_categories
    /// 和 normalize_finding 两个主链路接入点的行为。
    #[test]
    fn yaml_integration_full_pipeline_simulation() {
        // ── Stage 1: 条款级候选检测（candidate_categories） ──
        // 模拟 react_loop 对每个条款调用 review_candidates_for_agent。
        // 每条 clause 的预期候选集来自 YAML ∪ 硬编码。

        #[track_caller]
        fn check_candidates(clauses: &[(&str, &[&str])]) {
            for (i, (text, expected)) in clauses.iter().enumerate() {
                let candidates = crate::rules::engine::candidate_categories(text);
                for cat in *expected {
                    assert!(
                        candidates.contains(cat),
                        "Clause {}: 应包含 `{}`，实际候选集: {:?}",
                        i + 1,
                        cat,
                        candidates
                    );
                }
            }
        }

        // (clause_text, expected_candidates)
        check_candidates(&[
            // YAML-only: PRIC_EXCESSIVE_DEPOSIT_PCT_RE（regex 匹配 "6%的保证金"）
            ("投标人应缴纳采购预算总额6%的保证金", &["EXCESSIVE_DEPOSIT"]),
            // 硬编码: LOCAL_REGISTRATION（"本市注册须"）
            ("投标人须在本市注册成立三年以上，且在本市设有分支机构。", &["LOCAL_REGISTRATION"]),
            // YAML-only: TIME_SHORT_CLAUSE（regex 匹配 "仅5日递交投标文件"）
            ("仅5日递交投标文件", &["SHORT_DEADLINE"]),
            // YAML-only: DISC_BRAND_ALIAS（regex 匹配 "指定品牌"）
            ("指定品牌XYZ型号", &["BRAND_LOCK"]),
            // 硬编码: REGIONAL_PERFORMANCE + OEM_AUTHORIZATION（多条款文本）
            (
                "供应商须提供采购人所在区县的同类服务案例，跨区域案例不作为有效业绩。\n\
                 投标人必须提交生产厂家针对本项目出具的授权函，否则投标无效。",
                &["REGIONAL_PERFORMANCE", "OEM_AUTHORIZATION"],
            ),
            // YAML + 硬编码并集: PRIC_SUBJECTIVE_RE（regex）+ 硬编码关键词
            ("技术方案由评委综合判断优劣情况", &["SUBJECTIVE_SCORING"]),
            // ── 边界：空条款 → 无候选 ──
            ("", &[]),
            // ── 边界：无匹配条款 → 无候选 ──
            ("这是一般商务条款，不涉及风险。", &[]),
            // ── 边界：YAML chapter_keywords + all_match（DISC_LOCAL_REG_CITY）──
            ("投标人资格：须在本市注册的企业", &["LOCAL_REGISTRATION"]),
            // ── 边界：YAML absence 模式（SAFE_ACCEPTANCE_ABSENCE → VAGUE_ACCEPTANCE）──
            // 施工资质章节中未提及"安全生产许可证"即触发 absence
            (
                "安全生产施工资质要求：施工单位须具备相关施工资质。",
                &["VAGUE_ACCEPTANCE"],
            ),
            // ── 边界：YAML field_compare（PRIC_EXCESSIVE_DEPOSIT_RATIO: deposit_ratio > 0.02）──
            ("投标保证金不得超过估算价的5%", &["EXCESSIVE_DEPOSIT"]),
        ]);

        // ── Stage 2: 证据分类 + Critical 判定（normalize_finding） ──
        // 模拟 coordinator 对每条 finding 的归一化处理。

        #[track_caller]
        fn check_findings(cases: &[(&str, &str, &str, bool)]) {
            for (i, (code, quote, expected_cat, expected_crit)) in cases.iter().enumerate() {
                let mut f = finding(code, quote);
                crate::rules::engine::normalize_finding(&mut f);
                assert_eq!(
                    &f.category_code, expected_cat,
                    "Finding {}: category_code 不符合预期（quote={}）",
                    i + 1, quote
                );
                assert_eq!(
                    f.is_critical, *expected_crit,
                    "Finding {}: is_critical={} 不符合预期（category={}, quote={}）",
                    i + 1, f.is_critical, f.category_code, quote
                );
            }
        }

        // (category_code, source_quote, expected_normalized_category, expected_critical)
        check_findings(&[
            // YAML Critical: DISC_BRAND_ALIAS（severity=Critical）+ 硬编码也命中
            ("BRAND_LOCK", "指定品牌XYZ型号的投标", "BRAND_LOCK", true),
            // YAML Medium: TIME_SHORT_CLAUSE（severity=Medium）
            ("SHORT_DEADLINE", "仅5日递交投标文件", "SHORT_DEADLINE", false),
            // YAML High: PRIC_EXCESSIVE_DEPOSIT_PCT_RE（severity=High, not Critical）
            ("EXCESSIVE_DEPOSIT", "投标人应缴纳采购预算总额6%的保证金", "EXCESSIVE_DEPOSIT", false),
            // 硬编码 critical_evidence: LOCAL_REGISTRATION
            ("LOCAL_REGISTRATION", "投标人须在本市注册成立三年以上，且在本市设有分支机构。", "LOCAL_REGISTRATION", true),
            // YAML 无匹配 + 硬编码也无匹配：回退到 alias 归一化
            ("DATE_CONFLICT", "投标截止时间为[日期]9时，同时规定[日期]17时后提交的文件一律拒收。", "CONFLICTING_DATES", false),
        ]);

        // ── Stage 3: 边界情况（normalize_finding 需要特殊构造） ──

        // Edge: no_risk=true → 不应标记 Critical
        let mut f = finding("SHORT_DEADLINE", "仅5日递交投标文件");
        f.no_risk = true;
        crate::rules::engine::normalize_finding(&mut f);
        assert!(!f.is_critical, "no_risk=true 不应标记为 Critical");

        // Edge: 空 source_quote → 不应标记 Critical
        let mut f = finding("SHORT_DEADLINE", "");
        crate::rules::engine::normalize_finding(&mut f);
        assert!(!f.is_critical, "空 source_quote 不应标记为 Critical");
    }

    /// 汇总报告：输出 15 类的覆盖矩阵，供 bin/test_rules 离线查看。
    /// 这个测试本身就是模拟数据集的"快照"。
    // ── YAML 接入主链路（TDD 测试用例）────────────────────────────────

    #[test]
    fn yaml_candidate_catches_excessive_deposit_without_投标保证金_keyword() {
        // YAML rule PRIC_EXCESSIVE_DEPOSIT_PCT_RE 匹配 "6%的保证金"（regex）
        // 硬编码 category_from_evidence 要求 "投标保证金" 关键词，不存在时不命中
        let text = "投标人应缴纳采购预算总额6%的保证金";
        let candidates = candidate_categories(text);
        assert!(
            candidates.contains(&"EXCESSIVE_DEPOSIT"),
            "YAML 应捕获 EXCESSIVE_DEPOSIT（6%的保证金，硬编码需要投标保证金）"
        );
    }

    #[test]
    fn yaml_candidate_preserves_hardcoded_results() {
        // 硬编码应仍然捕获 LOCAL_REGISTRATION（回归测试）
        let text = "投标人须在本市注册成立三年以上，且在本市设有分支机构。";
        let candidates = candidate_categories(text);
        assert!(candidates.contains(&"LOCAL_REGISTRATION"));
    }

    #[test]
    fn normalize_finding_yaml_critical_supplements_hardcoded() {
        // DISC_BRAND_ALIAS（severity=Critical, no chapter_keywords）
        // 应通过 YAML 路径标记为 Critical
        let mut f = finding("BRAND_LOCK", "指定品牌XYZ型号的投标");
        normalize_finding(&mut f);
        assert_eq!(f.category_code, "BRAND_LOCK");
        assert!(f.is_critical, "YAML rule DISC_BRAND_ALIAS severity=Critical 应标记为 Critical");
    }

    #[test]
    fn normalize_finding_yaml_does_not_cause_false_critical_for_non_critical() {
        // SHORT_DEADLINE YAML 规则 severity=Medium，不应误标 Critical
        let mut f = finding("SHORT_DEADLINE", "投标人须在3日内递交投标文件。");
        normalize_finding(&mut f);
        assert!(!f.is_critical, "SHORT_DEADLINE YAML severity=Medium 不应标记为 Critical");
    }

    #[test]
    fn critical_coverage_summary_report() {
        let critical_cases = vec![
            ("LOCAL_REGISTRATION", "投标人须在本市注册成立三年以上，且在本市设有分支机构。"),
            ("BRAND_LOCK", "本项目仅接受指定品牌XYZ型号的投标，其他品牌投标无效。"),
            ("UNRELATED_CERT", "供应商须提供诚信示范企业荣誉证书，否则资格审查不通过。"),
            ("REGIONAL_PERFORMANCE", "供应商须提供本市同类项目业绩案例，外地业绩不作为有效业绩。"),
            ("SCALE_THRESHOLD", "投标人注册资本不得低于5000万元，且近三年营业收入不少于1亿元。"),
            ("OEM_AUTHORIZATION", "投标人必须提交生产厂家针对本项目出具的专项授权函，否则投标无效。"),
            ("UNBOUNDED_IP", "供应商对采购人承担全部知识产权侵权责任，无限赔偿，无上限。"),
            ("UNILATERAL_CHANGE", "采购人有权单方无限变更需求，供应商不得调整合同费用和工期。"),
        ];
        let non_critical_cases = vec![
            ("SHORT_DEADLINE", "投标人须在3日内递交投标文件。"),
            ("EXCESSIVE_DEPOSIT", "投标保证金不得超过估算价的5%。"),
            ("SUBJECTIVE_SCORING", "技术方案由评委酌情打分。"),
            ("LOCAL_AWARD", "本市获奖企业加2分。"),
            ("VAGUE_ACCEPTANCE", "验收由采购人满意为准。"),
            ("CONFLICTING_DATES", "投标截止日期与开标日期矛盾。"),
            ("UNCLEAR_PENALTY", "违约金由采购人自行决定。"),
        ];

        let mut critical_hit = 0;
        let mut critical_miss = 0;
        let mut non_critical_ok = 0;
        let mut non_critical_false_positive = 0;
        let mut report = String::new();
        report.push_str("\n=== Critical 覆盖矩阵（15 类模拟数据） ===\n");
        report.push_str("类别                    | 应Critical | 实际Critical | 状态\n");
        report.push_str("-----------------------|------------|--------------|------\n");

        for (code, quote) in &critical_cases {
            let (_, _, crit) = critical_case(code, quote);
            let status = if crit { "✓ HIT" } else { "✗ MISS" };
            if crit { critical_hit += 1; } else { critical_miss += 1; }
            report.push_str(&format!(
                "{:<23} | {:<10} | {:<12} | {}\n",
                code, "是", if crit { "是" } else { "否" }, status
            ));
        }
        for (code, quote) in &non_critical_cases {
            let (_, _, crit) = critical_case(code, quote);
            let status = if !crit { "✓ OK" } else { "✗ FP" };
            if !crit { non_critical_ok += 1; } else { non_critical_false_positive += 1; }
            report.push_str(&format!(
                "{:<23} | {:<10} | {:<12} | {}\n",
                code, "否", if crit { "是" } else { "否" }, status
            ));
        }
        report.push_str(&format!("\n汇总：应Critical 命中 {}/8，漏标 {}/8\n", critical_hit, critical_miss));
        report.push_str(&format!("      非Critical 正确 {}/7，误报 {}/7\n", non_critical_ok, non_critical_false_positive));
        let recall = critical_hit as f64 / 8.0;
        let precision = if critical_hit + non_critical_false_positive > 0 {
            critical_hit as f64 / (critical_hit + non_critical_false_positive) as f64
        } else { 1.0 };
        report.push_str(&format!("      Critical Recall={:.0}%  Precision={:.0}%\n", recall * 100.0, precision * 100.0));
        eprintln!("{}", report);

        // 硬性验收：8/8 Critical 命中，0/7 非Critical 误报
        assert_eq!(critical_hit, 8, "Critical Recall 必须 8/8 = 100%");
        assert_eq!(critical_miss, 0, "不允许漏标 Critical");
        assert_eq!(non_critical_false_positive, 0, "不允许非 Critical 误报为 Critical");
    }
}
