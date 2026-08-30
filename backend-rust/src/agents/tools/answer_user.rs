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
                "description": "向用户输出最终回答。用自然中文回答，像法律顾问在与采购专家对话。\n\n【何时使用】\n- 证据充分、逻辑完整时 → 直接输出回答\n- 问题超出标书审查范围 → 礼貌说明边界\n- 用户只是闲聊或确认 → 简短回应\n\n【何时不使用】\n- 还需要搜索法规确认 → 先用 web_search\n- 还需要精读某个章节 → 先用 read_section\n- 还需要在文档中搜索关联条款 → 先用 search_document\n\n【block_id 高亮】\n- 引用原文时在 answer 中用方括号标注：'[b_3_7]原文片段'\n- 前端自动将 [b_xxx] 渲染为 PDF 高亮链接\n\n【confidence】\n- 涉及合规判断时填写（0-1），纯信息性回答不需要\n\n【Markdown 格式（硬性要求）】\n- 标题层级用 ## 或 ###，禁止用「一、」「二、」纯中文序号当标题\n- 加粗 **文字** 中 ** 与文字之间不得留空格\n- 列表用 - 或 1.，标记后留一个空格\n- 表格必须包含 | --- | 表头分隔行，否则前端不渲染成表格\n- 禁止 emoji",
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
