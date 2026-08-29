//! 招标文件双视图脱敏服务。
//!
//! 原文只保留在本地；发送到远程 Embedding/LLM/工具链的是脱敏副本。
//! 日期、金额、比例默认不替换，因为它们是期限、预算、保证金等审核规则的必要证据。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\w.\-+]+@[\w.\-]+\.[a-zA-Z]{2,}").unwrap());
static ID_CARD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?P<id>\d{17}[\dXx])").unwrap());
static PHONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<phone>1[3-9]\d-?\d{4}-?\d{4})").unwrap());
static LANDLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<landline>0\d{2,3}[-\s]?\d{7,8})").unwrap());
static BANK_CARD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<bank>\b\d{16,19}\b)").unwrap());
static USCC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[0-9A-HJ-NPQRTUWXY]{18}\b").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesensitizationMode {
    Off,
    #[default]
    Low,
}

impl DesensitizationMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "none" => Some(Self::Off),
            "low" | "standard" | "true" | "on" => Some(Self::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesensitizationSummary {
    pub mode: DesensitizationMode,
    pub total_replacements: usize,
    pub counts: BTreeMap<String, usize>,
    /// 明确记录审核关键字段没有被替换，便于验收和审计。
    pub preserved_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RedactionVault {
    mode: DesensitizationMode,
    value_to_placeholder: HashMap<String, String>,
    placeholder_to_value: HashMap<String, String>,
    counts: BTreeMap<String, usize>,
}

impl RedactionVault {
    pub fn new(mode: DesensitizationMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    pub fn mode(&self) -> DesensitizationMode {
        self.mode
    }

    fn placeholder_for(&mut self, label: &str, original: &str) -> String {
        let key = format!("{label}\0{original}");
        if let Some(existing) = self.value_to_placeholder.get(&key) {
            return existing.clone();
        }
        let next = self.counts.get(label).copied().unwrap_or(0) + 1;
        let placeholder = format!("[{label}_{next}]");
        self.counts.insert(label.to_string(), next);
        self.value_to_placeholder.insert(key, placeholder.clone());
        self.placeholder_to_value
            .insert(placeholder.clone(), original.to_string());
        placeholder
    }

    fn redact_matches(&mut self, text: &str, regex: &Regex, label: &str) -> String {
        let matches: Vec<(usize, usize, String)> = regex
            .find_iter(text)
            .map(|found| (found.start(), found.end(), found.as_str().to_string()))
            .collect();
        self.replace_ranges(text, matches, label)
    }

    fn replace_ranges(
        &mut self,
        text: &str,
        matches: Vec<(usize, usize, String)>,
        label: &str,
    ) -> String {
        if matches.is_empty() {
            return text.to_string();
        }
        let mut output = String::with_capacity(text.len());
        let mut cursor = 0;
        for (start, end, original) in matches {
            if start < cursor {
                continue;
            }
            output.push_str(&text[cursor..start]);
            output.push_str(&self.placeholder_for(label, &original));
            cursor = end;
        }
        output.push_str(&text[cursor..]);
        output
    }

    pub fn redact(&mut self, text: &str) -> String {
        if self.mode == DesensitizationMode::Off {
            return text.to_string();
        }
        let mut result = text.to_string();
        // 从特化模式到宽模式，防止身份证/银行卡被普通数字模式抢占。
        result = self.redact_matches(&result, &EMAIL_RE, "邮箱");
        result = self.redact_matches(&result, &ID_CARD_RE, "证件号");
        result = self.redact_matches(&result, &USCC_RE, "统一社会信用代码");
        result = self.redact_matches(&result, &BANK_CARD_RE, "银行账号");
        result = self.redact_matches(&result, &PHONE_RE, "联系电话");
        result = self.redact_matches(&result, &LANDLINE_RE, "联系电话");

        result
    }

    pub fn restore(&self, text: &str) -> String {
        let mut restored = text.to_string();
        // 长占位符优先，避免 `_1]` 成为 `_10]` 的子串时误替换。
        let mut replacements: Vec<_> = self.placeholder_to_value.iter().collect();
        replacements.sort_by_key(|(placeholder, _)| std::cmp::Reverse(placeholder.len()));
        for (placeholder, original) in replacements {
            restored = restored.replace(placeholder, original);
        }
        restored
    }

    pub fn summary(&self) -> DesensitizationSummary {
        DesensitizationSummary {
            mode: self.mode,
            total_replacements: self.counts.values().sum(),
            counts: self.counts.clone(),
            preserved_fields: vec![
                "日期".to_string(),
                "金额".to_string(),
                "比例".to_string(),
                "期限".to_string(),
            ],
        }
    }
}

/// 无状态兼容接口，供单条远程嵌入文本使用。
pub fn desensitize(text: &str) -> String {
    RedactionVault::new(DesensitizationMode::Low).redact(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_redacts_pii_but_preserves_review_facts() {
        let mut vault = RedactionVault::new(DesensitizationMode::Low);
        let input = "联系人电话13812345678，邮箱a@example.com，保证金5万元，截止2026年9月20日。";
        let output = vault.redact(input);
        assert!(!output.contains("13812345678"));
        assert!(!output.contains("a@example.com"));
        assert!(output.contains("5万元"));
        assert!(output.contains("2026年9月20日"));
        assert_eq!(vault.restore(&output), input);
    }

    #[test]
    fn placeholders_are_stable_and_numbered() {
        let mut vault = RedactionVault::new(DesensitizationMode::Low);
        let first = vault.redact("电话13812345678");
        let second = vault.redact("再次联系13812345678，或13912345678");
        assert!(first.contains("[联系电话_1]"));
        assert!(second.contains("[联系电话_1]"));
        assert!(second.contains("[联系电话_2]"));
        assert_eq!(vault.summary().total_replacements, 2);
    }

    #[test]
    fn off_mode_is_noop() {
        let mut vault = RedactionVault::new(DesensitizationMode::Off);
        let input = "电话13812345678";
        assert_eq!(vault.redact(input), input);
        assert_eq!(vault.summary().total_replacements, 0);
    }

    /// 部署 reload（c）依赖 vault 可跨进程持久化：序列化→反序列化后，
    /// 解掩能力必须保留，否则重启后 findings 的正文无法还原原文。
    #[test]
    fn vault_serialization_round_trips() {
        let mut vault = RedactionVault::new(DesensitizationMode::Low);
        let input = "联系人电话13812345678，邮箱a@example.com，保证金5万元，截止2026年9月20日。";
        let masked = vault.redact(input);

        let json = serde_json::to_string(&vault).expect("vault 应可序列化");
        let reloaded: RedactionVault = serde_json::from_str(&json).expect("vault 应可反序列化");

        assert_eq!(reloaded.restore(&masked), input, "重启后解掩应还原原文");
        assert_eq!(reloaded.summary().total_replacements, vault.summary().total_replacements);
    }
}
