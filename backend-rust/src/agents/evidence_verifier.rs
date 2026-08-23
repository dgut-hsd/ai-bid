//! EvidenceVerifier — 证据核验器（证伪导向 NLI 三分类）。
//!
//! 在 LegalVerify / Debate 之后、Triage 之前，对每条 Verified finding
//! 仅凭 source_quote + risk_type 做独立裁决（不喂 Agent 的 reason，避免被错误论证带偏）：
//! - support      → 证据成立，放行（保留原 severity）
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

【已由人工复核的判例，供你校准】：
1. "供应商注册地为茂名市的，每提供一个业绩另加1分" → support（注册地直接作差异化加分，构成地域歧视）
2. "仅限华润、林德、空气产品等品牌，其他品牌不得分" → support（明确指定品牌且排斥同等产品）
3. "酌情给分，最高2分，且不设具体量化标准" → support（明确主观、未量化）
4. "本采购包不接受联合体投标" → refute（采购人有权不接受联合体，不构成排斥供应商）
5. "本项目气体产品不允许采购进口产品" → insufficient（不允许进口是本国产品政策的合规方向，非违规）
6. "给予1%-5%的价格扣除，具体比例根据重要性确定" → refute（1-5%是法定政策区间，非标准不明确）
7. "广东省内的电子认证服务机构签发的CA数字证书" → insufficient（省域CA属行业惯例，不足以判定地域歧视）

只输出一行 JSON，禁止任何多余文字：{"verdict":"support|refute|insufficient","reason":"一句话"}"#;

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

/// 单次 LLM 调用做证据核验，返回 (verdict, reason)。
/// verdict ∈ {"support", "refute", "insufficient"}；调用/解析失败返回 None。
pub async fn verify_evidence(
    llm: &dyn LlmClient,
    quote: &str,
    risk_type: &str,
) -> Option<(String, String)> {
    let quote_trunc: String = quote.chars().take(800).collect();
    let messages = vec![
        ChatMessage::System {
            content: EVIDENCE_VERIFIER_SYSTEM_PROMPT.to_string(),
        },
        ChatMessage::User {
            content: format!("【招标文件原文】\n{}\n\n【待核风险类型】\n{}", quote_trunc, risk_type),
        },
    ];
    let resp = llm.chat(&messages, &[], &ToolChoice::Auto).await.ok()?;
    let content = resp.content?;
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if end <= start {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(&content[start..=end]).ok()?;
    let verdict = parsed.get("verdict")?.as_str()?.to_string();
    let reason = parsed
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((verdict, reason))
}
