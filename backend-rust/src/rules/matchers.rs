//! Three-pattern matchers — keyword / regex / field_compare.
//!
//! Driven by TDD — every matcher has positive / negative / boundary cases.

use crate::rules::metrics::DocumentMetrics;
use crate::rules::schema::Pattern;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Mutex;

// ── Regex pre-compile cache (global Mutex, cheap lock) ───────
//
// We use a process-level cache keyed by the *original* regex string (before
// \uXXXX normalization).  A Mutex-based HashMap is acceptable here:
// rule evaluation happens in the < 1ms budget; cache locks are ~ns range.

fn regex_cache() -> &'static Mutex<HashMap<String, Option<Regex>>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, Option<Regex>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Normalize JS-style `\uXXXX` escapes into Rust `\x{XXXX}`.
pub fn normalize_regex(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 5 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'u' {
                // Collect 4 hex digits after \u
                let hex_start = i + 2;
                let hex_end = hex_start + 4;
                if hex_end <= bytes.len() {
                    let hex = &pattern[hex_start..hex_end];
                    if (0..4).all(|k| {
                        let c = bytes[hex_start + k];
                        c.is_ascii_hexdigit()
                    }) {
                        out.push_str("\\x{");
                        out.push_str(hex);
                        out.push('}');
                        i = hex_end;
                        continue;
                    }
                }
            }
        }
        // Safe: push the byte as char when ASCII, else decode UTF-8 properly.
        // We work on chars from pattern to preserve non-ASCII Chinese directly.
        // Walk chars instead for correctness.
        break;
    }
    // Above byte-walk was careful but breaks on multi-byte.  Use a char-based
    // scan for the remainder — which is the common case.  Restart char-iterate
    // from scratch for simplicity (regex patterns are short, ~100 chars max).
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 5 < chars.len() && chars[i + 1] == 'u' {
            // Check the next 4 chars are hex
            let hex: String = chars[i + 2..i + 6].iter().collect();
            if hex.chars().all(|c| c.is_ascii_hexdigit()) {
                result.push_str("\\x{");
                result.push_str(&hex);
                result.push('}');
                i += 6;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    let _ = out; // silence unused
    result
}

/// Compile a regex (or retrieve from cache).
/// Returns None on compile failure — caller must treat as "no match" (graceful
/// degradation, single broken regex must not crash the rule engine).
fn compile_regex(pattern: &str) -> Option<Regex> {
    let key = pattern.to_string();
    let mut cache = regex_cache().lock().expect("regex cache poisoned");
    if let Some(entry) = cache.get(&key) {
        return entry.clone();
    }
    let normalized = normalize_regex(pattern);
    let compiled = Regex::new(&normalized).ok();
    cache.insert(key, compiled.clone());
    compiled
}

// ── Public keyword helpers ────────────────────────────────────

pub fn keyword_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}

pub fn keyword_all(text: &str, words: &[&str]) -> bool {
    words.iter().all(|word| text.contains(word))
}

// ── Public evaluate helpers ──────────────────────────────────

/// Evaluate a single Pattern — field_compare uses zero/default metrics.
pub fn evaluate_pattern(text: &str, pattern: &Pattern) -> bool {
    evaluate_pattern_with_metrics(text, pattern, &DocumentMetrics::default())
}

/// Evaluate a single Pattern with supplied DocumentMetrics.
/// Graceful degradation: on any parsing / compile / evaluation error the
/// matcher returns false (never over-reports).
pub fn evaluate_pattern_with_metrics(
    text: &str,
    pattern: &Pattern,
    metrics: &DocumentMetrics,
) -> bool {
    match pattern {
        Pattern::Keyword { value, mode, match_mode } => {
            let words: Vec<&str> = value.iter().map(|s| s.as_str()).collect();
            let base_match = if mode == "all" {
                keyword_all(text, &words)
            } else {
                keyword_any(text, &words)
            };
            match match_mode.as_deref() {
                Some("absence") => !base_match,
                _ => base_match,
            }
        }
        Pattern::Regex { value } => compile_regex(value)
            .map(|re| re.is_match(text))
            .unwrap_or(false),
        Pattern::FieldCompare { left, operator, right } => {
            field_compare(left, operator, right, metrics)
        }
    }
}

/// Evaluate all patterns of a rule, aggregate by `check` strategy.
pub fn evaluate_patterns(text: &str, patterns: &[Pattern], check: &str) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let m = DocumentMetrics::extract_from_clause_text(text);
    let results: Vec<bool> = patterns
        .iter()
        .map(|p| evaluate_pattern_with_metrics(text, p, &m))
        .collect();
    if check == "all_match" {
        results.iter().all(|&r| r)
    } else {
        results.iter().any(|&r| r)
    }
}

// ── field_compare ─────────────────────────────────────────────

/// Resolve a field name or arithmetic expression into a concrete f64.
/// Syntax supported: `estimate_price * 0.02` / `1000` / `deposit_ratio`.
fn resolve_operand(expr: &str, metrics: &DocumentMetrics) -> Option<f64> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Binary operator: `a op b` where op ∈ { + - * / }
    for op in ['*', '/', '+', '-'] {
        if let Some(idx) = trimmed.find(op) {
            // Avoid matching negative sign as operator
            if op == '-' && idx == 0 {
                continue;
            }
            let left_s = &trimmed[..idx];
            let right_s = &trimmed[idx + 1..];
            let l = resolve_operand(left_s, metrics)?;
            let r = resolve_operand(right_s, metrics)?;
            return Some(match op {
                '*' => l * r,
                '/' => {
                    if r.abs() < 1e-12 {
                        return None;
                    }
                    l / r
                }
                '+' => l + r,
                '-' => l - r,
                _ => unreachable!(),
            });
        }
    }

    // Literal number: `0.02` / `1000` / `12.5`
    if let Ok(n) = trimmed.parse::<f64>() {
        return Some(n);
    }

    // Named field on DocumentMetrics
    match trimmed {
        "estimate_price" | "控制价" | "估算价" => metrics.estimate_price,
        "deposit_amount" | "保证金金额" => metrics.deposit_amount,
        "deposit_ratio" | "保证金比例" => metrics.deposit_ratio,
        "preparation_days" | "准备期天数" => metrics.preparation_days.map(|d| d as f64),
        _ => None,
    }
}

fn field_compare(left: &str, op: &str, right: &str, metrics: &DocumentMetrics) -> bool {
    let Some(l) = resolve_operand(left, metrics) else {
        return false;
    };
    let Some(r) = resolve_operand(right, metrics) else {
        return false;
    };
    match op {
        ">" => l > r,
        ">=" => l >= r,
        "<" => l < r,
        "<=" => l <= r,
        "==" | "=" => (l - r).abs() < 1e-9,
        "!=" => (l - r).abs() >= 1e-9,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::metrics::DocumentMetrics;

    // ── keyword ──────────────────────────────────────────────────

    #[test]
    fn keyword_any_matches() {
        assert!(keyword_any("投标人须在本市注册", &["本市", "本区"]));
        assert!(!keyword_any("投标人须注册", &["本市", "本区"]));
    }

    #[test]
    fn keyword_all_matches() {
        assert!(keyword_all("投标人须在本市注册", &["本市", "注册"]));
        assert!(!keyword_all("投标人须在本市注册", &["本市", "分公司"]));
    }

    // ── regex ────────────────────────────────────────────────────

    #[test]
    fn regex_positive_hit() {
        let re = Pattern::Regex {
            value: r"(投标人|潜在投标人).*(本市|本省).*注册".into(),
        };
        assert!(evaluate_pattern("投标人须在本市注册成立三年以上", &re));
        assert!(evaluate_pattern("潜在投标人如在本省注册分公司也可参与投标", &re));
    }

    #[test]
    fn regex_negative_miss() {
        let re = Pattern::Regex {
            value: r"(投标人|潜在投标人).*(本市|本省).*注册".into(),
        };
        assert!(!evaluate_pattern("招标人须在本市注册", &re));
        assert!(!evaluate_pattern("投标人须在境外注册分公司", &re));
    }

    #[test]
    fn regex_invalid_compiles_to_false_gracefully() {
        let re = Pattern::Regex {
            value: r"(未闭合组".into(),
        };
        // 编译失败不应 panic，应保守返回 false
        assert!(!evaluate_pattern("任意文本", &re));
    }

    #[test]
    fn regex_escape_uxxxx_to_unicode() {
        // \u6295\u6807\u4eba = "投标人"
        let re = Pattern::Regex {
            value: r"\u6295\u6807\u4eba.*\u672c\u5e02".into(),
        };
        assert!(evaluate_pattern("投标人须在本市注册", &re));
    }

    #[test]
    fn regex_negation_prevents_false_positive() {
        // Rust regex lookbehind on non-ASCII works for fixed 1-char width, but to
        // keep the rule portable we use a combined regex pattern: match "(须|应)
        // 受.*限制" which explicitly requires a prescriptive modal, so "不受限制"
        // ("不" is a negation modal) is excluded naturally.
        let re = Pattern::Regex {
            value: r"[须应]受[^。]{0,30}限制".into(),
        };
        assert!(evaluate_pattern("投标人须受本市资质限制", &re));
        assert!(!evaluate_pattern("投标人资质不受限制", &re));
    }

    // ── field_compare ────────────────────────────────────────────

    #[test]
    fn field_compare_excessive_deposit_ratio() {
        let text = "投标保证金不得超过估算价的5%";
        let metrics = DocumentMetrics::extract_from_clause_text(text);
        let pat = Pattern::FieldCompare {
            left: "deposit_ratio".into(),
            operator: ">".into(),
            right: "0.02".into(),
        };
        assert!(evaluate_pattern_with_metrics(text, &pat, &metrics));
    }

    #[test]
    fn field_compare_normal_deposit_not_excessive() {
        let text = "投标保证金按估算价的1%计取";
        let metrics = DocumentMetrics::extract_from_clause_text(text);
        let pat = Pattern::FieldCompare {
            left: "deposit_ratio".into(),
            operator: ">".into(),
            right: "0.02".into(),
        };
        assert!(!evaluate_pattern_with_metrics(text, &pat, &metrics));
    }

    #[test]
    fn field_compare_parse_failure_returns_false() {
        // 文本中没有可提取的保证金比例
        let text = "项目不设投标保证金";
        let metrics = DocumentMetrics::extract_from_clause_text(text);
        let pat = Pattern::FieldCompare {
            left: "deposit_ratio".into(),
            operator: ">".into(),
            right: "0.02".into(),
        };
        // 解析失败应保守返回 false（不误报）
        assert!(!evaluate_pattern_with_metrics(text, &pat, &metrics));
    }

    #[test]
    fn field_compare_deposit_amount_expression() {
        // right 支持表达式：估算价 * 0.02 作为上限
        let text = "本项目估算价为1000万元，投标保证金为50万元。";
        let metrics = DocumentMetrics::extract_from_clause_text(text);
        let pat = Pattern::FieldCompare {
            left: "deposit_amount".into(),
            operator: ">".into(),
            right: "estimate_price * 0.02".into(),
        };
        // 50万 vs 1000万*0.02 = 20万 → 50>20 → true（超额保证金）
        assert!(evaluate_pattern_with_metrics(text, &pat, &metrics));
    }

    // ── absence 模式（缺失匹配）────────────────────────────────

    #[test]
    fn keyword_absence_detects_missing_cert() {
        // 资质章节里"未提及安全生产许可证" → 应该命中
        let pat = Pattern::Keyword {
            value: vec!["安全生产许可证".into()],
            mode: "any".into(),
            match_mode: Some("absence".into()),
        };
        // 文本里没有"安全生产许可证" → absence 模式应返回 true（缺了）
        assert!(evaluate_pattern(
            "本项目资质要求：营业执照、税务登记证、组织机构代码证。",
            &pat
        ));
        // 文本里有"安全生产许可证" → absence 模式应返回 false（没缺）
        assert!(!evaluate_pattern(
            "本项目资质要求：营业执照、安全生产许可证、税务登记证。",
            &pat
        ));
    }
}
