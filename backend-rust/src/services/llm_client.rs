//! LLM 客户端抽象 — 多协议支持（DashScope 原生 + OpenAI 兼容）。
//!
//! ## 协议选择
//!
//! 通过环境变量 `AIBID_LLM_PROTOCOL` 切换：
//! - `dashscope`（默认）— DashScope 原生 API，支持 `search_info` 返回搜索源
//! - `openai_compatible` — OpenAI Chat Completions 兼容端点（MaaS / 任意兼容服务）
//!
//! ## 扩展
//!
//! 新增提供商只需实现 `LlmClient` trait，然后在 `create_llm_client()` 中加一个分支。
//! `ReActLoop` / `Coordinator` 全部通过 trait 消费，零感知协议差异。
//!
//! ## Model 选择
//!
//! 当前固定从环境变量获取。未来产品侧支持扫码/列表选择模型后，
//! 工厂函数将改为接受 `&ModelConfig` 参数。

use crate::agents::react_loop::{
    ChatMessage, LlmClient, LlmResponse, TokenUsage, ToolCall, ToolChoice,
};
use anyhow::{Context, Result};
use serde_json::Value;

/// 从 API 响应中提取 Token 使用量（兼容 DashScope 和 OpenAI 格式）。
fn parse_usage(body: &Value) -> Option<TokenUsage> {
    // DashScope 原生格式: output.usage.{input_tokens, output_tokens, total_tokens}
    if let Some(u) = body.get("output").and_then(|o| o.get("usage")) {
        return Some(TokenUsage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        });
    }
    // DashScope message 格式 / OpenAI 格式: usage 在顶层
    // 字段名两者不同，需同时兼容：
    //   DashScope: input_tokens / output_tokens
    //   OpenAI:    prompt_tokens / completion_tokens
    if let Some(u) = body.get("usage") {
        let input = u
            .get("input_tokens")
            .or_else(|| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output = u
            .get("output_tokens")
            .or_else(|| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let total = u
            .get("total_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or((input + output) as u64) as u32;
        return Some(TokenUsage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: total,
        });
    }
    None
}

/// 解析布尔型环境变量：`1`/`true`/`on`（忽略大小写/首尾空白）为真，其余为假。
fn parse_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on"
    )
}

// ── 工厂函数 ──────────────────────────────────────────────────────

/// 根据 `AIBID_LLM_PROTOCOL` 环境变量创建对应的 LLM 客户端。
///
/// 用于批量审核等业务场景，模型由 `DASHSCOPE_MODEL` / `LLM_MODEL` 控制。
/// * `dashscope` — DashScope 原生 API（默认）
/// * `openai_compatible` — OpenAI 兼容端点
pub fn create_llm_client() -> Result<Box<dyn LlmClient>> {
    let protocol = std::env::var("AIBID_LLM_PROTOCOL").unwrap_or_else(|_| "dashscope".to_string());

    match protocol.as_str() {
        "dashscope" => Ok(Box::new(DashScopeNativeClient::from_env()?)),
        "openai_compatible" => Ok(Box::new(OpenAICompatibleClient::from_env()?)),
        other => anyhow::bail!(
            "未知的 AIBID_LLM_PROTOCOL: '{}'。支持: dashscope, openai_compatible",
            other
        ),
    }
}

// ── DashScope 原生客户端 ─────────────────────────────────────────

/// DashScope 原生 API 客户端。
///
/// 使用阿里云 DashScope 原生协议（非 OpenAI 兼容模式）。
/// 端点：`https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation`
///
/// 与 OpenAI 兼容模式的关键差异：
/// - 请求体用 `input.messages` 而非 `messages`
/// - 参数包裹在 `parameters` 下
/// - 响应路径为 `output.choices` 而非 `choices`
/// - 支持 `search_info` 返回搜索源（未来扩展）
pub struct DashScopeNativeClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
    endpoint: String,
}

impl DashScopeNativeClient {
    /// DashScope 原生 Text Generation API 端点。
    const DEFAULT_ENDPOINT: &'static str =
        "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation";

    /// 创建新的 DashScope 原生客户端。
    pub fn new(api_key: &str, model: &str) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest Client 构建失败");
        Self {
            http: client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint: Self::DEFAULT_ENDPOINT.to_string(),
        }
    }

    /// 从环境变量创建（业务模型）。
    ///
    /// 读取顺序：
    /// - `DASHSCOPE_API_KEY` → `OPENAI_API_KEY`（回退）
    /// - `DASHSCOPE_MODEL`（默认 `qwen-plus`）
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("DASHSCOPE_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .context("DashScope 原生 API 需要密钥。请设置 DASHSCOPE_API_KEY 或 OPENAI_API_KEY")?;
        let model = std::env::var("DASHSCOPE_MODEL").unwrap_or_else(|_| "qwen-plus".to_string());
        Ok(Self::new(&api_key, &model))
    }

    /// 将 ChatMessage 转为 DashScope API 的消息格式。
    ///
    /// 与 OpenAI 兼容格式相同（`result_format: "message"` 时消息结构一致）。
    fn message_to_json(msg: &ChatMessage) -> Value {
        match msg {
            ChatMessage::System { content } => {
                serde_json::json!({"role": "system", "content": content})
            }
            ChatMessage::User { content } => {
                serde_json::json!({"role": "user", "content": content})
            }
            ChatMessage::Assistant {
                content,
                tool_calls,
            } => {
                let mut obj = serde_json::json!({"role": "assistant"});
                if let Some(c) = content {
                    obj["content"] = Value::String(c.clone());
                } else {
                    obj["content"] = Value::Null;
                }
                if let Some(tcs) = tool_calls {
                    let oai_tcs: Vec<Value> = tcs
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string()
                                }
                            })
                        })
                        .collect();
                    obj["tool_calls"] = Value::Array(oai_tcs);
                }
                obj
            }
            ChatMessage::Tool {
                tool_call_id,
                content,
            } => {
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": content
                })
            }
        }
    }

    /// 解析 DashScope 响应为 LlmResponse。
    ///
    /// DashScope 原生响应结构（`result_format: "message"`）：
    /// ```json
    /// {
    ///   "output": {
    ///     "choices": [{
    ///       "message": { "content": "...", "tool_calls": [...] },
    ///       "finish_reason": "stop"
    ///     }]
    ///   }
    /// }
    /// ```
    fn parse_response(body: &Value) -> Result<LlmResponse> {
        let choice = body["output"]["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .context("DashScope 返回空 output.choices")?;

        let msg = &choice["message"];
        // 提取 reasoning_content（DeepSeek-R1 / qwq 等推理模型专用字段）
        let reasoning = msg["reasoning_content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let raw_content = msg["content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let tool_calls: Vec<ToolCall> = msg["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|tc| {
                        let func = &tc["function"];
                        let args: Value = func["arguments"]
                            .as_str()
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(Value::Null);
                        ToolCall {
                            id: tc["id"].as_str().unwrap_or("unknown").to_string(),
                            name: func["name"].as_str().unwrap_or("").to_string(),
                            arguments: args,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let has_tool_calls = !tool_calls.is_empty();

        // 推理分配：reasoning_content 优先 -> content+tool_calls -> 纯回答
        let (thought, content) = if let Some(reason) = reasoning {
            (Some(reason), raw_content)
        } else if has_tool_calls && raw_content.is_some() {
            (raw_content, None)
        } else {
            (None, raw_content)
        };

        Ok(LlmResponse {
            content,
            thought,
            tool_calls,
            usage: parse_usage(body),
        })
    }
}

#[async_trait::async_trait]
impl LlmClient for DashScopeNativeClient {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[Value],
        tool_choice: &ToolChoice,
    ) -> Result<LlmResponse> {
        // 转换消息
        let msg_array: Vec<Value> = messages.iter().map(Self::message_to_json).collect();

        // 转换工具定义
        let tool_array: Vec<Value> = tools
            .iter()
            .map(|t| {
                if t.get("type").is_some() {
                    t.clone()
                } else {
                    serde_json::json!({
                        "type": "function",
                        "function": t["function"].clone()
                    })
                }
            })
            .collect();

        let mut parameters = serde_json::json!({
            "result_format": "message",
        });

        if !tool_array.is_empty() {
            parameters["tools"] = Value::Array(tool_array);
            // ★ tool_choice: 仅在非 Auto 且存在工具时传递
            let tc_value = tool_choice.to_dashscope_value();
            if !tc_value.is_null() {
                parameters["tool_choice"] = tc_value;
            }
        }

        let body = serde_json::json!({
            "model": self.model,
            "input": {
                "messages": msg_array,
            },
            "parameters": parameters,
        });

        let response = self
            .http
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("DashScope API 请求失败")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "DashScope API 返回错误 {}: {}",
                status,
                error_text
            ));
        }

        let body: Value = response.json().await.context("解析 DashScope 响应失败")?;
        Self::parse_response(&body)
    }
}

// ── OpenAI 兼容客户端 ────────────────────────────────────────────

/// 基于 reqwest 的 OpenAI 兼容 LLM 客户端实现。
///
/// 直接调用 OpenAI Chat Completions API（或任何兼容端点如阿里云 MaaS）。
/// 不依赖 async-openai crate，零额外类型开销。
///
/// 注意：OpenAI 兼容协议不支持返回 `search_info`（搜索源标题+URL）。
/// 如需搜索源标注，请使用 `DashScopeNativeClient`。
pub struct OpenAICompatibleClient {
    http: reqwest::Client,
    api_base: String,
    api_key: String,
    model: String,
    disable_thinking: bool,
}

impl OpenAICompatibleClient {
    /// 创建新的 OpenAI 兼容客户端。
    pub fn new(api_base: &str, api_key: &str, model: &str) -> Self {
        // 显式禁用系统代理——阿里云 MaaS 端点直连即可，不经本地代理（如 Clash）。
        // 若需代理，可通过 HTTPS_PROXY 环境变量显式配置。
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120)) // 连接+请求总超时 2 分钟
            .build()
            .expect("reqwest Client 构建失败");
        Self {
            http: client,
            api_base: api_base.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            disable_thinking: false,
        }
    }

    /// 是否关闭思考模式（请求体追加 `enable_thinking=false`）。
    ///
    /// qwen3.x 等混合思考模型默认输出大量 `reasoning` token（慢且费）。
    /// 法律条款审查看重工具链+法条引用而非逐步推理，关闭思考可显著提速降本。
    pub fn with_disable_thinking(mut self, disable: bool) -> Self {
        self.disable_thinking = disable;
        self
    }

    /// 使用环境变量创建（业务模型）。
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY 环境变量未设置")?;
        let api_base = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
        let disable_thinking = std::env::var("AIBID_DISABLE_THINKING")
            .map(|v| parse_truthy(&v))
            .unwrap_or(false);
        Ok(Self::new(&api_base, &api_key, &model).with_disable_thinking(disable_thinking))
    }

    fn message_to_json(msg: &ChatMessage) -> Value {
        match msg {
            ChatMessage::System { content } => {
                serde_json::json!({"role": "system", "content": content})
            }
            ChatMessage::User { content } => {
                serde_json::json!({"role": "user", "content": content})
            }
            ChatMessage::Assistant {
                content,
                tool_calls,
            } => {
                let mut obj = serde_json::json!({"role": "assistant"});
                if let Some(c) = content {
                    obj["content"] = Value::String(c.clone());
                } else {
                    obj["content"] = Value::Null;
                }
                if let Some(tcs) = tool_calls {
                    let oai_tcs: Vec<Value> = tcs
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string()
                                }
                            })
                        })
                        .collect();
                    obj["tool_calls"] = Value::Array(oai_tcs);
                }
                obj
            }
            ChatMessage::Tool {
                tool_call_id,
                content,
            } => {
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": content
                })
            }
        }
    }

    /// 解析 OpenAI API 响应为 LlmResponse。
    fn parse_response(body: &Value) -> Result<LlmResponse> {
        let choice = body["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .context("LLM 返回空 choices")?;

        let msg = &choice["message"];

        // 提取 reasoning_content（DeepSeek-R1 等推理模型专用字段）
        let reasoning = msg["reasoning_content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let raw_content = msg["content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let tool_calls: Vec<ToolCall> = msg["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|tc| {
                        let func = &tc["function"];
                        let args: Value = func["arguments"]
                            .as_str()
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(Value::Null);
                        ToolCall {
                            id: tc["id"].as_str().unwrap_or("unknown").to_string(),
                            name: func["name"].as_str().unwrap_or("").to_string(),
                            arguments: args,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let has_tool_calls = !tool_calls.is_empty();

        // 推理分配：reasoning_content 优先 -> content+tool_calls -> 纯回答
        let (thought, content) = if let Some(reason) = reasoning {
            (Some(reason), raw_content)
        } else if has_tool_calls && raw_content.is_some() {
            (raw_content, None)
        } else {
            (None, raw_content)
        };

        Ok(LlmResponse {
            content,
            thought,
            tool_calls,
            usage: parse_usage(body),
        })
    }

    /// 构建 OpenAI 兼容 Chat Completions 请求体（纯函数，便于单测）。
    ///
    /// 当 `disable_thinking` 为真时追加 `"enable_thinking": false`，关闭
    /// qwen3.x 等混合思考模型的推理阶段（省 token、提速度）。
    fn build_chat_body(
        model: &str,
        disable_thinking: bool,
        messages: &[ChatMessage],
        tools: &[Value],
        tool_choice: &ToolChoice,
    ) -> Value {
        let msg_array: Vec<Value> = messages.iter().map(Self::message_to_json).collect();

        let tool_array: Vec<Value> = tools
            .iter()
            .map(|t| {
                if t.get("type").is_some() {
                    t.clone()
                } else {
                    serde_json::json!({
                        "type": "function",
                        "function": t["function"].clone()
                    })
                }
            })
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "messages": msg_array,
        });

        if disable_thinking {
            body["enable_thinking"] = Value::Bool(false);
        }

        if !tool_array.is_empty() {
            body["tools"] = Value::Array(tool_array);
            // ★ tool_choice: 仅在非 Auto 且存在工具时传递
            let tc_value = tool_choice.to_openai_value();
            if tc_value != serde_json::json!("auto") {
                body["tool_choice"] = tc_value;
            }
        }

        body
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAICompatibleClient {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[Value],
        tool_choice: &ToolChoice,
    ) -> Result<LlmResponse> {
        let body = Self::build_chat_body(
            &self.model,
            self.disable_thinking,
            messages,
            tools,
            tool_choice,
        );

        let url = format!("{}/chat/completions", self.api_base);
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("LLM API 请求失败")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "LLM API 返回错误 {}: {}",
                status,
                error_text
            ));
        }

        let body: Value = response.json().await.context("解析 LLM 响应失败")?;
        Self::parse_response(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage::User {
            content: content.to_string(),
        }
    }

    #[test]
    fn disable_thinking_adds_enable_thinking_false() {
        let body = OpenAICompatibleClient::build_chat_body(
            "qwen3.7-plus",
            true,
            &[user_msg("hi")],
            &[],
            &ToolChoice::Auto,
        );
        assert_eq!(body["enable_thinking"], serde_json::Value::Bool(false));
        assert_eq!(body["model"], "qwen3.7-plus");
    }

    #[test]
    fn default_omits_enable_thinking() {
        let body = OpenAICompatibleClient::build_chat_body(
            "qwen3.7-plus",
            false,
            &[user_msg("hi")],
            &[],
            &ToolChoice::Auto,
        );
        assert!(body.get("enable_thinking").is_none());
    }

    #[test]
    fn parse_truthy_recognizes_true_variants() {
        assert!(parse_truthy("1"));
        assert!(parse_truthy("true"));
        assert!(parse_truthy("ON"));
        assert!(parse_truthy(" true "));
        assert!(!parse_truthy("0"));
        assert!(!parse_truthy("false"));
        assert!(!parse_truthy(""));
        assert!(!parse_truthy("yes"));
    }

    #[test]
    fn tools_and_tool_choice_still_serialized() {
        let tool = serde_json::json!({
            "type": "function",
            "function": {"name": "search", "parameters": {"type": "object"}}
        });
        // Auto 模式不显式带 tool_choice；但 tools 必须原样带上
        let body = OpenAICompatibleClient::build_chat_body(
            "m",
            false,
            &[user_msg("q")],
            std::slice::from_ref(&tool),
            &ToolChoice::Auto,
        );
        assert!(body["tools"].as_array().is_some());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn specific_tool_choice_is_serialized() {
        let tool = serde_json::json!({
            "type": "function",
            "function": {"name": "output_finding", "parameters": {"type": "object"}}
        });
        let body = OpenAICompatibleClient::build_chat_body(
            "m",
            true,
            &[user_msg("q")],
            std::slice::from_ref(&tool),
            &ToolChoice::Specific {
                name: "output_finding".to_string(),
            },
        );
        assert!(body["tool_choice"].is_object());
        assert_eq!(body["enable_thinking"], serde_json::Value::Bool(false));
    }
}
