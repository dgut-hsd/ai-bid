//! `output_finding` 终端工具。
//!
//! 工具名为兼容旧 Agent 配置而保留；新协议一次输出零到多条风险发现。

use anyhow::Result;

use super::AgentTool;

pub struct OutputFindingTool;

#[async_trait::async_trait]
impl AgentTool for OutputFindingTool {
    fn name(&self) -> &str {
        "output_finding"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "output_finding",
                "description": "输出当前条款最终结论。逐项列出独立问题；无风险返回 findings=[]；最多5条，超出用 has_more=true。source_quote 只引用支撑该条的原文；reason 含事实→规则→结论；confidence<0.6 不得输出 high。severity：high=必须修改/红线，medium=建议修改，low/info=优化提示。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "findings": {
                            "type": "array",
                            "maxItems": 5,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "no_risk": { "type": "boolean" },
                                    "severity": { "type": "string", "enum": ["high", "medium", "low", "info"] },
                                    "is_critical": { "type": "boolean" },
                                    "critical_reason": { "type": "string" },
                                    "risk_type": { "type": "string" },
                                    "category_code": { "type": "string" },
                                    "source_quote": { "type": "string" },
                                    "legal_basis": { "type": "array", "items": { "type": "string" } },
                                    "reason": { "type": "string" },
                                    "suggestion": { "type": "string" },
                                    "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 }
                                },
                                "required": [
                                    "no_risk", "severity", "is_critical", "critical_reason",
                                    "risk_type", "category_code", "source_quote", "legal_basis",
                                    "reason", "suggestion", "confidence"
                                ]
                            }
                        },
                        "has_more": { "type": "boolean" },
                        "coverage": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["findings", "has_more", "coverage"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> serde_json::Value {
        OutputFindingTool.definition()
    }

    /// 精简 schema 绝不能破坏解析契约：顶层与 findings 子项的 required 字段、
    /// severity 枚举、confidence 边界都必须原样保留。
    #[test]
    fn keeps_parsing_contract() {
        let v = schema();
        let params = &v["function"]["parameters"];
        let props = &params["properties"];

        let top_req: Vec<&str> = params["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(top_req, vec!["findings", "has_more", "coverage"]);

        let items_req: Vec<&str> = props["findings"]["items"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(
            items_req,
            vec![
                "no_risk", "severity", "is_critical", "critical_reason", "risk_type",
                "category_code", "source_quote", "legal_basis", "reason", "suggestion",
                "confidence"
            ]
        );

        let sev: Vec<&str> = props["findings"]["items"]["properties"]["severity"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(sev, vec!["high", "medium", "low", "info"]);

        let conf = &props["findings"]["items"]["properties"]["confidence"];
        assert_eq!(conf["maximum"].as_f64(), Some(1.0));
    }

    /// category_code 的枚举清单应移除（已由条款级 checklist 注入），
    /// 只保留普通 string，避免每轮重复下发 15 个分类码。
    #[test]
    fn category_code_is_plain_string_without_enum() {
        let v = schema();
        let cc = &v["function"]["parameters"]["properties"]["findings"]["items"]
            ["properties"]["category_code"];
        assert_eq!(cc["type"], "string");
        assert!(
            cc.get("enum").is_none(),
            "category_code 不应再带 15 值枚举，应从 checklist 选值"
        );
    }

    /// schema 必须足够短，堵住将来把描述重新写啰嗦的回归。
    #[test]
    fn schema_is_slim() {
        let s = serde_json::to_string(&schema()).unwrap();
        let n = s.chars().count();
        assert!(n < 1500, "schema 应精简到 <1500 字符，当前 {n}");
    }

    /// 精简描述可以，但质量门槛不能丢。
    #[test]
    fn description_preserves_quality_rules() {
        let v = schema();
        let desc = v["function"]["description"].as_str().unwrap();
        assert!(desc.contains("findings"), "必须说明无风险返回空数组");
        assert!(desc.contains("has_more"), "必须保留多问题协议");
        assert!(desc.contains("source_quote"), "必须保留引用原文规范");
        assert!(desc.contains("confidence"), "必须保留 confidence 门槛");
    }

    /// severity 标定锚点（high=必须修改 / medium=建议修改）是全局评级的最后一道尺子，
    /// 各 agent 提示词只有单点触发条件，没有这条总标尺容易把红线降级为 medium。
    #[test]
    fn severity_calibration_anchor_preserved() {
        let v = schema();
        let desc = v["function"]["description"].as_str().unwrap();
        assert!(desc.contains("必须修改"), "必须保留 high=必须修改 锚点");
        assert!(desc.contains("建议修改"), "必须保留 medium=建议修改 锚点");
    }
}
