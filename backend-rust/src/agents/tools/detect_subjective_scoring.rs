//! `detect_subjective_scoring` 工具 — 主观评分检测。
//!
//! 根据《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条，
//! 检测评分标准条款中是否存在主观性表述，判断是否违反"评审因素应当量化"
//! 的法定要求。本工具进行纯关键词检测与区间计算，不访问外部 I/O。
//!
//! ## 核心逻辑
//!
//! 1. 关键词检测：评委酌情/自行掌握/综合判断/满意程度/优良/酌情打分/灵活掌握
//! 2. 区间跨度检测：(max-min) > 5 分标记过宽，(max-min) > 10 分标记严重
//! 3. 量化细则缺失："优良中差"等定性描述但未给出具体量化细则
//!
//! ## 法条依据
//!
//! - 《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条：
//!   评审因素应当细化和量化，且与相应的商务条件和采购需求对应。商务条件和
//!   采购需求指标有区间规定的，评审因素也应当量化到相应区间。

use anyhow::{Result};
use serde::Deserialize;

use super::AgentTool;

// ─── 主观表述关键词 ────────────────────────────────────────────

/// 强主观性关键词（直接表明评委自由裁量）。
const SUBJECTIVE_KEYWORDS: &[&str] = &[
    "评委酌情",
    "自行掌握",
    "酌情打分",
    "灵活掌握",
    "自主判断",
    "酌情考虑",
    "自由裁量",
];

/// 弱主观性关键词（暗示缺乏量化标准）。
const WEAK_SUBJECTIVE_KEYWORDS: &[&str] = &[
    "综合判断",
    "满意程度",
    "满意",
    "优秀",
    "良好",
    "一般",
    "优良",
    "优良中差",
    "酌情给分",
];

/// 区间跨度阈值
const RANGE_SPAN_WARNING: f64 = 5.0;
const RANGE_SPAN_SEVERE: f64 = 10.0;

// ─── 参数 ──────────────────────────────────────────────────────

/// `detect_subjective_scoring` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct DetectSubjectiveScoringArgs {
    /// 评分标准条款文本
    pub scoring_text: String,
    /// 评分区间最大值（可选）
    #[serde(default)]
    pub score_range_max: Option<f64>,
    /// 评分区间最小值（可选）
    #[serde(default)]
    pub score_range_min: Option<f64>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 主观评分检测返回结果。
#[derive(Debug, serde::Serialize)]
struct SubjectiveScoringResult {
    /// 整体判定: "clean"/"suspicious"/"violation"
    status: String,
    /// 检测到的主观关键词
    detected_keywords: Vec<String>,
    /// 评分区间跨度
    range_span: f64,
    /// 区间跨度是否过宽
    range_too_wide: bool,
    /// 量化问题列表
    quantification_issues: Vec<String>,
    /// 风险等级: "low"/"medium"/"high"
    risk_level: String,
    /// 改进建议
    suggestion: String,
    /// 法条依据
    legal_basis: String,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `detect_subjective_scoring` 工具实现。
///
/// 纯关键词检测与区间计算工具，无外部依赖。
pub struct DetectSubjectiveScoringTool;

impl DetectSubjectiveScoringTool {
    /// 检测评分文本中的主观关键词。
    fn detect_keywords(text: &str) -> Vec<String> {
        let mut keywords = Vec::new();

        for kw in SUBJECTIVE_KEYWORDS {
            if text.contains(kw) {
                keywords.push(kw.to_string());
            }
        }

        for kw in WEAK_SUBJECTIVE_KEYWORDS {
            if text.contains(kw) {
                keywords.push(kw.to_string());
            }
        }

        keywords.sort();
        keywords.dedup();
        keywords
    }

    /// 检测"优良中差"等定性描述但未给出量化细则。
    fn detect_quantification_issues(text: &str) -> Vec<String> {
        let mut issues = Vec::new();

        let qualitative_terms = [
            ("优良中差", "优良中差"),
            ("优良", "优良"),
            ("优秀", "优秀"),
            ("良好", "良好"),
            ("一般", "一般"),
        ];

        for (term, label) in &qualitative_terms {
            if text.contains(term) {
                // 检查后面是否跟着分数或百分比等量化指标
                let idx = text.find(term).unwrap_or(0);
                let after_term = &text[idx + term.len()..];
                let before_term = &text[..idx];

                // 查找量化指标: 数字后跟"分"或"%"
                let has_quantification = {
                    let has_number_after = after_term
                        .chars()
                        .take(50)
                        .collect::<String>()
                        .chars()
                        .any(|c| c.is_ascii_digit());
                    let has_number_before = before_term
                        .chars()
                        .rev()
                        .take(50)
                        .collect::<String>()
                        .chars()
                        .any(|c| c.is_ascii_digit());
                    has_number_after || has_number_before
                };

                if !has_quantification {
                    issues.push(format!(
                        "评分条款使用了定性描述'{}'但未给出具体量化细则，\
                        违反《政府采购货物和服务招标投标管理办法》第55条关于评审因素应当量化的规定。",
                        label
                    ));
                    break; // 只报告一次
                }
            }
        }

        issues
    }

    /// 核心检测逻辑。
    fn detect(args: &DetectSubjectiveScoringArgs) -> Result<SubjectiveScoringResult> {
        let mut quantification_issues: Vec<String> = Vec::new();

        // ── 1. 关键词检测 ──
        let detected_keywords = Self::detect_keywords(&args.scoring_text);

        let has_strong_subjective = detected_keywords
            .iter()
            .any(|k| SUBJECTIVE_KEYWORDS.contains(&k.as_str()));

        let has_weak_subjective = detected_keywords
            .iter()
            .any(|k| WEAK_SUBJECTIVE_KEYWORDS.contains(&k.as_str()));

        // ── 2. 区间跨度检测 ──
        let (range_span, range_too_wide) = match (args.score_range_min, args.score_range_max) {
            (Some(min), Some(max)) => {
                let span = max - min;
                let too_wide = span > RANGE_SPAN_WARNING;
                if span > RANGE_SPAN_SEVERE {
                    quantification_issues.push(format!(
                        "评分区间跨度 {} 分（{}-{}）过于宽泛，超出 10 分的合理范围，\
                        容易导致评审主观性过强。",
                        span, min, max
                    ));
                } else if span > RANGE_SPAN_WARNING {
                    quantification_issues.push(format!(
                        "评分区间跨度 {} 分（{}-{}）偏宽，超出 5 分建议范围。\
                        建议细化评分档次。",
                        span, min, max
                    ));
                }
                (span, too_wide)
            }
            _ => (0.0, false),
        };

        // ── 3. 量化细则缺失检测 ──
        let qual_issues = Self::detect_quantification_issues(&args.scoring_text);
        quantification_issues.extend(qual_issues);

        // ── 4. 综合判定 ──
        // 注意: && 优先级高于 ||, 需加括号明确语义
        let (status, risk_level, suggestion) = if (has_strong_subjective || !quantification_issues.is_empty()) && range_too_wide && range_span > RANGE_SPAN_SEVERE {
            // 强主观关键词 或 有量化问题 + 区间严重过宽 → violation
            let s = "violation";
            let rl = "high";
            let mut sugg = String::new();

            if has_strong_subjective {
                sugg.push_str("删除'评委酌情''自行掌握'等主观表述，改为可量化的客观指标；");
            }
            if !quantification_issues.is_empty() {
                sugg.push_str("将评分区间细化为多档，每档明确量化标准；");
            }
            if range_too_wide && range_span > RANGE_SPAN_SEVERE {
                sugg.push_str(&format!(
                    "将评分区间跨度从 {:.0} 分缩小到 5 分以内，细化评分档次。",
                    range_span
                ));
            }

            (s.to_string(), rl.to_string(), sugg)
        } else if has_strong_subjective || has_weak_subjective || range_too_wide || !quantification_issues.is_empty() {
            // 强/弱主观 或 区间偏宽 或 有量化问题 → suspicious
            let s = "suspicious";
            let rl = "medium";
            let mut sugg = Vec::new();

            if has_weak_subjective {
                sugg.push("将'综合判断''满意程度'等模糊表述替换为具体可量化的评分标准".to_string());
            }
            if range_too_wide {
                sugg.push(format!(
                    "将评分区间跨度从 {:.0} 分缩小到 5 分以内",
                    range_span
                ));
            }
            if !quantification_issues.is_empty() {
                sugg.push("为定性描述补充具体量化细则".to_string());
            }

            (s.to_string(), rl.to_string(), sugg.join("；") + "。")
        } else {
            // 无问题 → clean
            (
                "clean".to_string(),
                "low".to_string(),
                "评分标准量化清晰，未检测到主观性表述。".to_string(),
            )
        };

        // ── 5. 法条依据 ──
        let legal_basis = "《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条：\
                          评审因素应当细化和量化，且与相应的商务条件和采购需求对应。\
                          商务条件和采购需求指标有区间规定的，评审因素也应当量化到相应区间。\
                          《政府采购法实施条例》第34条：采用综合评分法的，评审标准中的分值设置\
                          应当与评审因素的量化指标相对应。"
            .to_string();

        Ok(SubjectiveScoringResult {
            status,
            detected_keywords,
            range_span,
            range_too_wide,
            quantification_issues,
            risk_level,
            suggestion,
            legal_basis,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for DetectSubjectiveScoringTool {
    fn name(&self) -> &str {
        "detect_subjective_scoring"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "detect_subjective_scoring",
                "description": "【使用场景】检测评分标准条款中是否存在主观性表述，判断是否违反\
                    《政府采购货物和服务招标投标管理办法》第55条关于评审因素应当量化的规定——\
                    ① 检测'评委酌情''自行掌握''综合判断''满意程度''优良中差''酌情打分''灵活掌握'等主观关键词；\
                    ② 评分区间跨度超过5分标记过宽，超过10分标记严重；\
                    ③ 检测'优良中差'等定性描述是否伴有量化细则。\
                    【不使用场景】不校验评分标准完整性（用 check_scoring_completeness）；\
                    不校验权重分配（用 validate_weight_distribution）。\
                    【法条依据】《政府采购货物和服务招标投标管理办法》（财政部令第87号）第55条、\
                    《政府采购法实施条例》第34条。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "scoring_text": {
                            "type": "string",
                            "description": "评分标准条款原文。"
                        },
                        "score_range_max": {
                            "type": "number",
                            "description": "评分区间最大值（可选，用于检测区间跨度）。"
                        },
                        "score_range_min": {
                            "type": "number",
                            "description": "评分区间最小值（可选）。"
                        }
                    },
                    "required": ["scoring_text"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: DetectSubjectiveScoringArgs = serde_json::from_value(args)?;
        let result = Self::detect(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_judge_discretion_suspicious() {
        // "评委酌情打分" → suspicious
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "评委酌情打分，根据投标人综合表现给予相应分值。".to_string(),
            score_range_max: None,
            score_range_min: None,
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert!(result.status == "suspicious" || result.status == "violation");
        assert!(
            result
                .detected_keywords
                .iter()
                .any(|k| k.contains("评委酌情")),
            "应检测到'评委酌情'关键词"
        );
    }

    #[test]
    fn test_range_span_8_too_wide() {
        // 区间跨度 8 分 → 过宽
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "根据投标人实施方案的完整性进行评分。".to_string(),
            score_range_max: Some(10.0),
            score_range_min: Some(2.0),
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert!(result.range_too_wide);
        assert!((result.range_span - 8.0).abs() < 1e-6);
        assert!(!result.quantification_issues.is_empty());
    }

    #[test]
    fn test_yxlzc_without_detail_violation() {
        // "优良中差"无细则 → 有量化问题
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "评审委员会根据投标人的技术方案分为优良中差四个等级进行打分。".to_string(),
            score_range_max: None,
            score_range_min: None,
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert!(
            !result.quantification_issues.is_empty(),
            "应检测到'优良中差'缺少量化细则"
        );
    }

    #[test]
    fn test_normal_quantified_clean() {
        // 正常量化评分 → clean
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "具备ISO9001质量管理体系认证得2分，不具备得0分。每提供一个同类项目业绩得1分，最高3分。"
                .to_string(),
            score_range_max: Some(5.0),
            score_range_min: Some(0.0),
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert_eq!(result.status, "clean");
        assert_eq!(result.risk_level, "low");
        assert!(result.detected_keywords.is_empty());
    }

    #[test]
    fn test_flexible_control_keyword_detected() {
        // "灵活掌握" → 检测到
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "评审时可根据实际情况灵活掌握评分尺度。".to_string(),
            score_range_max: None,
            score_range_min: None,
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert!(
            result
                .detected_keywords
                .iter()
                .any(|k| k.contains("灵活掌握")),
            "应检测到'灵活掌握'关键词"
        );
    }

    #[test]
    fn test_comprehensive_judgment_suspicious() {
        // "综合判断" → suspicious
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "由评审委员会根据投标文件进行综合判断后打分。".to_string(),
            score_range_max: None,
            score_range_min: None,
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert!(
            result
                .detected_keywords
                .iter()
                .any(|k| *k == "综合判断"),
            "应检测到'综合判断'关键词"
        );
    }

    #[test]
    fn test_range_span_12_severe() {
        // 区间跨度 12 分 → 严重
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "根据投标人服务质量进行综合评估打分。".to_string(),
            score_range_max: Some(15.0),
            score_range_min: Some(3.0),
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert!(result.range_too_wide);
        let has_severe = result
            .quantification_issues
            .iter()
            .any(|i| i.contains("过于宽泛"));
        assert!(has_severe, "12分跨度应标记严重");
    }

    #[test]
    fn test_satisfaction_level_suspicious() {
        // "满意程度" → 弱主观关键词
        let args = DetectSubjectiveScoringArgs {
            scoring_text: "根据采购人的满意程度进行打分，满意得满分，基本满意得一半分。".to_string(),
            score_range_max: None,
            score_range_min: None,
        };
        let result = DetectSubjectiveScoringTool::detect(&args).unwrap();
        assert!(
            result
                .detected_keywords
                .iter()
                .any(|k| k.contains("满意")),
            "应检测到'满意程度'关键词"
        );
    }
}
