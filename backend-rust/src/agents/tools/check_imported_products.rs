//! `check_imported_products` 工具 — 进口产品管理检查。
//!
//! 根据《政府采购进口产品管理办法》（财库〔2007〕119号）第4-7条，
//! 检查采购文件中是否涉及进口产品，以及是否已依法取得财政部门审批。
//! 本工具执行关键词匹配和规则判定，不访问外部 I/O。
//!
//! ## 核心逻辑
//!
//! - 检测采购文件中是否出现进口产品相关关键词
//! - 区分"采购进口产品"（需审批）与"参考国际标准"（无需审批）
//! - 判定是否已取得进口产品审批
//!
//! ## 法条依据
//!
//! - 《政府采购进口产品管理办法》（财库〔2007〕119号）第4-7条
//!   - 第4条：政府采购应当采购本国产品，确需采购进口产品的，实行审核管理。
//!   - 第5条：采购进口产品需经财政部门审核同意。
//!   - 第6条：采购人提交申请材料要求。
//!   - 第7条：财政部门审核内容和程序。

use anyhow::{Result, anyhow};
use serde::Deserialize;

use super::AgentTool;

// ─── 关键词常量 ──────────────────────────────────────────────

/// 进口产品采购相关关键词（命中即需审批）。
const IMPORT_PURCHASE_KEYWORDS: &[&str] = &[
    "采购进口",
    "采购原装进口",
    "进口设备",
    "进口产品",
    "境外采购",
    "境外提供",
];

/// 弱进口信号（需结合上下文判断，避免误报）。
/// 这些词本身可能只是背景描述（如"海外项目经验"），必须结合周边判断。
const IMPORT_WEAK_KEYWORDS: &[&str] = &[
    "原装进口",
    "海外",
    "国际品牌",
    "CE认证",
    "FDA认证",
    "UL认证",
];

/// 排除上下文 — 包含这些短语时弱信号不应触发。
const IMPORT_EXCLUDE_CONTEXT: &[&str] = &[
    "不接受", "禁止", "不得采购", "不得使用", "未经审批",
    "海外项目", "海外市场", "国际品牌不", "不接受国际",
];

/// 仅表示参考国际标准的关键词（不视为进口产品采购）。
const STANDARD_REFERENCE_KEYWORDS: &[&str] = &[
    "国外标准",
    "国际标准",
];

/// 国产优先相关信号词。
const DOMESTIC_PRIORITY_KEYWORDS: &[&str] = &[
    "国产优先",
    "优先采购国产",
    "优先采购本国产品",
    "国产产品优先",
    "支持国产",
];

// ─── 参数 ──────────────────────────────────────────────────────

/// `check_imported_products` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct CheckImportedProductsArgs {
    /// 项目描述文本
    pub project_description: String,
    /// 采购类别："货物"/"工程"/"服务"
    pub procurement_category: String,
    /// 是否已有进口产品审批
    #[serde(default)]
    pub has_import_approval: Option<bool>,
    /// 审批文件编号（如有）
    #[serde(default)]
    pub approval_document: Option<String>,
}

// ─── 输出 ──────────────────────────────────────────────────────

/// 进口产品检查的返回结果。
#[derive(Debug, serde::Serialize)]
pub struct ImportedProductCheckResult {
    /// 整体合规判定：compliant / violation / uncertain / clean
    pub status: String,
    /// 检测到的所有关键词
    pub detected_keywords: Vec<String>,
    /// 是否检测到进口产品采购意图
    pub imported_detected: bool,
    /// 是否有审批
    pub has_approval: bool,
    /// 是否需要审批
    pub need_approval: bool,
    /// 风险信号
    pub risk_signals: Vec<String>,
    /// 综合建议
    pub suggestion: String,
    /// 法条依据
    pub legal_basis: String,
}

// ─── 工具实现 ──────────────────────────────────────────────────

/// `check_imported_products` 工具实现。
///
/// 纯关键词匹配与规则判定工具，无外部依赖。
pub struct CheckImportedProductsTool;

impl CheckImportedProductsTool {
    /// 核心检查逻辑。
    fn check(args: &CheckImportedProductsArgs) -> Result<ImportedProductCheckResult> {
        let text = &args.project_description;
        if text.trim().is_empty() {
            return Err(anyhow!("project_description 不能为空"));
        }

        // 1. 验证 procurement_category
        let valid_categories = ["货物", "工程", "服务"];
        if !valid_categories.contains(&args.procurement_category.as_str()) {
            return Err(anyhow!(
                "无效的 procurement_category '{}'，有效值为: 货物/工程/服务",
                args.procurement_category
            ));
        }

        let mut detected_keywords: Vec<String> = Vec::new();
        let mut risk_signals: Vec<String> = Vec::new();

        // 2. 检测所有关键词
        let mut has_import_purchase = false;
        let mut has_standard_only = false;

        for kw in IMPORT_PURCHASE_KEYWORDS {
            if text.contains(kw) {
                detected_keywords.push(kw.to_string());
                has_import_purchase = true;
            }
        }

        // 弱信号检测（需排除上下文）
        for kw in IMPORT_WEAK_KEYWORDS {
            if text.contains(kw) {
                let byte_pos = text.find(kw).unwrap_or(0);
                // 安全的 UTF-8 边界切片
                let window_start = if byte_pos <= 20 { 0 } else {
                    // 向前找到最近的字边界
                    let mut pos = byte_pos.saturating_sub(20);
                    while pos > 0 && !text.is_char_boundary(pos) { pos -= 1; }
                    pos
                };
                let window_end = (byte_pos + kw.len() + 20).min(text.len());
                let window_end = {
                    let mut pos = window_end;
                    while pos < text.len() && !text.is_char_boundary(pos) { pos += 1; }
                    pos.min(text.len())
                };
                let context = &text[window_start..window_end];
                let is_excluded = IMPORT_EXCLUDE_CONTEXT.iter().any(|ex| context.contains(ex));
                if !is_excluded {
                    detected_keywords.push(kw.to_string());
                    has_import_purchase = true;
                }
            }
        }

        for kw in STANDARD_REFERENCE_KEYWORDS {
            if text.contains(kw) {
                detected_keywords.push(kw.to_string());
                has_standard_only = true;
            }
        }

        // 3. 国产优先检测（弱信号）
        for kw in DOMESTIC_PRIORITY_KEYWORDS {
            if text.contains(kw) {
                if !detected_keywords.contains(&kw.to_string()) {
                    detected_keywords.push(kw.to_string());
                }
                risk_signals.push(format!(
                    "出现'{}'表述：招标文件不应使用'国产优先'等模糊表述，\
                     如需采购本国产品应明确'本项目不接受进口产品投标'。\
                     模糊表述可能在投诉中被质疑为倾向性或歧视性条款。",
                    kw
                ));
            }
        }

        // 4. 判定逻辑
        let has_approval = args.has_import_approval.unwrap_or(false);

        let (status, imported_detected, need_approval, suggestion) = if has_import_purchase {
            // 检测到进口产品采购意图
            if has_approval {
                let approval_info = args
                    .approval_document
                    .as_ref()
                    .map(|d| format!(" 审批文件编号：{}。", d))
                    .unwrap_or_default();
                (
                    "compliant".to_string(),
                    true,
                    true,
                    format!(
                        "检测到进口产品相关关键词（{}），但已取得财政部门审批。{} \
                        建议在采购文件中明确引用审批文件编号，并注明进口产品清单。",
                        detected_keywords.join("、"),
                        approval_info
                    ),
                )
            } else {
                (
                    "violation".to_string(),
                    true,
                    true,
                    format!(
                        "检测到进口产品相关关键词（{}），但未提供进口产品审批。\
                         根据《政府采购进口产品管理办法》第4-7条，采购进口产品须经财政部门审核同意。\
                         建议：① 如确需采购进口产品，先向财政部门申请审批；\
                         ② 如非必需，修改采购文件，将'进口产品'改为'本国产品'。",
                        detected_keywords.join("、")
                    ),
                )
            }
        } else if has_standard_only {
            // 仅检测到标准引用关键词 → 不需要审批
            let mut suggestion_text = format!(
                "仅检测到国际/国外标准引用关键词（{}），属于技术规范引用范畴，\
                 不属于进口产品采购，无需取得进口产品审批。",
                detected_keywords.join("、")
            );
            if !risk_signals.is_empty() {
                suggestion_text.push_str(&format!(
                    " 但存在以下需注意的信号：{}",
                    risk_signals.join("；")
                ));
            }
            (
                "clean".to_string(),
                false,
                false,
                suggestion_text,
            )
        } else {
            // 未检测到任何进口产品相关关键词
            let mut suggestion_text = "未检测到进口产品相关关键词，采购内容为国产产品，无需进口审批。".to_string();
            if !risk_signals.is_empty() {
                suggestion_text.push_str(&format!(
                    " 但存在以下需注意的信号：{}",
                    risk_signals.join("；")
                ));
            }
            (
                "clean".to_string(),
                false,
                false,
                suggestion_text,
            )
        };

        let legal_basis = "《政府采购进口产品管理办法》（财库〔2007〕119号）第4-7条：\
            第4条 政府采购应当采购本国产品，确需采购进口产品的，实行审核管理。\
            第5条 采购人需要采购的产品在中国境内无法获取或者无法以合理的商业条件获取，\
            以及法律法规另有规定确需采购进口产品的，应当在获得财政部门核准后，依法开展政府采购活动。\
            第6条 采购人报财政部门审核时，应当出具以下材料……\
            第7条 财政部门审核同意后，采购人可以依法开展采购活动。"
            .to_string();

        Ok(ImportedProductCheckResult {
            status,
            detected_keywords,
            imported_detected,
            has_approval,
            need_approval,
            risk_signals,
            suggestion,
            legal_basis,
        })
    }
}

// ─── AgentTool 实现 ────────────────────────────────────────────

#[async_trait::async_trait]
impl AgentTool for CheckImportedProductsTool {
    fn name(&self) -> &str {
        "check_imported_products"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "check_imported_products",
                "description": "【使用场景】检查采购文件是否涉及进口产品，以及是否已依法取得财政部门审批——\
                    ① 检测文本中是否出现进口产品采购相关关键词（进口/原装进口/海外/国际品牌/境外采购/境外提供/CE认证/FDA认证/UL认证）；\
                    ② 区分'采购进口产品'（需审批）与'参考国际标准'（无需审批）；\
                    ③ 验证是否已取得进口产品审批，如无审批则标记违规。\
                    【不使用场景】不负责审核进口产品审批材料的具体内容是否完整；\
                    不负责判断进口产品的必要性或合理性。\
                    【法条依据】《政府采购进口产品管理办法》（财库〔2007〕119号）第4-7条。\
                    【注意】进口产品采购须经财政部门审核同意后方可进行，\\
                    未经审批的进口产品采购条款属于严重合规风险。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project_description": {
                            "type": "string",
                            "description": "项目描述文本，通常为采购需求或技术规格中的文字内容。"
                        },
                        "procurement_category": {
                            "type": "string",
                            "enum": ["货物", "工程", "服务"],
                            "description": "采购类别。"
                        },
                        "has_import_approval": {
                            "type": "boolean",
                            "description": "是否已有进口产品审批。可选，默认 false。"
                        },
                        "approval_document": {
                            "type": "string",
                            "description": "审批文件编号（如有）。可选。"
                        }
                    },
                    "required": ["project_description", "procurement_category"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: CheckImportedProductsArgs = serde_json::from_value(args)?;
        let result = Self::check(&parsed)?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_without_approval_violation() {
        // 含"进口产品"无审批 → violation
        let args = CheckImportedProductsArgs {
            project_description: "本项目拟采购进口产品一批，包括进口医疗设备等。"
                .to_string(),
            procurement_category: "货物".to_string(),
            has_import_approval: None,
            approval_document: None,
        };
        let result = CheckImportedProductsTool::check(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(result.imported_detected);
        assert!(result.need_approval);
        assert!(!result.has_approval);
        assert!(result.detected_keywords.contains(&"进口产品".to_string()));
    }

    #[test]
    fn test_international_standard_clean() {
        // 含"国际标准"→ clean（不需要审批）
        let args = CheckImportedProductsArgs {
            project_description: "本项目设备应符合国际标准ISO 9001和国外标准相关要求。"
                .to_string(),
            procurement_category: "货物".to_string(),
            has_import_approval: None,
            approval_document: None,
        };
        let result = CheckImportedProductsTool::check(&args).unwrap();
        assert_eq!(result.status, "clean");
        assert!(!result.imported_detected);
        assert!(!result.need_approval);
        assert!(result.detected_keywords.contains(&"国际标准".to_string()));
        assert!(result.detected_keywords.contains(&"国外标准".to_string()));
    }

    #[test]
    fn test_import_with_approval_compliant() {
        // 含"原装进口"有审批 → compliant
        let args = CheckImportedProductsArgs {
            project_description: "核心部件采用原装进口产品，已取得进口审批。"
                .to_string(),
            procurement_category: "货物".to_string(),
            has_import_approval: Some(true),
            approval_document: Some("财采审〔2025〕001号".to_string()),
        };
        let result = CheckImportedProductsTool::check(&args).unwrap();
        assert_eq!(result.status, "compliant");
        assert!(result.imported_detected);
        assert!(result.need_approval);
        assert!(result.has_approval);
        assert!(result.detected_keywords.contains(&"原装进口".to_string()));
    }

    #[test]
    fn test_pure_domestic_clean() {
        // 纯国产 → clean
        let args = CheckImportedProductsArgs {
            project_description: "本次采购全部为国产产品，供应商须为境内注册企业。"
                .to_string(),
            procurement_category: "服务".to_string(),
            has_import_approval: None,
            approval_document: None,
        };
        let result = CheckImportedProductsTool::check(&args).unwrap();
        assert_eq!(result.status, "clean");
        assert!(!result.imported_detected);
        assert!(!result.need_approval);
        assert!(result.detected_keywords.is_empty());
    }

    #[test]
    fn test_domestic_priority_weak_signal() {
        // "国产优先" → 弱信号（仍为 clean 但产生风险信号）
        let args = CheckImportedProductsArgs {
            project_description: "本项目在同等条件下国产优先。"
                .to_string(),
            procurement_category: "货物".to_string(),
            has_import_approval: None,
            approval_document: None,
        };
        let result = CheckImportedProductsTool::check(&args).unwrap();
        assert_eq!(result.status, "clean");
        assert!(!result.risk_signals.is_empty());
        assert!(result
            .risk_signals
            .iter()
            .any(|s| s.contains("国产优先")));
    }

    #[test]
    fn test_ce_fda_cert_without_approval_violation() {
        // 含CE认证/FDA认证，无审批 → violation
        let args = CheckImportedProductsArgs {
            project_description: "投标产品须取得CE认证和FDA认证证书。"
                .to_string(),
            procurement_category: "货物".to_string(),
            has_import_approval: None,
            approval_document: None,
        };
        let result = CheckImportedProductsTool::check(&args).unwrap();
        assert_eq!(result.status, "violation");
        assert!(result.imported_detected);
        assert!(result.detected_keywords.contains(&"CE认证".to_string()));
        assert!(result.detected_keywords.contains(&"FDA认证".to_string()));
    }

    #[test]
    fn test_empty_description_error() {
        let args = CheckImportedProductsArgs {
            project_description: "".to_string(),
            procurement_category: "货物".to_string(),
            has_import_approval: None,
            approval_document: None,
        };
        let result = CheckImportedProductsTool::check(&args);
        assert!(result.is_err());
    }
}
