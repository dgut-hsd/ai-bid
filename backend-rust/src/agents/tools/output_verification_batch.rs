//! `output_verification_batch` 工具 — 批量法条验证的终端工具。
//!
//! LegalVerifyAgent 在批量模式下使用此工具一次性输出多条验证结论，
//! 触发 ReAct 循环退出（与 `output_finding` 类似）。
//!
//! ## 与 `output_finding` 的区别
//!
//! - `output_finding` — 单条审查结论（主审查流程使用）
//! - `output_verification_batch` — 批量验证结论（LegalVerify 批量模式专用）

use anyhow::Result;
use serde_json::Value;

use super::AgentTool;

pub struct OutputVerificationBatchTool;

#[async_trait::async_trait]
impl AgentTool for OutputVerificationBatchTool {
    fn name(&self) -> &str {
        "output_verification_batch"
    }

    fn definition(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "output_verification_batch",
                "description": "输出所有法条验证的批量结论。每次调用必须包含所有待验证 finding 的完整结果。调用此工具后审查结束。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "verifications": {
                            "type": "array",
                            "description": "所有待验证 finding 的验证结果列表",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "risk_id": {
                                        "type": "string",
                                        "description": "待验证的 risk_id（如 R_001）"
                                    },
                                    "is_valid": {
                                        "type": "boolean",
                                        "description": "法条引用是否真实、准确、适用。true=验证通过，false=需修正"
                                    },
                                    "corrected_legal_basis": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "修正后的法条引用列表。is_valid=true 时可复用原始引用；is_valid=false 时必须提供正确的引用（含 URL 链接，Markdown 格式）"
                                    },
                                    "confidence": {
                                        "type": "number",
                                        "minimum": 0.0,
                                        "maximum": 1.0,
                                        "description": "验证置信度。基于搜索结果的法条匹配度、时效性、适用范围"
                                    },
                                    "reason": {
                                        "type": "string",
                                        "description": "验证推理：搜了什么 → 找到了什么 → 为什么通过/修正/降级"
                                    }
                                },
                                "required": ["risk_id", "is_valid", "corrected_legal_basis", "confidence", "reason"]
                            }
                        }
                    },
                    "required": ["verifications"]
                }
            }
        })
    }

    async fn execute(&self, _args: Value) -> Result<Value> {
        // 批量验证的 arguments 由 ReAct 循环直接解析，
        // 不需要在此处做额外处理。
        Ok(serde_json::json!({"status": "batch_verification_received"}))
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        let tool = OutputVerificationBatchTool;
        assert_eq!(tool.name(), "output_verification_batch");
    }

    #[test]
    fn test_definition_has_required_fields() {
        let tool = OutputVerificationBatchTool;
        let def = tool.definition();
        let func = def.get("function").expect("缺少 function");
        assert_eq!(func["name"], "output_verification_batch");
        assert!(!func["description"].as_str().unwrap().is_empty());
        let params = &func["parameters"];
        assert_eq!(params["type"], "object");
        let required = params["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "verifications"));
    }

    #[test]
    fn test_execute_always_returns_received() {
        let tool = OutputVerificationBatchTool;
        // 即使传空 JSON，execute 也应正常返回（不做实际解析）
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(serde_json::json!({})));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["status"], "batch_verification_received");
    }

    #[test]
    fn test_execute_with_valid_batch() {
        let tool = OutputVerificationBatchTool;
        let args = serde_json::json!({
            "verifications": [
                {
                    "risk_id": "R_001",
                    "is_valid": true,
                    "corrected_legal_basis": ["《政府采购法》第22条"],
                    "confidence": 0.95,
                    "reason": "法条引用正确，条款号匹配，时效性有效"
                },
                {
                    "risk_id": "R_002",
                    "is_valid": false,
                    "corrected_legal_basis": ["《招标投标法》第41条"],
                    "confidence": 0.60,
                    "reason": "原引用的87号令第55条不适用此场景，应引用招标投标法第41条"
                }
            ]
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(args));
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["status"], "batch_verification_received");
    }

    #[test]
    fn test_execute_with_empty_verifications() {
        let tool = OutputVerificationBatchTool;
        let args = serde_json::json!({"verifications": []});
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(args));
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["status"], "batch_verification_received");
    }

    #[test]
    fn test_definition_verification_item_has_required_fields() {
        let tool = OutputVerificationBatchTool;
        let def = tool.definition();
        let items = &def["function"]["parameters"]["properties"]["verifications"]["items"];
        let required = items["required"].as_array().unwrap();
        let required_fields: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_fields.contains(&"risk_id"));
        assert!(required_fields.contains(&"is_valid"));
        assert!(required_fields.contains(&"corrected_legal_basis"));
        assert!(required_fields.contains(&"confidence"));
        assert!(required_fields.contains(&"reason"));
    }

    #[test]
    fn test_definition_confidence_bounds() {
        let tool = OutputVerificationBatchTool;
        let def = tool.definition();
        let conf = &def["function"]["parameters"]["properties"]["verifications"]["items"]["properties"]["confidence"];
        assert_eq!(conf["minimum"], 0.0);
        assert_eq!(conf["maximum"], 1.0);
    }

    /// AgentTool trait 的基本语义测试
    #[test]
    fn test_tool_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<OutputVerificationBatchTool>();
    }
}
