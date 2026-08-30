//! `answer_user` 工具 — ChatAgent 的终止工具。
//!
//! 功能等价于 `output_finding`，但输出自然语言而非结构化 Risk 对象。
//!
//! 与 output_finding 的关键区别：
//! - 不需要 agent_id（只有 ChatAgent 使用）
//! - 不写 SessionGraph（由 Harness 在对话结束后总结写入）
//! - 输出自然语言 + 可选引用，而非结构化 risk 对象

use crate::agents::tools::AgentTool;
use anyhow::Result;

/// ChatAgent 的答案输出工具。
///
/// 纯透传：不做任何处理，直接将 LLM 输出返回给调用方。
/// 终止逻辑由 ChatAgent::chat() 侧的 `has_answer_user()` 检测触发。
pub struct AnswerUserTool;

#[async_trait::async_trait]
impl AgentTool for AnswerUserTool {
    fn name(&self) -> &str {
        "answer_user"
    }

    fn definition(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "answer_user",
                "description": "向用户输出自然语言回答(Markdown)。证据充分时直接回答；引用原文用[b_xxx]标注高亮。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "answer": {
                            "type": "string",
                            "description": "Markdown 格式的自然语言回答，严格遵守 Markdown 语法"
                        },
                        "confidence": {
                            "type": "number",
                            "minimum": 0,
                            "maximum": 1,
                            "description": "置信度（仅当做出合规判断时填写）"
                        },
                        "references": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "block_id": { "type": "string" },
                                    "quote": { "type": "string", "description": "精确引用的文字片段" }
                                }
                            },
                            "description": "原文引用列表"
                        },
                        "knowledge_refs": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "ref_type": { "type": "string", "enum": ["law", "case", "negative_list"] },
                                    "title": { "type": "string" },
                                    "excerpt": { "type": "string" }
                                }
                            },
                            "description": "法规/案例引用列表"
                        },
                        "suggested_actions": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "建议用户下一步做什么（如'查看 b_10_5 评分标准'）"
                        }
                    },
                    "required": ["answer"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        // 纯透传：不做任何处理，直接将 LLM 输出返回给调用方
        // 终止逻辑由 ChatAgent::chat() 侧的 has_answer_user() 检测触发
        Ok(args)
    }
}
