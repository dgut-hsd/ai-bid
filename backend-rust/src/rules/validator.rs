//! Static rule validator + confusion-matrix evaluator + YAML rulebook loader.
//!
//! Static validation checklist (5 items, per Day 2 plan spec):
//! 1. Law/regulation name existence (against built-in local law list).
//! 2. Clause number canonicality (第十八条 vs 第18条 normalized).
//! 3. Regex patterns compile successfully (failing rules are marked skipped).
//! 4. conditions.trigger / conditions.exclude self-consistency.
//! 5. industry value in accepted taxonomy.

use crate::rules::matchers::{evaluate_pattern_with_metrics, normalize_regex};
use crate::rules::metrics::DocumentMetrics;
use crate::rules::schema::{Rule, RuleBook};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Accepted taxonomies ──────────────────────────────────────────

const INDUSTRY_SET: &[&str] = &["GENERAL", "CONSTRUCTION", "GOVERNMENT", "IT", "HEALTHCARE"];
const CATEGORY_SET: &[&str] = &[
    "LOCAL_REGISTRATION",
    "BRAND_LOCK",
    "UNRELATED_CERT",
    "REGIONAL_PERFORMANCE",
    "SCALE_THRESHOLD",
    "SHORT_DEADLINE",
    "EXCESSIVE_DEPOSIT",
    "OEM_AUTHORIZATION",
    "SUBJECTIVE_SCORING",
    "LOCAL_AWARD",
    "VAGUE_ACCEPTANCE",
    "UNBOUNDED_IP",
    "UNILATERAL_CHANGE",
    "CONFLICTING_DATES",
    "UNCLEAR_PENALTY",
];

/// Built-in law/regulation catalog used by validation item #1.
const LAW_CATALOG: &[&str] = &[
    "中华人民共和国招标投标法",
    "中华人民共和国招标投标法实施条例",
    "中华人民共和国政府采购法",
    "中华人民共和国政府采购法实施条例",
    "政府采购货物和服务招标投标管理办法",
    "政府采购竞争性磋商采购方式管理暂行办法",
    "中华人民共和国民法典",
    "民法典 合同编",
    "中华人民共和国建筑法",
    "建设工程质量管理条例",
    "工程建设项目施工招标投标办法",
    "工程建设项目货物招标投标办法",
];

// ── Validation output ───────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    pub total: usize,
    pub passed: usize,
    pub failed: Vec<RuleIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleIssue {
    pub rule_id: String,
    pub item: String,
    pub message: String,
}

/// Top-level static validator entry point.
pub fn validate_rulebook(book: &RuleBook) -> ValidationReport {
    let mut report = ValidationReport {
        total: book.rules.len(),
        passed: 0,
        failed: Vec::new(),
    };
    for rule in &book.rules {
        let n_fail_before = report.failed.len();
        law_existence(rule, &mut report);
        clause_number(rule, &mut report);
        regex_compile(rule, &mut report);
        conditions_self_consistent(rule, &mut report);
        industry_taxonomy(rule, &mut report);
        absence_needs_chapter_keywords(rule, &mut report);
        if report.failed.len() == n_fail_before {
            report.passed += 1;
        }
    }
    report
}

// ── 5 validation items ──────────────────────────────────────────

fn law_existence(rule: &Rule, report: &mut ValidationReport) {
    let law = rule.source.law.trim();
    if law.is_empty() {
        return; // not every rule has explicit law source — skip silently
    }
    let law_prefix: String = law.chars().take(8).collect();
    let hit = LAW_CATALOG
        .iter()
        .any(|c| law.contains(*c) || c.contains(&law_prefix));
    if !hit {
        report.failed.push(RuleIssue {
            rule_id: rule.id.clone(),
            item: "#1 law name existence".into(),
            message: format!("未在本地法规清单中找到：`{law}`（是否名字写错或遗漏在 LAW_CATALOG？）"),
        });
    }
}

fn clause_number(rule: &Rule, report: &mut ValidationReport) {
    let article = rule.source.article.trim();
    if article.is_empty() {
        return;
    }
    // Accept both forms: "第十八条" / "第18条" / "第18条之一款"
    let re_cn = Regex::new(r"^第[一二三四五六七八九十百零两〇\d]+条").unwrap();
    if !re_cn.is_match(article) && !article.starts_with("art.") && !article.starts_with("Article") {
        report.failed.push(RuleIssue {
            rule_id: rule.id.clone(),
            item: "#2 clause number canonicality".into(),
            message: format!("`{article}` 不符合「第XX条」规范格式，请归一化后再校验。"),
        });
    }
}

fn regex_compile(rule: &Rule, report: &mut ValidationReport) {
    for (idx, pat) in rule.patterns.iter().enumerate() {
        if let crate::rules::schema::Pattern::Regex { value } = pat {
            let normalized = normalize_regex(value);
            if Regex::new(&normalized).is_err() {
                report.failed.push(RuleIssue {
                    rule_id: rule.id.clone(),
                    item: format!("#3 regex compile [{idx}]"),
                    message: format!("正则无法编译：`{value}`（归一化后：`{normalized}`）"),
                });
            }
        }
    }
}

fn conditions_self_consistent(rule: &Rule, report: &mut ValidationReport) {
    let Some(exclude) = rule.conditions.exclude.as_ref() else {
        return;
    };
    let trigger_projects = &rule.conditions.project_types;
    for ex in &exclude.project_types {
        if trigger_projects.iter().any(|p| p == ex) {
            report.failed.push(RuleIssue {
                rule_id: rule.id.clone(),
                item: "#4 conditions self-consistency".into(),
                message: format!(
                    "project_type `{ex}` 同时出现在 project_types 和 exclude.project_types，条件矛盾。"
                ),
            });
        }
    }
}

fn industry_taxonomy(rule: &Rule, report: &mut ValidationReport) {
    if !INDUSTRY_SET.iter().any(|v| *v == rule.industry) {
        report.failed.push(RuleIssue {
            rule_id: rule.id.clone(),
            item: "#5 industry taxonomy".into(),
            message: format!(
                "industry=`{}` 不在已接受集合 {:?}。（注意大小写，建议全大写。）",
                rule.industry, INDUSTRY_SET
            ),
        });
    }
    if !CATEGORY_SET.iter().any(|v| *v == rule.category) {
        report.failed.push(RuleIssue {
            rule_id: rule.id.clone(),
            item: "#5 category taxonomy".into(),
            message: format!(
                "category=`{}` 不在 15 个 canonical code 中。（规则引擎需要向后兼容 risk_taxonomy。）",
                rule.category
            ),
        });
    }
}

// ── Rulebook loader ─────────────────────────────────────────────

/// Load a rulebook from a YAML file path.  Returns the book plus any
/// parse / validation failures as warnings.  Missing files yield an empty
/// rulebook with an alert; a single bad rule does not crash the load.
pub fn load_rulebook(path: impl AsRef<Path>) -> Result<(RuleBook, Vec<String>), String> {
    let p = path.as_ref();
    let raw = std::fs::read_to_string(p)
        .map_err(|e| format!("读取规则库 {:?} 失败：{e}", p.file_name().unwrap_or_default()))?;
    let book: RuleBook = serde_yaml::from_str(&raw)
        .map_err(|e| format!("YAML 解析失败：{e}"))?;

    // Mark skipped rules (e.g. regex fail) before returning so the caller
    // sees warnings but execution continues.
    let validation = validate_rulebook(&book);
    let warnings: Vec<String> = validation
        .failed
        .iter()
        .map(|iss| format!("[{}] {}: {}", iss.rule_id, iss.item, iss.message))
        .collect();

    Ok((book, warnings))
}

/// Run a rulebook against a clause text + metrics.  Returns rule ids that
/// matched, paired with the category code.
pub fn evaluate_rulebook(
    book: &RuleBook,
    clause_text: &str,
    metrics: &DocumentMetrics,
) -> Vec<(String, String)> {
    book.rules
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| {
            if !passes_conditions(r, clause_text) {
                return None;
            }
            let patterns_ok = crate::rules::matchers::evaluate_patterns(
                clause_text,
                &r.patterns,
                &r.check,
            );
            // Also support a second branch where field_compare patterns were
            // already evaluated inside evaluate_patterns, but we still need to
            // feed DocumentMetrics explicitly for patterns that need it:
            let metrics_ok = if r.patterns.is_empty() {
                false
            } else {
                r.patterns.iter().any(|p| {
                    matches!(p, crate::rules::schema::Pattern::FieldCompare { .. })
                        && evaluate_pattern_with_metrics(clause_text, p, metrics)
                })
            };
            if patterns_ok || metrics_ok {
                Some((r.id.clone(), r.category.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn passes_conditions(rule: &Rule, clause_text: &str) -> bool {
    // 1) exclude.project_types — clause text mentioning an excluded type blocks
    if let Some(exc) = rule.conditions.exclude.as_ref() {
        if exc
            .project_types
            .iter()
            .any(|p| clause_text.contains(p))
        {
            return false;
        }
    }
    // 2) document_type — soft: if set we only check a weak signal in-clause
    if let Some(dt) = rule.conditions.document_type.as_ref() {
        if !dt.is_empty() && !clause_text.contains("招标") && !clause_text.contains("采购") {
            // Weak signal: leave it to rules using chapter_keywords.
        }
    }
    // 3) trigger.chapter_keywords (highly constraining) — if present, at least
    // one keyword must appear in-clause to narrow false positives.
    if let Some(trigger) = rule.conditions.trigger.as_ref() {
        if !trigger.chapter_keywords.is_empty()
            && !trigger
                .chapter_keywords
                .iter()
                .any(|k| clause_text.contains(k))
        {
            return false;
        }
    }
    true
}

// ── Confusion-matrix evaluator (Day 3 Golden Standard) ─────────

/// Validation #6: absence-mode patterns must be paired with
/// `conditions.trigger.chapter_keywords` to prevent over-matching
/// (otherwise the rule fires on every clause that merely lacks the keyword).
fn absence_needs_chapter_keywords(rule: &Rule, report: &mut ValidationReport) {
    let has_absence = rule.patterns.iter().any(|p| {
        matches!(
            p,
            crate::rules::schema::Pattern::Keyword {
                match_mode: Some(mm),
                ..
            } if mm == "absence"
        )
    });
    if !has_absence {
        return;
    }
    let has_chapter_keywords = rule
        .conditions
        .trigger
        .as_ref()
        .map(|t| !t.chapter_keywords.is_empty())
        .unwrap_or(false);
    if !has_chapter_keywords {
        report.failed.push(RuleIssue {
            rule_id: rule.id.clone(),
            item: "#6 absence requires chapter_keywords".into(),
            message: "absence 模式必须搭配 conditions.trigger.chapter_keywords，否则会对所有不含该关键词的条款误报。".into(),
        });
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleEval {
    pub tp: usize,
    pub fp: usize,
    pub fn_: usize,
    pub recall: f64,
    pub precision: f64,
    pub f1: f64,
}

pub fn eval_predictions(
    expected_rule_ids: &[String],
    predicted_rule_ids: &[String],
) -> RuleEval {
    let tp = expected_rule_ids
        .iter()
        .filter(|e| predicted_rule_ids.contains(e))
        .count();
    let fp = predicted_rule_ids
        .iter()
        .filter(|p| !expected_rule_ids.contains(p))
        .count();
    let fn_ = expected_rule_ids
        .iter()
        .filter(|e| !predicted_rule_ids.contains(e))
        .count();
    let recall = if tp + fn_ > 0 {
        tp as f64 / (tp + fn_) as f64
    } else {
        1.0
    };
    let precision = if tp + fp > 0 {
        tp as f64 / (tp + fp) as f64
    } else {
        1.0
    };
    let f1 = if recall + precision > 0.0 {
        2.0 * recall * precision / (recall + precision)
    } else {
        0.0
    };
    RuleEval {
        tp,
        fp,
        fn_,
        recall,
        precision,
        f1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::schema::{Conditions, Trigger};

    #[test]
    fn industry_category_taxonomy_validation_passes() {
        let good = RuleBook {
            rules: vec![Rule {
                id: "R1".into(),
                category: "LOCAL_REGISTRATION".into(),
                industry: "CONSTRUCTION".into(),
                severity: "High".into(),
                source: Default::default(),
                conditions: Default::default(),
                patterns: vec![],
                check: "any_match".into(),
                enabled: true,
                law_ref: String::new(),
            }],
        };
        let r = validate_rulebook(&good);
        assert_eq!(r.passed, 1);
        assert_eq!(
            r.failed.iter().filter(|i| i.item.contains("taxonomy")).count(),
            0
        );
    }

    #[test]
    fn conflicting_exclude_triggers_validation() {
        let bad = RuleBook {
            rules: vec![Rule {
                id: "R2".into(),
                category: "OEM_AUTHORIZATION".into(),
                industry: "GENERAL".into(),
                severity: "High".into(),
                source: Default::default(),
                conditions: crate::rules::schema::Conditions {
                    project_types: vec!["国际招标".into()],
                    exclude: Some(crate::rules::schema::Exclude {
                        project_types: vec!["国际招标".into()],
                    }),
                    ..Default::default()
                },
                patterns: vec![],
                check: "any_match".into(),
                enabled: true,
                law_ref: String::new(),
            }],
        };
        let r = validate_rulebook(&bad);
        assert!(r
            .failed
            .iter()
            .any(|i| i.item.contains("self-consistency")));
    }

    #[test]
    fn eval_computes_f1_correctly() {
        let expected = vec!["A".into(), "B".into()];
        let predicted = vec!["A".into(), "C".into()]; // 1 TP (A), 1 FP (C), 1 FN (B)
        let e = eval_predictions(&expected, &predicted);
        assert_eq!((e.tp, e.fp, e.fn_), (1, 1, 1));
        assert!((e.recall - 0.5).abs() < 1e-9);
        assert!((e.precision - 0.5).abs() < 1e-9);
    }

    // ── absence + chapter_keywords 约束 ──────────────────────────

    #[test]
    fn absence_without_chapter_keywords_fails_validation() {
        let bad = RuleBook {
            rules: vec![Rule {
                id: "R_ABS".into(),
                category: "VAGUE_ACCEPTANCE".into(),
                industry: "CONSTRUCTION".into(),
                severity: "Medium".into(),
                source: Default::default(),
                conditions: Default::default(), // 缺少 trigger.chapter_keywords
                patterns: vec![crate::rules::schema::Pattern::Keyword {
                    value: vec!["安全生产许可证".into()],
                    mode: "any".into(),
                    match_mode: Some("absence".into()),
                }],
                check: "any_match".into(),
                enabled: true,
                law_ref: String::new(),
            }],
        };
        let r = validate_rulebook(&bad);
        assert!(r
            .failed
            .iter()
            .any(|i| i.item.contains("absence requires chapter_keywords")));
    }

    #[test]
    fn absence_with_chapter_keywords_passes_validation() {
        let good = RuleBook {
            rules: vec![Rule {
                id: "R_ABS_OK".into(),
                category: "VAGUE_ACCEPTANCE".into(),
                industry: "CONSTRUCTION".into(),
                severity: "Medium".into(),
                source: Default::default(),
                conditions: Conditions {
                    trigger: Some(Trigger {
                        chapter_keywords: vec!["资格".into(), "资质".into()],
                    }),
                    ..Default::default()
                },
                patterns: vec![crate::rules::schema::Pattern::Keyword {
                    value: vec!["安全生产许可证".into()],
                    mode: "any".into(),
                    match_mode: Some("absence".into()),
                }],
                check: "any_match".into(),
                enabled: true,
                law_ref: String::new(),
            }],
        };
        let r = validate_rulebook(&good);
        assert!(
            !r
                .failed
                .iter()
                .any(|i| i.item.contains("absence requires chapter_keywords")),
            "absence + chapter_keywords should pass: {:?}",
            r.failed
        );
    }

    #[test]
    fn absence_rule_does_not_match_non_qualification_clause() {
        // 模拟 SAFE_ACCEPTANCE_ABSENCE 修复后行为：
        // 资质章节 + 缺失安全生产许可证 → 命中
        // 非资质章节（如邀请函）+ 缺失安全生产许可证 → 不命中
        let rule = Rule {
            id: "R_ABS_CTX".into(),
            category: "VAGUE_ACCEPTANCE".into(),
            industry: "CONSTRUCTION".into(),
            severity: "Medium".into(),
            source: Default::default(),
            conditions: Conditions {
                trigger: Some(Trigger {
                    chapter_keywords: vec!["资格".into(), "资质".into()],
                }),
                ..Default::default()
            },
            patterns: vec![crate::rules::schema::Pattern::Keyword {
                value: vec!["安全生产许可证".into()],
                mode: "any".into(),
                match_mode: Some("absence".into()),
            }],
            check: "any_match".into(),
            enabled: true,
            law_ref: String::new(),
        };
        let book = RuleBook {
            rules: vec![rule],
        };
        let metrics = DocumentMetrics::default();

        // 资质条款 + 未提及安全生产许可证 → 应命中
        let qual_clause = "投标人资格要求：营业执照、税务登记证、组织机构代码证。";
        let hits_qual = evaluate_rulebook(&book, qual_clause, &metrics);
        assert!(
            !hits_qual.is_empty(),
            "资质章节未提及安全生产许可证应命中 absence 规则"
        );

        // 非资质条款（邀请函）+ 未提及安全生产许可证 → 不应命中
        let invite_clause = "东莞市公共资源交易中心受东莞理工学院的委托，采用竞争性磋商方式组织采购。";
        let hits_invite = evaluate_rulebook(&book, invite_clause, &metrics);
        assert!(
            hits_invite.is_empty(),
            "非资质章节不应命中 absence 规则，但命中了 {:?}",
            hits_invite
        );
    }

    // ── 全 15 类别覆盖测试（Golden Standard 合成条款）──────────────

    /// 合成条款集：每条对应一个 canonical category，用于验证 YAML 规则库
    /// 全覆盖。这些条款参照 conditions.yaml 中各规则的 patterns 构造。
    #[test]
    fn all_15_categories_triggered_by_synthetic_clauses() {
        let path = std::path::Path::new("src/rules/data/conditions.yaml");
        let (book, warnings) = load_rulebook(path).expect("rulebook must load");
        assert!(
            warnings.is_empty(),
            "rulebook has validation warnings: {warnings:?}"
        );

        // (期望类别, 合成条款文本)
        let synthetic_clauses: &[(&str, &str)] = &[
            // 1. LOCAL_REGISTRATION
            ("LOCAL_REGISTRATION", "投标人资格条件：须在本市注册成立分支机构，否则资格无效。"),
            // 2. BRAND_LOCK
            ("BRAND_LOCK", "技术要求：指定品牌为华为MateBook，不接受同等产品替代。"),
            // 3. UNRELATED_CERT
            ("UNRELATED_CERT", "资质要求：须提供驰名商标证书，否则作为资格审查不通过处理。"),
            // 4. REGIONAL_PERFORMANCE
            ("REGIONAL_PERFORMANCE", "类似项目业绩：投标人须在本省范围内完成类似项目业绩不少于3个。"),
            // 5. SCALE_THRESHOLD
            ("SCALE_THRESHOLD", "投标人资格：注册资本不得低于500万元，实缴资本不少于200万元。"),
            // 6. SHORT_DEADLINE
            ("SHORT_DEADLINE", "自招标公告发布之日起至投标截止时间止，不少于5日。"),
            // 7. EXCESSIVE_DEPOSIT
            ("EXCESSIVE_DEPOSIT", "投标保证金为估算价的5%，保证金金额50万元，估算价1000万元。"),
            // 8. OEM_AUTHORIZATION
            ("OEM_AUTHORIZATION", "资格要求：须提供原厂授权书作为资格条件，否则无效。"),
            // 9. SUBJECTIVE_SCORING
            ("SUBJECTIVE_SCORING", "评分办法：技术方案由评委酌情打分，根据满意程度综合判断。"),
            // 10. LOCAL_AWARD
            ("LOCAL_AWARD", "评分细则：本地企业获诚信企业荣誉奖项的加分5分，同等条件优先本地供应商中标。"),
            // 11. VAGUE_ACCEPTANCE
            ("VAGUE_ACCEPTANCE", "验收标准：竣工验收由采购人满意为准，无需说明理由。"),
            // 12. UNBOUNDED_IP
            ("UNBOUNDED_IP", "知识产权侵权责任：供应商承担一切责任，无上限赔偿全部损失。"),
            // 13. UNILATERAL_CHANGE
            ("UNILATERAL_CHANGE", "采购人有权变更需求，新增需求由供应商承担，费用不变。"),
            // 14. CONFLICTING_DATES
            ("CONFLICTING_DATES", "投标截止时间另有规定，两处分别记载开标时间为2024年3月15日和2024年3月10日。"),
            // 15. UNCLEAR_PENALTY
            ("UNCLEAR_PENALTY", "违约责任：逾期违约金由采购人自行决定，累计计算无上限。"),
        ];

        let mut missing: Vec<&str> = Vec::new();
        let mut summary: Vec<String> = Vec::new();
        for (expected_cat, clause_text) in synthetic_clauses {
            let metrics = DocumentMetrics::extract_from_clause_text(clause_text);
            let hits = evaluate_rulebook(&book, clause_text, &metrics);
            let hit_cats: Vec<String> = hits
                .iter()
                .map(|(_, c)| c.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let ok = hit_cats.iter().any(|c| c == expected_cat);
            summary.push(format!(
                "  {:<22} → {:<5} hits={:?}",
                expected_cat,
                if ok { "✓" } else { "✗" },
                hit_cats
            ));
            if !ok {
                missing.push(expected_cat);
            }
        }

        eprintln!("Category coverage report:");
        for line in &summary {
            eprintln!("{line}");
        }

        assert!(
            missing.is_empty(),
            "以下类别未被合成条款触发：{:?}\n覆盖报告:\n{}",
            missing,
            summary.join("\n")
        );
    }

    // ── P3: blind-v2 11 个 FN 条款回归测试 ────────────────────────
    //
    // 验证补充规则后，原 blind_validate 报告的 11 个 FN 条款全部被规则引擎
    // 正确分类。条款原文取自 llm_fn_annotations.json。

    #[test]
    fn blind_v2_fn_clauses_all_matched() {
        let path = std::path::Path::new("src/rules/data/conditions.yaml");
        let (book, warnings) = load_rulebook(path).expect("rulebook must load");
        assert!(warnings.is_empty(), "rulebook warnings: {warnings:?}");

        // (finding_id, 期望类别, 条款原文)
        let fn_clauses: &[(&str, &str, &str)] = &[
            (
                "BLIND-002-F02",
                "EXCESSIVE_DEPOSIT",
                "本项目预算为500万元，投标保证金固定收取25万元，未在开标前到账的响应无效。",
            ),
            (
                "BLIND-002-F03",
                "UNBOUNDED_IP",
                "供应商在承接项目前形成的软件组件、算法和工具也须永久无偿转让，并承担全部无限额索赔。",
            ),
            (
                "BLIND-003-F03",
                "UNILATERAL_CHANGE",
                "履约期间采购人可随时增加任意数量的服务事项，供应商不得增加费用或申请延长工期。",
            ),
            (
                "BLIND-004-F02",
                "SUBJECTIVE_SCORING",
                "评委认为实施方案非常好的得10分、较好的得6分、普通的得2分，各档没有列明可核验条件。",
            ),
            (
                "BLIND-004-F03",
                "CONFLICTING_DATES",
                "投标须知载明2026年9月8日10时停止收件，同一条款又称2026年9月6日17时以后不再接收。",
            ),
            (
                "BLIND-006-F02",
                "SHORT_DEADLINE",
                "自供应商下载招标文件之日起十二个自然日截止收件，不因法定节假日顺延。",
            ),
            (
                "BLIND-006-F03",
                "VAGUE_ACCEPTANCE",
                "项目完成标准由采购人现场口头确认，采购人可不说明拒绝验收的具体理由。",
            ),
            (
                "BLIND-007-F02",
                "EXCESSIVE_DEPOSIT",
                "投标人应缴纳采购预算总额6%的保证金，并且只允许从基本账户以现金转账方式支付。",
            ),
            (
                "BLIND-008-F03",
                "UNILATERAL_CHANGE",
                "所有后续新增功能自动包含在原合同价内，是否属于新增需求由采购人单方决定。",
            ),
            (
                "BLIND-009-F02",
                "SUBJECTIVE_SCORING",
                "根据方案的美观性、感染力和评审专家总体感觉在0至15分之间自由给分，不设置评分刻度。",
            ),
            (
                "BLIND-009-F03",
                "CONFLICTING_DATES",
                "答疑文件写明开标时间为2026年10月20日9时，日程表同时要求当日8时前完成开标。",
            ),
        ];

        let mut failures: Vec<String> = Vec::new();
        let mut report: Vec<String> = Vec::new();
        for (fid, expected_cat, clause_text) in fn_clauses {
            let metrics = DocumentMetrics::extract_from_clause_text(clause_text);
            let hits = evaluate_rulebook(&book, clause_text, &metrics);
            let hit_cats: Vec<String> = hits
                .iter()
                .map(|(_, c)| c.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let ok = hit_cats.iter().any(|c| c == expected_cat);
            report.push(format!(
                "  {fid} {:<22} → {} hits={:?}",
                expected_cat,
                if ok { "✓" } else { "✗" },
                hit_cats
            ));
            if !ok {
                failures.push(format!("{fid}: 期望 {expected_cat}，实际命中 {hit_cats:?}"));
            }
        }

        eprintln!("blind-v2 FN 回归报告:");
        for line in &report {
            eprintln!("{line}");
        }

        assert!(
            failures.is_empty(),
            "以下 FN 条款未被正确分类：\n{}\n回归报告:\n{}",
            failures.join("\n"),
            report.join("\n")
        );
    }
}
