//! `check_scoring_completeness` 工具 — 评分标准完整性检查。
//!
//! 根据《政府采购货物和服务招标投标管理办法》（财政部令第87号），
//! 检查评分标准是否完整、分值是否闭合、评审维度是否齐备、评分细则是否充分。
//! 本工具进行纯数值计算与维度匹配，不访问外部 I/O。
//!
//! ## 核心逻辑
//!
//! 1. 分值求和验证：所有评审项分值之和应等于 total_score
//! 2. 评分维度完整性：货物须有价格/技术/商务，服务须有价格/服务/技术
//! 3. 评分细则覆盖率：has_detail=true 的评审项比例
//!
//! ## 法条依据
//!
//! - 《政府采购货物和服务招标投标管理办法》（财政部令第87号）

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 浮点数比较容差 ────────────────────────────────────────────

const EPSILON: f64 = 1e-6;

// ─── 参数 ──────────────────────────────────────────────────────

/// `check_scoring_completeness` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct CheckScoringCompletenessArgs {
    /// 评审项列表
    pub scoring_items: Vec<ScoringItemInput>,
    /// 总分值
    pub total_score: f64,
    /// 采购品类："货物"/"工程"/"服务"
    pub procurement_category: String,
}

/// 评审项输入。
#[derive(Debug, Deserialize)]
pub struct ScoringItemInput {
    /// 评审项名称
    pub name: String,
    /// 最高分值
    pub max_score: f64,
    /// 是否有评分细则
    #[serde(default)]
    pub has_detail: bool,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 评分标准完整性检查返回结果。
#[derive(Debug, serde::Serialize)]
struct ScoringCompletenessResult {
    /// 整体判定: "complete"/"incomplete"/"violation"
    status: String,
    /// 分值总和
    score_sum: f64,
    /// 分值是否闭合（等于 total_score）
    score_ok: bool,
    /// 必要维度检查结果
    required_dimensions: Vec<DimensionCheck>,
    /// 评分细则覆盖率 (0.0-1.0)
    detail_coverage: f64,
    /// 缺少评分细则的评审项名称
    items_without_detail: Vec<String>,
    /// 缺失的必要评审维度
    missing_dimensions: Vec<String>,
    /// 综合摘要
    summary: String,
}

/// 维度检查结果。
#[derive(Debug, serde::Serialize)]
struct DimensionCheck {
    /// 维度名称
    dimension: String,
    /// 是否已包含
    present: bool,
    /// 是否必需
    required: bool,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `check_scoring_completeness` 工具实现。
///
/// 纯数值计算与维度匹配工具，无外部依赖。
pub struct CheckScoringCompletenessTool;

impl CheckScoringCompletenessTool {
    /// 判断两个 f64 是否相等（在容差范围内）。
    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    /// 检查名称列表中是否包含特定维度的关键词。
    fn contains_dimension(items: &[ScoringItemInput], keywords: &[&str]) -> bool {
        items.iter().any(|item| {
            keywords
                .iter()
                .any(|kw| item.name.contains(kw) || item.name.to_lowercase().contains(&kw.to_lowercase()))
        })
    }

    /// 获取品类的必要维度关键词。
    fn get_required_dimensions(category: &str) -> Result<Vec<(&'static str, Vec<&'static str>)>> {
        match category {
            "货物" => Ok(vec![
                ("价格", vec!["价格", "报价", "投标价"]),
                ("技术", vec!["技术", "施工方案", "实施方案"]),
                ("商务", vec!["商务", "业绩", "资质", "信誉", "财务状况"]),
            ]),
            "工程" => Ok(vec![
                ("价格", vec!["价格", "报价", "投标价"]),
                ("技术", vec!["技术", "施工方案", "施工组织", "项目管理"]),
            ]),
            "服务" => Ok(vec![
                ("价格", vec!["价格", "报价", "投标价"]),
                ("服务", vec!["服务方案", "服务", "售后服务", "运维"]),
                ("技术", vec!["技术", "人员", "团队", "资质"]),
            ]),
            _ => Err(anyhow!(
                "不支持的采购品类 '{}'，有效值为: 货物/工程/服务",
                category
            )),
        }
    }

    /// 核心检查逻辑。
    fn check(args: &CheckScoringCompletenessArgs) -> Result<ScoringCompletenessResult> {
        // ── 1. 分值求和验证 ──
        let score_sum: f64 = args.scoring_items.iter().map(|item| item.max_score).sum();
        let score_ok = Self::approx_eq(score_sum, args.total_score);

        // ── 2. 评分细则覆盖率 ──
        let total_items = args.scoring_items.len();
        let items_with_detail = args.scoring_items.iter().filter(|i| i.has_detail).count();
        let detail_coverage = if total_items > 0 {
            items_with_detail as f64 / total_items as f64
        } else {
            1.0
        };

        let items_without_detail: Vec<String> = args
            .scoring_items
            .iter()
            .filter(|i| !i.has_detail)
            .map(|i| i.name.clone())
            .collect();

        // ── 3. 维度完整性检查 ──
        let required_dims = Self::get_required_dimensions(&args.procurement_category)?;
        let mut required_dimensions = Vec::new();
        let mut missing_dimensions = Vec::new();

        for (dim_name, keywords) in &required_dims {
            let present = Self::contains_dimension(&args.scoring_items, keywords);
            required_dimensions.push(DimensionCheck {
                dimension: dim_name.to_string(),
                present,
                required: true,
            });
            if !present {
                missing_dimensions.push(dim_name.to_string());
            }
        }

        // ── 4. 综合判定 ──
        let has_violation = !score_ok
            || !missing_dimensions.is_empty();

        let status = if has_violation {
            "violation"
        } else if detail_coverage < 1.0 {
            "incomplete"
        } else {
            "complete"
        };

        // ── 5. 综合摘要 ──
        let mut summary_parts: Vec<String> = Vec::new();

        if score_ok {
            summary_parts.push(format!(
                "评审分值总和 {:.1} 等于总分 {:.0}，分值闭合",
                score_sum, args.total_score
            ));
        } else {
            summary_parts.push(format!(
                "分值不闭合：评审项之和 {:.1} 不等于总分 {:.0}（差额 {:.1}）",
                score_sum,
                args.total_score,
                (score_sum - args.total_score).abs()
            ));
        }

        if missing_dimensions.is_empty() {
            summary_parts.push(format!(
                "{} 品类评审维度齐备",
                args.procurement_category
            ));
        } else {
            summary_parts.push(format!(
                "{} 品类缺失必要评审维度：{}",
                args.procurement_category,
                missing_dimensions.join("、")
            ));
        }

        if detail_coverage >= 1.0 {
            summary_parts.push("全部评审项均有评分细则".to_string());
        } else {
            summary_parts.push(format!(
                "评分细则覆盖率 {:.0}%（{} / {} 项有细则），以下评审项缺少细则：{}",
                detail_coverage * 100.0,
                items_with_detail,
                total_items,
                items_without_detail.join("、")
            ));
        }

        let summary = summary_parts.join("；") + "。";

        Ok(ScoringCompletenessResult {
            status: status.to_string(),
            score_sum,
            score_ok,
            required_dimensions,
            detail_coverage,
            items_without_detail,
            missing_dimensions,
            summary,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for CheckScoringCompletenessTool {
    fn name(&self) -> &str {
        "check_scoring_completeness"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "check_scoring_completeness",
                "description": "检查评分标准完整性：各评审项分值之和=总分、维度齐全、细则覆盖。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "scoring_items": {
                            "type": "array",
                            "description": "评审项列表。",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "description": "评审项名称，如'价格得分'、'技术方案'、'商务资质'等。"
                                    },
                                    "max_score": {
                                        "type": "number",
                                        "description": "该评审项最高分值。"
                                    },
                                    "has_detail": {
                                        "type": "boolean",
                                        "description": "是否有评分细则（可选，默认 false）。"
                                    }
                                },
                                "required": ["name", "max_score"]
                            }
                        },
                        "total_score": {
                            "type": "number",
                            "description": "评审总分值，通常为 100。"
                        },
                        "procurement_category": {
                            "type": "string",
                            "enum": ["货物", "工程", "服务"],
                            "description": "采购品类。"
                        }
                    },
                    "required": ["scoring_items", "total_score", "procurement_category"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: CheckScoringCompletenessArgs = serde_json::from_value(args)?;
        let result = Self::check(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 构建测试用的评审项列表。
    fn make_items(items: Vec<(&str, f64, bool)>) -> Vec<ScoringItemInput> {
        items
            .into_iter()
            .map(|(name, max_score, has_detail)| ScoringItemInput {
                name: name.to_string(),
                max_score,
                has_detail,
            })
            .collect()
    }

    #[test]
    fn test_goods_price_tech_biz_complete() {
        // 货物：价格30 + 技术50 + 商务20 = 100 → 完整
        let items = make_items(vec![
            ("价格得分", 30.0, true),
            ("技术方案", 50.0, true),
            ("商务资质", 20.0, true),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "货物".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        assert_eq!(result.status, "complete");
        assert!(result.score_ok);
        assert!(result.missing_dimensions.is_empty());
        assert!((result.detail_coverage - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_goods_score_not_closed() {
        // 货物：价格30 + 技术60 = 90，不等于100 → 不闭合
        let items = make_items(vec![
            ("价格得分", 30.0, true),
            ("技术方案", 60.0, true),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "货物".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(!result.score_ok);
        assert!((result.score_sum - 90.0).abs() < 1e-6);
    }

    #[test]
    fn test_goods_missing_biz_dimension() {
        // 货物：价格30 + 技术70 = 100，但缺少商务维度 → violation
        let items = make_items(vec![
            ("价格得分", 30.0, true),
            ("技术方案", 70.0, true),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "货物".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(
            result
                .missing_dimensions
                .iter()
                .any(|d| d == "商务"),
            "货物品类应检测到缺少商务维度"
        );
    }

    #[test]
    fn test_no_detail_coverage() {
        // 全部无评分细则 → coverage = 0
        let items = make_items(vec![
            ("价格得分", 30.0, false),
            ("技术方案", 50.0, false),
            ("商务资质", 20.0, false),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "货物".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        assert_eq!(result.status, "incomplete");
        assert!((result.detail_coverage - 0.0).abs() < 1e-6);
        assert_eq!(result.items_without_detail.len(), 3);
    }

    #[test]
    fn test_service_complete() {
        // 服务：价格20 + 服务40 + 技术40 = 100 → 完整
        let items = make_items(vec![
            ("价格得分", 20.0, true),
            ("服务方案", 40.0, true),
            ("技术能力", 40.0, true),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "服务".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        assert_eq!(result.status, "complete");
        assert!(result.missing_dimensions.is_empty());
    }

    #[test]
    fn test_engineering_complete() {
        // 工程：价格40 + 技术60 = 100 → 完整
        let items = make_items(vec![
            ("价格得分", 40.0, true),
            ("技术方案（施工组织设计）", 60.0, true),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "工程".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        assert_eq!(result.status, "complete");
        assert!(result.missing_dimensions.is_empty());
    }

    #[test]
    fn test_missing_dimension_and_detail_coverage() {
        // 货物缺少商务维度 + 一半有细则
        let items = make_items(vec![
            ("价格得分", 30.0, true),
            ("技术方案", 70.0, false),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "货物".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(
            result.missing_dimensions.iter().any(|d| d == "商务"),
            "应检测到缺少商务维度"
        );
        assert!(
            result.items_without_detail.iter().any(|i| i == "技术方案"),
            "技术方案应标记为缺少细则"
        );
    }

    #[test]
    fn test_invalid_category_errors() {
        let items = make_items(vec![
            ("价格得分", 100.0, true),
        ]);
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "设计".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_items_total_score_match() {
        // 空列表，总分100 → 完整（没有任何检查项为0个时coverage=1.0）
        let items: Vec<ScoringItemInput> = vec![];
        let args = CheckScoringCompletenessArgs {
            scoring_items: items,
            total_score: 100.0,
            procurement_category: "货物".to_string(),
        };
        let result = CheckScoringCompletenessTool::check(&args).unwrap();
        // 空列表 sum=0，不等于 total_score=100，所以不闭合，且缺少所有维度 → violation
        assert_eq!(result.status, "violation");
    }
}
