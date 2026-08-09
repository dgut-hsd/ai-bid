//! LLM 预标注工具 —— 对规则引擎漏检（FN）条款调用 qwen-plus 进行分类标注，
//! 生成结构化 JSON 用于人工复核和规则库扩充。
//!
//! ## P3a: LLM 预标注 Prompt Schema
//!
//! ### System Prompt 结构
//! - 角色定义：政府采购合规审查标注专家
//! - 15 个风险类别的判别标准（每条含触发条件 + 法条依据）
//! - Critical 判定规则（C01-C05 前五类均为 Critical）
//! - 输出约束：严格 JSON、仅选择最匹配类别、无风险选 NO_RISK
//!
//! ### User Prompt 模板
//! ```text
//! 请对以下政府采购条款进行风险分类标注：
//!
//! 条款原文：
//! """
//! {clause_text}
//! """
//!
//! 章节上下文：{section_title}
//! 来源文档：{document_id}
//!
//! 请严格按照 System Prompt 中定义的 15 类标准进行判别。
//! ```
//!
//! ### Output JSON Schema
//! ```json
//! {
//!   "category_code": "LOCAL_REGISTRATION | BRAND_LOCK | ... | NO_RISK",
//!   "is_critical": true,
//!   "confidence": 0.92,
//!   "evidence_quote": "精确原文片段",
//!   "reasoning": "判定理由（1-2 句）",
//!   "severity": "high | medium",
//!   "alternative_categories": ["候选2", "候选3"]
//! }
//! ```
//!
//! ## 运行方法
//! ```powershell
//! # 对 blind-v2 中所有 11 个 FN 条款跑 LLM 预标注
//! cargo run --bin llm_label
//!
//! # 仅对指定 finding_id 跑（调试用）
//! cargo run --bin llm_label -- BLIND-002-F02 BLIND-004-F03
//! ```

use ai_bid::agents::react_loop::{ChatMessage, ToolChoice};
use ai_bid::rules::catalog::display_name;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

// ── 数据模型 ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BlindAnnotation {
    document_id: String,
    finding_id: String,
    category_code: String,
    risk_type: String,
    severity: String,
    is_critical: bool,
    source_quote: String,
    section_path: Option<Vec<String>>,
}

/// LLM 返回的标注结果（JSON Schema）
#[derive(Debug, Deserialize, Serialize, Clone)]
struct LlmAnnotation {
    category_code: String,
    is_critical: bool,
    confidence: f64,
    evidence_quote: String,
    reasoning: String,
    severity: String,
    #[serde(default)]
    alternative_categories: Vec<String>,
}

/// 最终输出：真值（ground truth）+ 规则引擎结果 + LLM 标注 三方对照
#[derive(Debug, Serialize)]
struct FnContrastRow {
    finding_id: String,
    document_id: String,
    section_title: String,
    clause_text: String,
    // Ground truth
    gt_category: String,
    gt_category_display: String,
    gt_is_critical: bool,
    gt_severity: String,
    // Rule engine status (FN = 规则引擎漏检)
    rule_engine_status: String, // "FN"
    // LLM annotation
    llm_category: String,
    llm_category_display: String,
    llm_is_critical: bool,
    llm_confidence: f64,
    llm_severity: String,
    llm_evidence_quote: String,
    llm_reasoning: String,
    llm_alternatives: Vec<String>,
    // Meta
    llm_matches_gt: bool,
    review_status: String, // "PENDING_REVIEW"
}

/// 汇总报告
#[derive(Debug, Serialize)]
struct LabelSummary {
    total_fn_count: usize,
    llm_called_count: usize,
    llm_success_count: usize,
    llm_error_count: usize,
    llm_matches_gt_count: usize,
    llm_match_rate: f64,
    // per-category: gt -> llm prediction distribution
    confusion_matrix: Vec<(String, String, usize)>,
    output_file: String,
}

// ── System Prompt: 15 类判别标准 ───────────────────────────────

const SYSTEM_PROMPT: &str = r#"你是政府采购合规审查领域的资深标注专家。请严格按照以下 15 个标准风险分类对给定条款进行判别。

## 输出格式要求
- 仅返回 JSON 对象，不要任何额外文字、说明或 Markdown 代码块
- 字段严格按以下 Schema：
{
  "category_code": "必须是 15 个类别码之一 或 NO_RISK",
  "is_critical": true/false,
  "confidence": 0.0~1.0,
  "evidence_quote": "从原文中精确截取的关键证据片段（不超过 60 字）",
  "reasoning": "简洁说明判定理由，1-2 句",
  "severity": "high 或 medium",
  "alternative_categories": ["可选次选类别，最多 2 个"]
}

## Critical 规则
以下 5 类（C01-C05）均为 Critical = true：
LOCAL_REGISTRATION, BRAND_LOCK, UNRELATED_CERT, REGIONAL_PERFORMANCE, SCALE_THRESHOLD
其余 10 类 Critical = false

## 15 类判别标准（含触发条件）

### C01 LOCAL_REGISTRATION — 地域注册限制
触发：将"本地注册/营业执照/分支机构/纳税登记"作为资格条件，排斥外地企业。
关键词组合：(本市/本区/本县/所在地/异地/采购人所在) + (注册/分公司/营业执照/纳税) + (须/必须/仅限/不接受/资格/退回)

### C02 BRAND_LOCK — 指定品牌且不接受同等产品
触发：直接指定唯一品牌/型号/厂牌，并明确排除同等/兼容/等效产品。
关键词组合：(品牌/商标/型号/厂牌/系列/原装/机型/指定机型) + (仅/只能/唯一/指定/不接受/不得偏离/不予响应/只准/不得)

### C03 UNRELATED_CERT — 设置与履约无关的资格条件
触发：将与采购标的履约能力无直接关系的认证/荣誉/资质作为资格门槛。
关键词组合：(认证/证书/荣誉/示范/百强/五星) + (资格/无效/废标/不通过/必须/须) + 证据显示与标的无关

### C04 REGIONAL_PERFORMANCE — 特定区域业绩限制
触发：限定"特定行政区域内"的业绩/案例/合同才计分或认可，排斥全国供应商。
关键词组合：(本市/本区/本县/本省/当地/省外/异地) + (业绩/案例/合同) + (只统计/不认可/不予认可/不计分/须/必须)

### C05 SCALE_THRESHOLD — 以经营规模设置资格门槛
触发：把注册资本/营业收入/资产总额/净资产等规模指标作为资格条件。
关键词组合：(注册资本/营业收入/资产总额/净资产/实缴资本/实收资本/业务收入/主营/规模) + (不得低于/不少于/以上/门槛/资格/达到/未达到)

### H01 SHORT_DEADLINE — 投标准备期不足
触发：公开招标从文件发出至投标截止明显短于法定期限（<20 日），或明确不顺延。
触发：条款含具体日期差 < 20 日，或"自下载之日起 X 日"中 X < 20，或"不因节假日顺延"。

### H02 EXCESSIVE_DEPOSIT — 投标保证金比例过高
触发：投标保证金 > 项目预算金额的 2%，或只允许现金/基本账户等不合理支付限制。
触发：出现具体比例 >2%，或"预算X万，保证金Y万"中 Y/X > 2%，或"只允许现金/基本户"。

### H03 OEM_AUTHORIZATION — 将厂家授权作为资格条件
触发：对非进口货物，将原厂/制造商授权/承诺函/背书函作为资格审查必备材料。
关键词组合：(原厂/厂家/制造商) + (授权/承诺函/背书) + (资格/必须/须/必备/终止审查/无效)

### H04 SUBJECTIVE_SCORING — 主观评分未细化量化
触发：评分只用"优/良/一般/较好/非常好/总体感觉"等主观描述，无量化分档条件。
关键词组合：(评委认为/非常好/较好/普通/美观性/感染力/总体感觉/自由给分) + (无/未/未列明/不设置) + (可核验条件/评分刻度/量化指标)

### H05 LOCAL_AWARD — 本地奖项加分
触发：仅"本省市/本区/本县"政府部门颁发的荣誉/奖项加分，外地同级别不加分。
关键词组合：(本市/本省/本区/本县/本地) + (荣誉/奖项/获奖/称号) + (加分/得分/评分/分)

### M01 VAGUE_ACCEPTANCE — 验收标准模糊
触发：验收以"采购人满意/口头确认/感觉/不说明理由"为准，无客观指标、方法、程序。
关键词组合：(满意为准/口头确认/感觉满意/不说明理由/不另设测试/不设指标)

### M02 UNBOUNDED_IP — 知识产权责任无限扩大
触发：要求转让背景知识产权，或赔偿金额不设上限，或采购人指定素材侵权仍由供应商全责。
关键词组合：(永久无偿转让/无限额索赔/一切责任/不设最高限额/全部无限额) + (知识产权/软件/算法/侵权/素材)

### M03 UNILATERAL_CHANGE — 采购人可单方无限变更需求
触发：采购人可"随时/任意/单方"增加服务或功能，且"不得增加费用/延长工期/自动包含在原价内"。
关键词组合：(随时增加/任意数量/不得增加费用/单方决定/自动包含/不得延长)

### M04 CONFLICTING_DATES — 关键日期相互矛盾
触发：同一条款或不同章节对同一关键日期（收件/开标/答疑截止）给出两个以上冲突规定。
触发：条款同时出现两个不同日期，用于同一事件（如"X日停止收件，Y日以后不再接收"且X≠Y）

### M05 UNCLEAR_PENALTY — 违约责任口径不清
触发：违约金比例/基数/触发条件/累计上限不清，可由"采购人任意/自行/任选"决定。
关键词组合：(采购人认为不适当/任选/自行确定/未规定计算基数/未规定触发条件/未规定累计上限)

## 判别原则
1. 选择最匹配的单一类别，宁缺毋滥
2. 若条款无风险或无法确定任何类别，选 NO_RISK
3. confidence：完全确定 ≥0.9，有较大把握 0.7~0.89，存疑 0.5~0.69
4. NO_RISK 的 confidence 一般 ≤ 0.5
"#;

// ── 盲-v2 FN 列表（基于 blind_validate 运行结果：TP=19 FN=11） ─

/// blind-v2 中规则引擎 category 未命中的 finding_id 列表（共 11 个）
const FN_FINDING_IDS: &[&str] = &[
    // EXCESSIVE_DEPOSIT x2
    "BLIND-002-F02",
    "BLIND-007-F02",
    // SUBJECTIVE_SCORING x2
    "BLIND-004-F02",
    "BLIND-009-F02",
    // CONFLICTING_DATES x2
    "BLIND-004-F03",
    "BLIND-009-F03",
    // UNILATERAL_CHANGE x2
    "BLIND-003-F03",
    "BLIND-008-F03",
    // SHORT_DEADLINE x1
    "BLIND-006-F02",
    // VAGUE_ACCEPTANCE x1
    "BLIND-006-F03",
    // UNBOUNDED_IP x1
    "BLIND-002-F03",
];

/// Map blind-v2 code (e.g. "H02_EXCESSIVE_DEPOSIT") → canonical
fn canonical(blind_code: &str) -> &str {
    blind_code
        .find('_')
        .map(|i| &blind_code[i + 1..])
        .unwrap_or(blind_code)
}

// ── 主逻辑 ──────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // 0. 环境初始化
    dotenv::dotenv().ok();
    if let Some(parent) = std::env::current_dir()
        .ok()
        .and_then(|d| d.parent().map(|p| p.join(".env")))
    {
        if parent.exists() {
            dotenv::from_path(&parent).ok();
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let specific_ids: HashSet<String> = if !args.is_empty() {
        eprintln!("??  模式：仅对指定 finding_id 运行 LLM：{:?}", args);
        args.iter().cloned().collect()
    } else {
        eprintln!("??  模式：对 blind-v2 全部 11 个 FN 条款运行 LLM 预标注");
        FN_FINDING_IDS.iter().map(|s| s.to_string()).collect()
    };

    // 1. 加载 annotations.jsonl
    let ann_path = "../benchmark/blind-v2/data/annotations.jsonl";
    let raw = std::fs::read_to_string(ann_path)
        .with_context(|| format!("读取 annotations 失败: {ann_path}"))?;
    let all_anns: Vec<BlindAnnotation> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<BlindAnnotation>(l).context("解析 annotation JSON 失败"))
        .collect::<Result<Vec<_>>>()?;

    eprintln!("Loaded {} ground truth annotations", all_anns.len());

    // 2. 筛选出 FN 子集
    let fn_anns: Vec<&BlindAnnotation> = all_anns
        .iter()
        .filter(|a| specific_ids.contains(&a.finding_id))
        .collect();

    eprintln!(
        "筛选出 {} 个 FN 条款进行 LLM 预标注",
        fn_anns.len()
    );

    // 3. 创建 LLM 客户端（使用 services/llm_client 的 create_llm_client）
    let llm = ai_bid::services::llm_client::create_llm_client()
        .context("创建 LLM 客户端失败（请检查 DASHSCOPE_API_KEY / OPENAI_API_KEY）")?;
    eprintln!("LLM 客户端已就绪（协议由 AIBID_LLM_PROTOCOL 决定）");

    // 4. 逐个跑 LLM
    let mut rows: Vec<FnContrastRow> = Vec::new();
    let mut success = 0usize;
    let mut errors = 0usize;
    let mut matches_gt = 0usize;
    let mut confusion: Vec<(String, String, usize)> = Vec::new();

    for (idx, ann) in fn_anns.iter().enumerate() {
        eprintln!(
            "\n[{}/{}] 标注 {} — GT:{} ({})",
            idx + 1,
            fn_anns.len(),
            ann.finding_id,
            canonical(&ann.category_code),
            ann.risk_type
        );
        eprintln!("    原文: {}", ann.source_quote.chars().take(60).collect::<String>());

        let section_title = ann
            .section_path
            .as_ref()
            .and_then(|p| p.first())
            .cloned()
            .unwrap_or_else(|| "采购文件补充条款".to_string());

        let user_prompt = format!(
            r#"请对以下政府采购条款进行风险分类标注：

条款原文：
"""
{clause}
"""

章节上下文：{section}
来源文档：{doc_id}
条款编号：{fid}

请严格按照 System Prompt 中定义的 15 类标准进行判别。返回 JSON。"#,
            clause = ann.source_quote,
            section = section_title,
            doc_id = ann.document_id,
            fid = ann.finding_id,
        );

        let messages = vec![
            ChatMessage::System {
                content: SYSTEM_PROMPT.to_string(),
            },
            ChatMessage::User {
                content: user_prompt,
            },
        ];

        let tools: Vec<serde_json::Value> = Vec::new(); // 纯文本 JSON 输出，不使用 function calling

        match llm.chat(&messages, &tools, &ToolChoice::Auto).await {
            Ok(resp) => {
                let content = resp.content.clone().unwrap_or_default();
                // 尝试解析 JSON：处理可能的 ```json ... ``` 包裹
                let json_str = strip_json_fences(&content);
                match serde_json::from_str::<LlmAnnotation>(&json_str) {
                    Ok(llm_ann) => {
                        success += 1;
                        let gt_cat = canonical(&ann.category_code).to_string();
                        let llm_cat = normalize_llm_code(&llm_ann.category_code);
                        let llm_display = display_name(&llm_cat).unwrap_or("—").to_string();
                        let gt_display = display_name(&gt_cat).unwrap_or("—").to_string();
                        let matches = llm_cat == gt_cat;
                        if matches {
                            matches_gt += 1;
                            eprintln!("    ? LLM 命中 GT: {gt_cat} (conf={:.2})", llm_ann.confidence);
                        } else {
                            eprintln!(
                                "    ?  LLM 判定: {}  GT: {} (conf={:.2})",
                                llm_cat, gt_cat, llm_ann.confidence
                            );
                        }

                        // Confusion matrix 累计
                        if let Some(entry) = confusion
                            .iter_mut()
                            .find(|(g, l, _)| g == &gt_cat && l == &llm_cat)
                        {
                            entry.2 += 1;
                        } else {
                            confusion.push((gt_cat.clone(), llm_cat.clone(), 1));
                        }

                        rows.push(FnContrastRow {
                            finding_id: ann.finding_id.clone(),
                            document_id: ann.document_id.clone(),
                            section_title,
                            clause_text: ann.source_quote.clone(),
                            gt_category: gt_cat.clone(),
                            gt_category_display: gt_display,
                            gt_is_critical: ann.is_critical,
                            gt_severity: ann.severity.clone(),
                            rule_engine_status: "FN".to_string(),
                            llm_category: llm_cat.clone(),
                            llm_category_display: llm_display,
                            llm_is_critical: llm_ann.is_critical,
                            llm_confidence: llm_ann.confidence,
                            llm_severity: llm_ann.severity.clone(),
                            llm_evidence_quote: llm_ann.evidence_quote.clone(),
                            llm_reasoning: llm_ann.reasoning.clone(),
                            llm_alternatives: llm_ann.alternative_categories.clone(),
                            llm_matches_gt: matches,
                            review_status: "PENDING_REVIEW".to_string(),
                        });
                    }
                    Err(e) => {
                        errors += 1;
                        eprintln!("    ? LLM JSON 解析失败: {e}\n    Raw: {content}");
                        rows.push(error_row(ann, &section_title, &format!("JSON parse error: {e}")));
                    }
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("    ? LLM API 调用失败: {e}");
                rows.push(error_row(ann, &section_title, &format!("API error: {e}")));
            }
        }
    }

    // 5. 写输出文件
    let output_path = "../benchmark/blind-v2/data/llm_fn_annotations.json";
    let total = rows.len();
    let match_rate = if total > 0 {
        matches_gt as f64 / total as f64
    } else {
        0.0
    };
    let summary = LabelSummary {
        total_fn_count: FN_FINDING_IDS.len(),
        llm_called_count: total,
        llm_success_count: success,
        llm_error_count: errors,
        llm_matches_gt_count: matches_gt,
        llm_match_rate: match_rate,
        confusion_matrix: confusion.clone(),
        output_file: output_path.to_string(),
    };

    let report = json!({
        "summary": summary,
        "prompt_schema": {
            "system_prompt_lines": SYSTEM_PROMPT.lines().count(),
            "output_fields": ["category_code", "is_critical", "confidence", "evidence_quote", "reasoning", "severity", "alternative_categories"],
            "critical_categories": ["LOCAL_REGISTRATION", "BRAND_LOCK", "UNRELATED_CERT", "REGIONAL_PERFORMANCE", "SCALE_THRESHOLD"],
        },
        "fn_contrast_rows": rows,
    });

    std::fs::write(output_path, serde_json::to_string_pretty(&report).unwrap())
        .with_context(|| format!("写输出文件失败: {output_path}"))?;
    eprintln!("\n? 结果已写入: {output_path}");

    // 6. 打印汇总到 stderr
    eprintln!("\n╔══════════════════════════════════════════════╗");
    eprintln!("║  LLM 预标注 — FN 条款三方对照报告             ║");
    eprintln!("╚══════════════════════════════════════════════╝");
    eprintln!("  Total FN 条款:        {}", FN_FINDING_IDS.len());
    eprintln!("  LLM 成功:             {success}");
    eprintln!("  LLM 错误:             {errors}");
    eprintln!("  LLM 命中 GT:          {matches_gt}/{total}");
    eprintln!("  LLM-GT 一致率:        {:.1}%", match_rate * 100.0);
    eprintln!("  ─────────────────────────────────────────────");
    eprintln!("  Confusion (GT → LLM):");
    confusion.sort_by(|a, b| b.2.cmp(&a.2));
    for (gt, llm, cnt) in &confusion {
        let marker = if gt == llm { "?" } else { "? " };
        eprintln!(
            "    {}  {:<22} → {:<22}  x{}",
            marker,
            gt,
            llm,
            cnt
        );
    }
    eprintln!("  ─────────────────────────────────────────────");
    eprintln!("  下一步：人工复核 llm_fn_annotations.json 中 review_status=PENDING_REVIEW 的条目");
    eprintln!("  输出文件: {output_path}");

    // JSON 到 stdout（便于脚本化）
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    Ok(())
}

// ── 辅助函数 ────────────────────────────────────────────────────

fn strip_json_fences(s: &str) -> String {
    let trimmed = s.trim();
    // 情况 1: ```json ... ```
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
    }
    // 情况 2: ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
    }
    // 情况 3: 尝试找到第一个 '{' 和最后一个 '}'
    let first = trimmed.find('{').unwrap_or(0);
    let last = trimmed.rfind('}').unwrap_or(trimmed.len() - 1);
    trimmed[first..=last].to_string()
}

/// 盲-v2 前缀（C01~M05）到 canonical code 的固定映射
const PREFIX_TO_CANONICAL: &[(&str, &str)] = &[
    ("C01", "LOCAL_REGISTRATION"),
    ("C02", "BRAND_LOCK"),
    ("C03", "UNRELATED_CERT"),
    ("C04", "REGIONAL_PERFORMANCE"),
    ("C05", "SCALE_THRESHOLD"),
    ("H01", "SHORT_DEADLINE"),
    ("H02", "EXCESSIVE_DEPOSIT"),
    ("H03", "OEM_AUTHORIZATION"),
    ("H04", "SUBJECTIVE_SCORING"),
    ("H05", "LOCAL_AWARD"),
    ("M01", "VAGUE_ACCEPTANCE"),
    ("M02", "UNBOUNDED_IP"),
    ("M03", "UNILATERAL_CHANGE"),
    ("M04", "CONFLICTING_DATES"),
    ("M05", "UNCLEAR_PENALTY"),
];

fn normalize_llm_code(code: &str) -> String {
    // LLM 可能返回：中文名 / 带前缀的代码(C02_BRAND_LOCK) / 纯前缀(M02) / 英文 canonical
    // 第一步：过滤并标准化字符
    let upper: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .flat_map(|c| {
            if c == '-' {
                vec!['_']
            } else {
                c.to_uppercase().collect::<Vec<_>>()
            }
        })
        .collect();

    if upper.is_empty() {
        return "NO_RISK".to_string();
    }

    // 情况 1：形如 C02_BRAND_LOCK —— 去前缀后检查 rest 是否为合法类别名
    if let Some((prefix, rest)) = upper.split_once('_') {
        if prefix.len() == 3
            && prefix.starts_with(|c: char| c.is_ascii_alphabetic())
            && prefix[1..].chars().all(|c| c.is_ascii_digit())
        {
            if !rest.is_empty() {
                return rest.to_string();
            }
            // rest 为空（如 "M03_"），退回前缀映射
            if let Some((_, canonical)) = PREFIX_TO_CANONICAL
                .iter()
                .find(|(p, _)| *p == prefix)
            {
                return (*canonical).to_string();
            }
        }
    }

    // 情况 2：纯前缀（长度=3，字母+2位数字，例如 M02 H04）
    if upper.len() == 3
        && upper.starts_with(|c: char| c.is_ascii_alphabetic())
        && upper[1..].chars().all(|c| c.is_ascii_digit())
    {
        if let Some((_, canonical)) = PREFIX_TO_CANONICAL
            .iter()
            .find(|(p, _)| *p == upper)
        {
            return (*canonical).to_string();
        }
    }

    // 情况 3：中文名或其他形式 → 尝试简单关键词匹配（兜底）
    let lower_original = code.to_lowercase();
    for (_p, canonical) in PREFIX_TO_CANONICAL {
        // 取 canonical code 中每个单词做模糊匹配
        let keywords: Vec<&str> = canonical.split('_').collect();
        if keywords
            .iter()
            .any(|k| lower_original.contains(&k.to_lowercase()))
        {
            return (*canonical).to_string();
        }
    }

    // 情况 4：已是 canonical（如 EXCESSIVE_DEPOSIT），或无法识别
    let valid_canonical = PREFIX_TO_CANONICAL.iter().any(|(_, c)| *c == upper);
    if valid_canonical {
        return upper;
    }
    if upper == "NORISK" || upper == "NO_RISK" || upper == "NONE" {
        return "NO_RISK".to_string();
    }
    upper
}

fn error_row(ann: &BlindAnnotation, section: &str, err: &str) -> FnContrastRow {
    let gt = canonical(&ann.category_code).to_string();
    FnContrastRow {
        finding_id: ann.finding_id.clone(),
        document_id: ann.document_id.clone(),
        section_title: section.to_string(),
        clause_text: ann.source_quote.clone(),
        gt_category: gt.clone(),
        gt_category_display: display_name(&gt).unwrap_or("—").to_string(),
        gt_is_critical: ann.is_critical,
        gt_severity: ann.severity.clone(),
        rule_engine_status: "FN".to_string(),
        llm_category: "ERROR".to_string(),
        llm_category_display: "LLM 调用失败".to_string(),
        llm_is_critical: false,
        llm_confidence: 0.0,
        llm_severity: "error".to_string(),
        llm_evidence_quote: String::new(),
        llm_reasoning: err.to_string(),
        llm_alternatives: Vec::new(),
        llm_matches_gt: false,
        review_status: "LLM_ERROR".to_string(),
    }
}
