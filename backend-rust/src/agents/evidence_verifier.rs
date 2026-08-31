//! EvidenceVerifier — 证据核验器（证伪导向 NLI 三分类）。
//!
//! 在 LegalVerify / Debate 之后、Triage 之前，对每条 Verified finding
//! 仅凭 source_quote + risk_type 做独立裁决（不喂 Agent 的 reason，避免被错误论证带偏）：
//! - support      → 证据成立，放行（保留原 severity；severity=medium 时"只降不升"）
//! - refute       → 原文相反/合规，降级 Info（"疑似"，折叠不默认展示）
//! - insufficient → 证据不足，降级 Info（"疑似"）
//!
//! 设计动机与离线实验结果见 benchmark/natural-errors/verifier_error_cases.csv。
//! 三轮迭代（v1 含 reason → v2 去 reason+去重 → v3 +few-shot）结论：
//!   去 reason 治"被错误论证带偏"+"同句原文自相矛盾"；
//!   few-shot 治"脱离 reason 后脑补违规"（联合体/进口/价格扣除等）。
//! 99 条 finding 最终收敛到 7 条 support（3 个真问题），precision 100%。

use crate::agents::react_loop::{ChatMessage, LlmClient, ToolChoice};

/// 证据核验 System Prompt（含 7 条人工复核的 few-shot 校准判例）。
pub const EVIDENCE_VERIFIER_SYSTEM_PROMPT: &str = r#"你是政府采购招标文件审核的独立复核员。给定【招标文件原文】和【待核风险类型】，仅依据原文字面内容，判断"原文是否确实构成该类违规"。三选一：
- support：原文明确、逐字包含该类违规的硬性表述，足以直接推出违规。
- refute：原文与该风险类型相反，或原文写明的是合法合规、无歧视、无排他的做法。
- insufficient：原文是中性、正当、常规的要求，无法仅凭字面推出该违规。

硬规则：
1. 从中性/正当要求推断出歧视或排他，一律 insufficient。
2. 原文没有明确违规定语就不得判 support。
3. 严禁脑补、联想，判断必须落到原文具体字句。
4. 论证与结论必须一致：若你的 reason 承认"原文未明确/未细化/未提及/无法判断/属常规正当条款"，则 verdict 只能 insufficient，严禁判 support。
5. 判断"口径不清/标准模糊/未细化量化/不明确"类风险时，只要原文已给出具体量化公式、明确数字或明确例外情形，就判 refute 或 insufficient，严禁因"表述复杂/未逐字解释"判 support。
6. 遇到"权重/总分/分值占比/价格分/商务分/技术分/报价分"类风险，必须把原文中的数字逐项相加核对：合计=100 判 refute，合计≠100 判 support。严禁因原文未逐字写出"不闭合/不等于100"就判 insufficient。
7. verdict=support 时必须给出 severity：只有"必须修改/红线级"（指定品牌、地域歧视、主观评分完全无量化、无限责任、权重和≠100）才判 high；有据但不致命的（连带扣分、免责措辞过宽、价格扣除比例按重要性确定、★号设置不当、逻辑矛盾）一律 medium。

【已由人工复核的判例，供你校准】：
1. "供应商注册地为茂名市的，每提供一个业绩另加1分" → support（注册地直接作差异化加分，构成地域歧视）
2. "仅限华润、林德、空气产品等品牌，其他品牌不得分" → support、severity=high（明确指定品牌且排斥同等产品）
3. "酌情给分，最高2分，且不设具体量化标准" → support、severity=high（明确主观、未量化）
4. "本采购包不接受联合体投标" → refute（采购人有权不接受联合体，不构成排斥供应商）
5. "本项目气体产品不允许采购进口产品" → insufficient（不允许进口是本国产品政策的合规方向，非违规）
6. "给予1%-5%的价格扣除，具体比例根据重要性确定" → refute（1-5%是法定政策区间，非标准不明确）
7. "广东省内的电子认证服务机构签发的CA数字证书" → insufficient（省域CA属行业惯例，不足以判定地域歧视）
8. "投标人漏报的单价均视为此项费用已包含在投标报价中，如中标不得再收取任何费用" → insufficient（总价包干/漏报不追加是合法常规商务条款，非违约责任违规）
9. "评价情况至少须为满意或好评或打分制为85分" → insufficient（85分是客观量化门槛，非主观评分，不得判主观评分未细化）
10. "采购人逾期付款按逾期金额×LPR÷365 逐日偿付违约金，中标人自身原因除外" → refute（违约计算方式明确且双向，不构成口径不清）
11. "评审因素分值构成：商务部分14分、技术部分56分、报价得分25分" → support、severity=high（14+56+25=95≠100，权重和不闭合）
12. "对获得节能产品认证证书的产品给予1%的价格扣除，具体扣除比例根据重要性、所占比重等因素确定" → support、severity=medium（比例依据主观裁量，但属政策区间内，非红线）
13. "因国家政策或不可抗力导致采购人不能执行所购货物时，合同终止，采购人及采购代理机构不承担任何责任" → support、severity=medium（免责措辞过宽，但不构成无限责任红线）

只输出一行 JSON，禁止任何多余文字：{"verdict":"support|refute|insufficient","severity":"high|medium","reason":"一句话"}"#;

/// 提取原文核心句作为去重 key（同一处原文只裁决一次）。
/// 简化实现：按句分隔符切分，取包含关键违规词的最长段；无匹配则用全文前 120 字符。
pub fn evidence_core_key(q: &str) -> String {
    const KEYS: &[&str] = &[
        "另加", "仅限", "指定", "不得", "须不低于", "酌情", "注册地", "加分", "品牌", "注册资本",
        "只接受", "唯一",
    ];
    let mut best: String = String::new();
    for seg in q.split(|c| matches!(c, '。' | '；' | '\n')) {
        if seg.is_empty() || !KEYS.iter().any(|k| seg.contains(k)) {
            continue;
        }
        let norm: String = seg.chars().filter(|c| !c.is_whitespace()).collect();
        if norm.len() > best.len() {
            best = norm;
        }
    }
    if best.is_empty() {
        best = q.chars().filter(|c| !c.is_whitespace()).collect();
    }
    best.chars().take(120).collect()
}

/// 证据核验裁决：verdict 三分类 + 一句话依据 + 可选的 severity 降级建议。
#[derive(Debug, Clone)]
pub struct EvidenceVerdict {
    /// support | refute | insufficient
    pub verdict: String,
    /// 一句话理由
    pub reason: String,
    /// 仅 verdict=support 时有效：high | medium，用于"只降不升"的 severity 校准。
    pub severity: Option<String>,
}

/// 单次 LLM 调用做证据核验，返回 EvidenceVerdict。
/// verdict ∈ {"support", "refute", "insufficient"}；调用/解析失败返回 None。
pub async fn verify_evidence(
    llm: &dyn LlmClient,
    quote: &str,
    risk_type: &str,
) -> Option<EvidenceVerdict> {
    let quote_trunc: String = quote.chars().take(800).collect();
    let messages = vec![
        ChatMessage::System {
            content: EVIDENCE_VERIFIER_SYSTEM_PROMPT.to_string(),
        },
        ChatMessage::User {
            content: format!("【招标文件原文】\n{}\n\n【待核风险类型】\n{}", quote_trunc, risk_type),
        },
    ];
    let resp = match llm.chat(&messages, &[], &ToolChoice::Auto).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [EvidenceVerify] llm.chat Err: {:#}", e);
            return None;
        }
    };
    let content = match resp.content {
        Some(c) => c,
        None => {
            eprintln!("  [EvidenceVerify] content 为 None");
            return None;
        }
    };
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if end <= start {
        return None;
    }
    let parsed: serde_json::Value = match serde_json::from_str(&content[start..=end]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("  [EvidenceVerify] JSON 解析失败: {} | 原文: {}", e, &content[start..end.min(content.len())]);
            return None;
        }
    };
    let verdict = parsed.get("verdict")?.as_str()?.to_string();
    let reason = parsed
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let severity = parsed
        .get("severity")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some(EvidenceVerdict {
        verdict,
        reason,
        severity,
    })
}

// ─── 确定性权重/分值构成核验（零 LLM）──────────────────────────────

/// 判定该 finding 是否属于"权重/总分/分值构成"类。
/// 命中的会走确定性数值核验（按 clause 全文求和比 100），不必再喂 LLM。
pub fn is_weight_related(category_code: &str, risk_type: &str) -> bool {
    let cc = category_code.to_ascii_lowercase();
    let code_hit = cc.contains("weight")
        || cc.contains("total_score")
        || cc.contains("score_sum")
        || cc.contains("price_weight");
    let text_hit = [
        "权重", "总分", "分值构成", "评分构成", "分值占比",
        "价格分", "商务分", "技术分", "报价分",
    ]
    .iter()
    .any(|k| risk_type.contains(k));
    code_hit || text_hit
}

/// 确定性数值核验结论：`closed=true` 表示组件分值合计 == 100（合规），
/// `false` 表示合计 ≠ 100（权重和不闭合）。`sum` 为实际合计。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightSumOutcome {
    pub closed: bool,
    pub sum: f64,
}

/// 从 clause 全文抽取"商务分/技术分/报价分(价格分)"三个组件分值并求和比 100。
///
/// 只有三类组件都能从原文稳定抽出时才给结论（return `Some`）；任一缺失则返回 `None`，
/// 由调用方回退 NLI——避免把"只写了部分分值"误判成不合规，也避免把其他"X分"污染进来。
pub fn deterministic_weight_sum_check(text: &str) -> Option<WeightSumOutcome> {
    let biz = extract_component_score(text, &["商务"])?;
    let tech = extract_component_score(text, &["技术"])?;
    let price = extract_component_score(text, &["报价", "价格"])?;
    let sum = biz + tech + price;
    let closed = (sum - 100.0).abs() < 1e-6;
    Some(WeightSumOutcome { closed, sum })
}

/// 格式化合计分值：整数不带小数，非整数保留一位。
pub fn fmt_weight_sum(sum: f64) -> String {
    if (sum - sum.round()).abs() < 1e-6 {
        format!("{}", sum.round() as i64)
    } else {
        format!("{:.1}", sum)
    }
}

/// 抽出"组件词 + (部分/得分/评分/分值/权重/占比/项)? + ≤4 个无关字符 + 数字 + 分"的第一个分值。
/// 例："技术部分56.0分"→56.0；"报价得分25.0分"→25.0；"商务30分"→30.0。
/// 故意限制组件词与数字之间的字符数，避免把"技术参数…共2项…15分"这类无关数字抓进来。
fn extract_component_score(text: &str, aliases: &[&str]) -> Option<f64> {
    for alias in aliases {
        let pattern = format!(
            r"{}(?:部分|得分|评分|分值|权重|占比|项)?[^0-9分]{{0,4}}(\d+(?:\.\d+)?)\s*分",
            regex::escape(alias)
        );
        let re = regex::Regex::new(&pattern).ok()?;
        if let Some(caps) = re.captures(text) {
            if let Some(m) = caps.get(1) {
                if let Ok(v) = m.as_str().parse::<f64>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_sum_mismatch_95() {
        let t = "评审因素分值构成：商务部分14.0分、技术部分56.0分、报价得分25.0分";
        let o = deterministic_weight_sum_check(t).expect("应能抽出三类组件");
        assert!(!o.closed, "95 不应闭合");
        assert!((o.sum - 95.0).abs() < 1e-6, "合计应为 95，实际 {}", o.sum);
        assert_eq!(fmt_weight_sum(o.sum), "95");
    }

    #[test]
    fn weight_sum_closed_100() {
        let t = "商务部分30分、技术部分30分、报价得分40分";
        let o = deterministic_weight_sum_check(t).expect("应能抽出三类组件");
        assert!(o.closed);
        assert!((o.sum - 100.0).abs() < 1e-6);
    }

    #[test]
    fn weight_sum_missing_component_indeterminate() {
        // 只有报价分，缺商务/技术 → 无法判断 → None（退回 NLI）
        assert!(deterministic_weight_sum_check("报价得分25.0分").is_none());
        assert!(deterministic_weight_sum_check("商务部分14分").is_none());
        assert!(deterministic_weight_sum_check("技术部分56分 报价得分25分").is_none());
    }

    #[test]
    fn weight_sum_ignores_unrelated_scores() {
        // 完整 ch_122 原文：含 ★号 15分/7.5分/共2项 等无关数字，必须只取 14/56/25
        let t = "3.详细评审采购包1(广东省第二中医院各类医用气体采购项目):评审因评审标准素分值构商务部分14.0分成技术部分56.0分报价得分25.0分技术部所投货物对采购需求中根据投标人对“第二章采购需求”中“具体技术(参数)要分带▲号的重要技术参数求”的重要技术参数（带“▲”技术参数，共2项，总共的符合性(15.0分)15分）的响应程度进行评审：标注“▲”的重要技术参数，该项为“正偏离”或“符合”或“无偏离”的，该项得7.5分；响应为“负偏离”或不响应的，该项不得分。";
        let o = deterministic_weight_sum_check(t).expect("应能抽出三类组件");
        assert!(!o.closed);
        assert!((o.sum - 95.0).abs() < 1e-6, "无关的 15/7.5/2 不应污染合计，实际 {}", o.sum);
    }

    #[test]
    fn is_weight_related_matches_code_and_text() {
        assert!(is_weight_related("SCORING_PRICE_WEIGHT_VIOLATION", "价格分权重违规"));
        assert!(is_weight_related("SOMETHING_ELSE", "技术分权重违规"));
        assert!(!is_weight_related("BRAND_LOCK", "指定品牌"));
    }
}
