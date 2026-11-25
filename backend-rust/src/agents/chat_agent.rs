//! ChatAgent — 交互式对话审查 Agent。
//!
//! 与批量审查 Agent 互补：批量审查=全覆盖一次性（像跑测试套件），
//! 对话审查=按需深挖随时（像 Debug 时问 AI）。
//!
//! ★ 架构决策：ChatAgent 不继承 ReActLoop。
//! ReActLoop 的输入 (ReviewClause → RiskFinding) 与 ChatAgent 的输入
//! (用户自然语言 → ChatResponse) 类型根本不同。两者共享底层抽象
//! （LlmClient trait / ToolRegistry / ChatMessage / execute_tool_calls helper），
//! 但不共享 ReAct 循环骨架。

use crate::agents::prompts::CHAT_AGENT_SYSTEM_PROMPT;
use crate::agents::react_loop::{
    ChatMessage, LlmClient, LlmResponse, ToolChoice, execute_tool_calls,
};
use crate::agents::tools::ToolRegistry;
use crate::agents::types::*;
use crate::domain::chunk::Chunk;
use crate::domain::vector_index::DocumentVectorIndex;
use crate::services::embedding_service::EmbeddingClient;
use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// ChatAgent — 交互式对话审查 Agent。
pub struct ChatAgent {
    pub config: ChatAgentConfig,
    pub project_config_content: String,
    pub user_preferences_content: String,
    /// LLM 客户端（Arc 共享，与批量审查 Agent 复用同一实例）
    pub llm: Arc<dyn LlmClient>,
    /// 工具注册表（含 web_search / search_document / read_section / answer_user）
    pub tools: ToolRegistry,
    /// 文档向量索引（可选，用于自动 RAG 注入）
    pub document_index: Option<Arc<DocumentVectorIndex>>,
    /// 嵌入客户端（可选，用于 RAG 查询编码）
    pub embed_client: Option<Arc<EmbeddingClient>>,
    /// Chunk 查找表（可选，用于 answer_user 验证）
    pub chunks: Option<Arc<HashMap<String, Chunk>>>,
}

impl ChatAgent {
    /// 创建 ChatAgent（加载 .ai-bid/ 配置文件）。
    ///
    /// * `document_index` — 可选，用于自动 RAG 注入（服务端搜索标书原文）
    /// * `embed_client` — 可选，配合 document_index 做查询编码
    /// * `chunks` — 可选，用于 answer_user 引用验证
    pub fn new(
        config: ChatAgentConfig,
        llm: Arc<dyn LlmClient>,
        tools: ToolRegistry,
        document_index: Option<Arc<DocumentVectorIndex>>,
        embed_client: Option<Arc<EmbeddingClient>>,
        chunks: Option<Arc<HashMap<String, Chunk>>>,
    ) -> Result<Self> {
        let project_config_content =
            std::fs::read_to_string(&config.project_config_path).unwrap_or_default();
        let user_preferences_content =
            std::fs::read_to_string(&config.preferences_path).unwrap_or_default();

        Ok(Self {
            config,
            project_config_content,
            user_preferences_content,
            llm,
            tools,
            document_index,
            embed_client,
            chunks,
        })
    }

    /// 构建完整的 system_prompt（含 config + preferences + selection 注入）。
    fn build_system_prompt(&self, selection: &Option<TextSelection>) -> String {
        let mut prompt = String::from(CHAT_AGENT_SYSTEM_PROMPT);

        if !self.project_config_content.is_empty() {
            prompt.push_str("\n\n── 项目审查配置 (review-config.md) ──\n");
            prompt.push_str(&self.project_config_content);
        }

        if !self.user_preferences_content.is_empty() {
            prompt.push_str("\n\n── 用户偏好 (user-preferences.md) ──\n");
            prompt.push_str(&self.user_preferences_content);
        }

        if let Some(sel) = selection {
            prompt.push_str(&format!(
                "\n\n用户当前在 PDF 第 {} 页选中了以下文字:\n> {}\n(block_ids: {:?})\n你可以在回答中引用 block_id 让前端渲染高亮。",
                sel.page + 1,
                sel.text,
                sel.block_ids
            ));
        }

        prompt
    }

    /// 构建用户消息（含 selection 注入）。
    fn build_user_message(&self, selection: &Option<TextSelection>, user_input: &str) -> String {
        if let Some(sel) = selection {
            format!(
                "用户在 PDF 第 {} 页选中了以下文字:\n> {}\n(block_ids: {:?})\n\n用户提问: {}",
                sel.page + 1,
                sel.text,
                sel.block_ids,
                if user_input.is_empty() {
                    "请分析这段文字的合规性"
                } else {
                    user_input
                }
            )
        } else {
            user_input.to_string()
        }
    }

    /// 服务端自动搜索标书原文，用于 RAG 注入。
    ///
    /// 编码用户问题 → 向量搜索 → 多样性筛选 → 返回 Top-K 结果的格式化文本。
    /// 如果未配置 document_index/embed_client，或查询为空，返回空字符串。
    ///
    /// ## 多样性策略（V6.0）
    ///
    /// 先取 Top-15，再按 section_path 前缀分组，每组只保留最高分的 1 条。
    /// 确保注入的上下文覆盖文档的不同章节区域，而非全部挤在同一区域。
    /// 最终注入最多 8 条（多样性筛选后取 Top-8）。
    async fn build_rag_context(
        &self,
        user_input: &str,
        selection: &Option<TextSelection>,
    ) -> String {
        let (index, embed) = match (&self.document_index, &self.embed_client) {
            (Some(idx), Some(emb)) => (idx, emb),
            _ => return String::new(),
        };

        // 构建搜索查询：合并划词文本 + 用户问题
        let query: String = match selection {
            Some(sel) if !sel.text.is_empty() && !user_input.is_empty() => {
                format!("{} {}", sel.text, user_input)
            }
            Some(sel) if !sel.text.is_empty() => sel.text.clone(),
            _ => user_input.to_string(),
        };

        // 跳过太短的查询（如"你好"、"谢谢"）
        let trimmed = query.trim();
        if trimmed.is_empty() || trimmed.chars().count() < 3 {
            return String::new();
        }

        // 编码查询 → 向量搜索
        let query_embs = match embed.encode_queries(&[trimmed]) {
            Ok(embs) => embs,
            Err(_) => return String::new(),
        };
        let query_emb = match query_embs.into_iter().next() {
            Some(emb) => emb,
            None => return String::new(),
        };

        // ★ V6.0: 先取 Top-15，再多样性筛选
        let raw_hits = index.search(&query_emb, 15);
        if raw_hits.is_empty() {
            return String::new();
        }

        let hits = diversify_hits(&raw_hits, 8);

        // 构建注入文本
        let mut ctx = String::from("\n\n── 标书相关原文（系统自动检索，请基于此回答）──\n");
        ctx.push_str("以下是从标书中自动检索到的最相关内容，已按章节分散以确保覆盖面。\n");
        ctx.push_str("⚠️ 这只是标书的一部分——如果涉及的信息不在下面，你必须用 search_document 搜索或用 read_section 精读。\n\n");

        for (i, hit) in hits.iter().enumerate() {
            let score_label = if hit.score >= 0.7 {
                "★★★"
            } else if hit.score >= 0.5 {
                "★★"
            } else {
                "★"
            };
            // 显示 section_path 的前 2 级，帮助 LLM 判断来源区域
            let area = if hit.title.is_empty() {
                String::new()
            } else {
                format!(" [{}]", hit.title)
            };
            ctx.push_str(&format!(
                "**{}** {} (相似度: {:.2}, 第{}页{})\n  chunk_id: {}\n  {}\n\n",
                i + 1,
                score_label,
                hit.score,
                hit.page_start + 1,
                area,
                hit.chunk_id,
                hit.snippet,
            ));
        }

        // 展示被过滤掉的区域提示
        let total_raw = raw_hits.len();
        let shown = hits.len();
        if total_raw > shown {
            ctx.push_str(&format!(
                "（共检索到 {} 条相关原文，以上展示了按章节分散后的 {} 条。",
                total_raw, shown
            ));
            ctx.push_str("如需更多结果，用 search_document 搜索。）\n");
        }

        ctx.push_str("\n⚠️ 重要：以上只是自动检索的初始结果，未必覆盖了用户问题的全部答案。\n");
        ctx.push_str(
            "如果信息不完整——比如只有笼统条款而没有具体程序/标准——你必须调用 search_document 或 read_section 主动深挖。\n",
        );
        ctx.push_str("禁止凭训练数据记忆编造标书内容。找不到信息就在 answer_user 中坦诚告知。\n");

        ctx
    }

    /// 单次对话入口。
    ///
    /// * `selection` — PDF 划词内容（可为 None）
    /// * `user_input` — 用户问题（可为空字符串）
    /// * `history` — 同一对话的历史消息（支持追问）
    pub async fn chat(
        &self,
        selection: Option<TextSelection>,
        user_input: &str,
        history: Option<Vec<ChatMessage>>,
    ) -> Result<ChatResponse> {
        // ── P0: 自动 RAG 注入：服务端先搜索标书原文 ──
        let rag_context = self.build_rag_context(user_input, &selection).await;

        let mut system_prompt = self.build_system_prompt(&selection);
        if !rag_context.is_empty() {
            system_prompt.push_str(&rag_context);
        }

        // 构建对话（使用 ChatMessage 枚举，与 ReActLoop 一致）
        let mut conversation: Vec<ChatMessage> = vec![ChatMessage::System {
            content: system_prompt,
        }];

        if let Some(h) = history {
            conversation.extend(h);
        }

        conversation.push(ChatMessage::User {
            content: self.build_user_message(&selection, user_input),
        });

        // ── 简化 ReAct 循环 ──
        let tool_defs = self.tools.definitions();
        let mut last_response: Option<LlmResponse> = None;

        for turn in 1..=self.config.max_turns {
            // ★ P1: 首轮禁止纯文本输出，强制至少调用一次工具
            // 最后一轮强制 answer_user
            let tool_choice = if turn == 1 {
                ToolChoice::Required
            } else if turn == self.config.max_turns {
                ToolChoice::Specific {
                    name: "answer_user".to_string(),
                }
            } else {
                ToolChoice::Auto
            };

            let result = match self.llm.chat(&conversation, &tool_defs, &tool_choice).await {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ChatResponse {
                        reasoning: Vec::new(),
                        answer: format!("抱歉，LLM 调用失败: {}", e),
                        references: Vec::new(),
                        knowledge_refs: Vec::new(),
                        confidence: None,
                        suggested_actions: vec!["检查 API 配置后重试".to_string()],
                    });
                }
            };

            // 终止条件 1: LLM 调用 answer_user → 循环结束
            if result.has_answer_user() {
                last_response = Some(result);
                break;
            }

            // ★ 调用共享的 tool execution helper（与 ReActLoop 共用）
            execute_tool_calls(&result, &self.tools, &mut conversation).await?;

            // 终止条件 2: LLM 直接输出文本（无 tool call）→ 视为回答
            if result.content.is_some() && result.tool_calls.is_empty() {
                last_response = Some(result);
                break;
            }
        }

        // 解析 ChatResponse
        match last_response {
            Some(resp) => {
                if let Some(args) = resp.get_answer() {
                    Ok(self.parse_answer_user(args))
                } else {
                    Ok(ChatResponse {
                        reasoning: Vec::new(),
                        answer: Self::filter_emojis(&resp.content.unwrap_or_default()),
                        references: Vec::new(),
                        knowledge_refs: Vec::new(),
                        confidence: None,
                        suggested_actions: Vec::new(),
                    })
                }
            }
            None => {
                // max_turns 耗尽 → 返回截断信息
                Ok(ChatResponse {
                    reasoning: Vec::new(),
                    answer: "抱歉，我在审查该问题时达到了分析上限。请尝试用更具体的问题重新提问。"
                        .to_string(),
                    references: Vec::new(),
                    knowledge_refs: Vec::new(),
                    confidence: None,
                    suggested_actions: vec!["尝试将问题拆分为更小的子问题".to_string()],
                })
            }
        }
    }

    /// 流式对话入口。
    ///
    /// 与 [`chat`] 逻辑相同，但通过 mpsc channel 实时发送
    /// [`ChatStreamEvent`] 事件，供 SSE handler 使用。
    ///
    /// * `tx` — 事件发送端（调用方持有 rx 端读取 SSE）
    ///   根据工具名称和参数生成人类可读的推理步骤描述。
    ///
    /// 当 LLM（如 qwen-plus）不输出 `content` 只输出 `tool_calls` 时，
    /// 从工具调用的实际参数中提取关键信息作为推理步骤展示。
    fn tool_reasoning_hint(name: &str, args: &serde_json::Value) -> String {
        match name {
            "web_search" => {
                let q = args["query"].as_str().unwrap_or("");
                let cat = args["category"].as_str().unwrap_or("");
                if !q.is_empty() {
                    let mut hint = format!("搜索法规: {}", Self::truncate_for_hint(q, 80));
                    if !cat.is_empty() {
                        hint.push_str(&format!("（分类: {}）", cat));
                    }
                    hint
                } else {
                    "搜索相关法规依据".to_string()
                }
            }
            "search_document" => {
                let q = args["query"].as_str().unwrap_or("");
                if !q.is_empty() {
                    format!("搜索标书原文: {}", Self::truncate_for_hint(q, 80))
                } else {
                    "搜索标书原文".to_string()
                }
            }
            "read_section" => {
                let block_id = args["block_id"].as_str().unwrap_or("");
                if !block_id.is_empty() {
                    format!("精读条款 {}", block_id)
                } else {
                    "精读条款原文".to_string()
                }
            }
            "search_knowledge" => {
                let q = args["query"].as_str().unwrap_or("");
                if !q.is_empty() {
                    format!("检索知识库: {}", Self::truncate_for_hint(q, 80))
                } else {
                    "检索知识库".to_string()
                }
            }
            "answer_user" => {
                // answer_user 是终端工具，其参数在 parse_answer_user 中解析
                "整理分析结果，生成最终回答".to_string()
            }
            _ => format!("执行: {}", name),
        }
    }

    /// 截断文本用于推理提示，超长加 "..."
    fn truncate_for_hint(text: &str, max_len: usize) -> String {
        let cleaned = text.replace('\n', " ").replace('\r', "");
        if cleaned.chars().count() > max_len {
            let truncated: String = cleaned.chars().take(max_len).collect();
            format!("{}...", truncated)
        } else {
            cleaned
        }
    }

    pub async fn chat_stream(
        &self,
        selection: Option<TextSelection>,
        user_input: &str,
        history: Option<Vec<ChatMessage>>,
        tx: UnboundedSender<ChatStreamEvent>,
    ) -> Result<()> {
        // 心跳：立即推送首个事件，避免前端在首轮 RAG/LLM 返回前长时间静默（实测约 2 分钟）
        // 导致浏览器 SSE 超时断连（Broken pipe）。
        let _ = tx.send(ChatStreamEvent::Thinking {
            message: "正在检索标书内容并分析您的提问…".to_string(),
        });

        // P0: RAG
        let rag_context = self.build_rag_context(user_input, &selection).await;

        let mut system_prompt = self.build_system_prompt(&selection);
        if !rag_context.is_empty() {
            system_prompt.push_str(&rag_context);
        }

        let mut conversation: Vec<ChatMessage> = vec![ChatMessage::System {
            content: system_prompt,
        }];

        if let Some(h) = history {
            conversation.extend(h);
        }

        conversation.push(ChatMessage::User {
            content: self.build_user_message(&selection, user_input),
        });

        // ReAct loop
        let tool_defs = self.tools.definitions();
        let mut last_response: Option<LlmResponse> = None;
        let mut reasoning_steps: Vec<String> = Vec::new();

        for turn in 1..=self.config.max_turns {
            let tool_choice = if turn == 1 {
                ToolChoice::Required
            } else if turn == self.config.max_turns {
                ToolChoice::Specific {
                    name: "answer_user".to_string(),
                }
            } else {
                ToolChoice::Auto
            };

            let result = match self.llm.chat(&conversation, &tool_defs, &tool_choice).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(ChatStreamEvent::Error(format!("LLM 调用失败: {}", e)));
                    return Ok(());
                }
            };

            // Stream LLM real reasoning (thought), or fallback to tool context
            if let Some(ref thought) = result.thought {
                let filtered = Self::filter_emojis(thought);
                if !filtered.trim().is_empty() {
                    reasoning_steps.push(filtered.clone());
                    let _ = tx.send(ChatStreamEvent::Thinking { message: filtered });
                }
            }

            // Emit tool calls with context-derived reasoning when thought is absent
            for tc in &result.tool_calls {
                // Generate meaningful step from actual tool args (not hardcoded label)
                let reasoning_hint = Self::tool_reasoning_hint(&tc.name, &tc.arguments);
                if reasoning_steps.is_empty()
                    || reasoning_steps.last().map(|s| s.as_str()) != Some(reasoning_hint.as_str())
                {
                    reasoning_steps.push(reasoning_hint.clone());
                    let _ = tx.send(ChatStreamEvent::Thinking {
                        message: reasoning_hint,
                    });
                }
                let _ = tx.send(ChatStreamEvent::ToolCall {
                    name: tc.name.clone(),
                    args: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                });
            }

            // Terminal: answer_user
            if result.has_answer_user() {
                last_response = Some(result);
                break;
            }

            // Execute tools
            if let Err(e) = execute_tool_calls(&result, &self.tools, &mut conversation).await {
                let _ = tx.send(ChatStreamEvent::Error(format!("工具执行失败: {}", e)));
                return Ok(());
            }

            // Terminal: direct text (no tool calls)
            if result.content.is_some() && result.tool_calls.is_empty() {
                last_response = Some(result);
                break;
            }
        }

        // Parse final
        let mut chat_response = match last_response {
            Some(resp) => {
                if let Some(args) = resp.get_answer() {
                    self.parse_answer_user(args)
                } else {
                    ChatResponse {
                        answer: Self::filter_emojis(&resp.content.unwrap_or_default()),
                        reasoning: Vec::new(),
                        references: Vec::new(),
                        knowledge_refs: Vec::new(),
                        confidence: None,
                        suggested_actions: Vec::new(),
                    }
                }
            }
            None => ChatResponse {
                answer: "抱歉，我在审查该问题时达到了分析上限。请尝试用更具体的问题重新提问。"
                    .to_string(),
                reasoning: Vec::new(),
                references: Vec::new(),
                knowledge_refs: Vec::new(),
                confidence: None,
                suggested_actions: vec!["尝试将问题拆分为更小的子问题".to_string()],
            },
        };

        chat_response.reasoning = reasoning_steps;

        let _ = tx.send(ChatStreamEvent::Answer(chat_response.clone()));
        let _ = tx.send(ChatStreamEvent::Done(chat_response));
        Ok(())
    }

    /// 过滤 Emoji 字符，仅保留纯文本内容。
    ///
    /// 匹配规则：
    /// - `\p{Emoji_Presentation}` — 默认 emoji 呈现的字符（😀🎉🔴等）
    /// - `\u{FE0F}` — 变体选择器 16（emoji 风格修饰符）
    /// - `\u{20E3}` — 按键封装组合符（keycap 序列）
    /// - `\u{2600}-\u{27BF}` — 杂项符号 + 丁贝符（⚠✅❌✨等）
    fn filter_emojis(text: &str) -> String {
        let re = Regex::new(r"[\p{Emoji_Presentation}\u{FE0F}\u{20E3}\u{2600}-\u{27BF}]").unwrap();
        re.replace_all(text, "").to_string()
    }

    /// 解析 answer_user 参数 → ChatResponse。
    fn parse_answer_user(&self, args: &serde_json::Value) -> ChatResponse {
        let answer = Self::filter_emojis(args["answer"].as_str().unwrap_or(""));

        // ── P3: 验证引用 block_id 存在性 + quote 文本匹配 ──
        let mut validation_warnings: Vec<String> = Vec::new();
        if let Some(chunks) = &self.chunks
            && let Some(refs) = args["references"].as_array()
        {
            for v in refs {
                if let Some(bid) = v["block_id"].as_str() {
                    match chunks.get(bid) {
                        None => {
                            validation_warnings.push(format!(
                                "⚠️ 引用的 block_id '{}' 在文档中不存在，可能是 LLM 编造的",
                                bid
                            ));
                        }
                        Some(chunk) => {
                            if let Some(quote) = v["quote"].as_str()
                                && !quote.is_empty()
                                && !chunk.text.contains(quote)
                            {
                                // 尝试模糊匹配（忽略空白差异）
                                let normalized_quote: String =
                                    quote.split_whitespace().collect::<Vec<_>>().join("");
                                let normalized_text: String =
                                    chunk.text.split_whitespace().collect::<Vec<_>>().join("");
                                if !normalized_text.contains(&normalized_quote) {
                                    validation_warnings.push(format!(
                                        "⚠️ 对 [{}] 的引用文字在原文中未找到精确匹配，请人工核实",
                                        bid
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        // 合并验证警告到 answer
        let validated_answer = if validation_warnings.is_empty() {
            answer
        } else {
            format!(
                "{}\n\n── 系统验证警告 ──\n{}",
                answer,
                validation_warnings.join("\n")
            )
        };

        // 合并两个来源的 BlockRef:
        // ① answer 文本中用 [b_xxx] 标注的引用（正则提取）
        // ② answer_user 结构化参数中的 references 数组
        let re = Regex::new(r"\[(b_\d+_\d+)\]").unwrap();
        let mut references: Vec<BlockRef> = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        // 来源①: 正则提取 [b_xxx]
        for cap in re.captures_iter(&validated_answer) {
            let bid = cap[1].to_string();
            if seen_ids.insert(bid.clone()) {
                references.push(BlockRef {
                    block_id: bid,
                    quote: String::new(),
                    snippet: String::new(),
                    page: 0,
                });
            }
        }

        // 来源②: 结构化 references 参数
        if let Some(arr) = args["references"].as_array() {
            for v in arr {
                if let Some(bid) = v["block_id"].as_str()
                    && seen_ids.insert(bid.to_string())
                {
                    references.push(BlockRef {
                        block_id: bid.to_string(),
                        quote: v["quote"].as_str().unwrap_or("").to_string(),
                        snippet: String::new(),
                        page: 0,
                    });
                }
            }
        }

        let knowledge_refs: Vec<KnowledgeRef> = args["knowledge_refs"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        let suggested_actions: Vec<String> = args["suggested_actions"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        ChatResponse {
            answer: validated_answer,
            reasoning: Vec::new(),
            references,
            knowledge_refs,
            confidence: args["confidence"].as_f64().map(|c| c as f32),
            suggested_actions,
        }
    }

    /// stdio 交互循环。
    pub async fn chat_loop(&self) -> Result<()> {
        println!();
        println!("═══ ChatAgent 交互模式 ═══");
        println!("输入 /help 查看命令, /quit 退出");
        println!();

        let mut history: Vec<ChatMessage> = Vec::new();

        loop {
            print!("You: ");
            io::stdout().flush()?;

            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            let line = line.trim().to_string();

            if line.is_empty() {
                continue;
            }
            if line == "/quit" || line == "/exit" {
                break;
            }
            if line == "/help" {
                println!("  /quit  — 退出对话");
                println!("  /clear — 清除对话历史");
                println!("  直接输入问题开始对话，支持多轮追问");
                continue;
            }
            if line == "/clear" {
                history.clear();
                println!("对话历史已清除");
                continue;
            }

            print!("ChatAgent: ");
            io::stdout().flush()?;

            match self.chat(None, &line, Some(history.clone())).await {
                Ok(response) => {
                    println!("{}", response.answer);
                    history.push(ChatMessage::User { content: line });
                    history.push(ChatMessage::Assistant {
                        content: Some(response.answer),
                        tool_calls: None,
                    });
                }
                Err(e) => {
                    eprintln!("Error: {:?}", e);
                }
            }
        }
        Ok(())
    }
}

// ─── RAG 多样性筛选 ─────────────────────────────────────────────

/// 对向量搜索结果做多样性筛选：按 chunk 的 section_path 顶层分组，
/// 每组只保留最高分的 1 条，确保注入的上下文覆盖文档的不同章节区域。
///
/// # 分组键
///
/// 使用 chunk title（即 section_path 的最后一级，如"付款方式""合同文本"）
/// 的前 2 个字符做粗粒度分组——同章节的相邻 chunk 会有相同或相似的 title。
/// 每组只保留 score 最高的那条。
///
/// # 填充策略
///
/// 分组后取各组第一名按 score 降序排列，取前 `max_results` 条。
/// 若分组数不足 `max_results`，用未入选的高分候补补齐。
fn diversify_hits(
    hits: &[crate::domain::vector_index::SearchHit],
    max_results: usize,
) -> Vec<crate::domain::vector_index::SearchHit> {
    if hits.len() <= max_results {
        return hits.to_vec();
    }

    // 分组键：section_path 第一级（如"第一章""第二章"）的前 4 字符
    // 若无 title，用 chunk_id 的前缀（如 "ch_0"、"ch_1"）
    use std::collections::HashMap;
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut group_order: Vec<String> = Vec::new();

    for (i, hit) in hits.iter().enumerate() {
        // 使用 title 前几个字符作为分组键（同一区域标题相似）
        let key = if hit.title.is_empty() {
            // fallback: 用 chunk_id 的数字部分（如 ch_027 → "027" 附近）
            hit.chunk_id
                .chars()
                .skip(3) // skip "ch_"
                .take(2)
                .collect::<String>()
        } else {
            hit.title.chars().take(4).collect::<String>()
        };

        if !groups.contains_key(&key) {
            group_order.push(key.clone());
        }
        groups.entry(key).or_default().push(i);
    }

    // 每组取最高分
    let mut selected: Vec<usize> = Vec::new();
    let mut remaining: Vec<usize> = Vec::new();

    for key in &group_order {
        if let Some(indices) = groups.get(key) {
            // 找该组最高分
            let best = indices
                .iter()
                .max_by(|&&a, &&b| {
                    hits[a]
                        .score
                        .partial_cmp(&hits[b].score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .copied()
                .unwrap();
            selected.push(best);
            // 其余作为候补
            for &idx in indices {
                if idx != best {
                    remaining.push(idx);
                }
            }
        }
    }

    // 按 score 排序 selected
    selected.sort_by(|&a, &b| {
        hits[b]
            .score
            .partial_cmp(&hits[a].score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 取前 max_results 条
    let mut result: Vec<crate::domain::vector_index::SearchHit> = selected
        .iter()
        .take(max_results)
        .map(|&i| hits[i].clone())
        .collect();

    // 若不够，用候补补齐
    if result.len() < max_results {
        remaining.sort_by(|&a, &b| {
            hits[b]
                .score
                .partial_cmp(&hits[a].score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &idx in remaining.iter() {
            if result.len() >= max_results {
                break;
            }
            // 避免重复
            if !selected.contains(&idx) {
                result.push(hits[idx].clone());
            }
        }
    }

    result
}

// ─── 测试 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::react_loop::ToolCall;
    use crate::agents::tools::answer_user::AnswerUserTool;
    use std::sync::Mutex;

    // ── Mock LLM 客户端 ─────────────────────────────────────────

    /// 可编程的 Mock LLM 客户端。
    ///
    /// `responses` 队列按顺序返回；超过队列长度后返回最后一个响应。
    /// 使用 `Arc` 共享以避免 `anyhow::Error` 的 Clone 问题。
    struct MockLlmClient {
        /// 每次调用返回的下一个响应索引
        call_index: Mutex<usize>,
        /// 预设的响应队列（全部为 Ok 时可直接访问）
        ok_responses: Vec<LlmResponse>,
        /// 预设的错误队列（按索引返回 Err）
        error_at: std::collections::HashMap<usize, String>,
    }

    impl MockLlmClient {
        /// 创建全为成功响应的 Mock。
        fn new(responses: Vec<LlmResponse>) -> Self {
            Self {
                call_index: Mutex::new(0),
                ok_responses: responses,
                error_at: std::collections::HashMap::new(),
            }
        }

        /// 创建在第 N 次调用时返回错误的 Mock。
        fn with_error(responses: Vec<LlmResponse>, error_index: usize, error_msg: &str) -> Self {
            let mut error_at = std::collections::HashMap::new();
            error_at.insert(error_index, error_msg.to_string());
            Self {
                call_index: Mutex::new(0),
                ok_responses: responses,
                error_at,
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for MockLlmClient {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[serde_json::Value],
            _tool_choice: &ToolChoice,
        ) -> Result<LlmResponse> {
            let mut idx_guard = self.call_index.lock().unwrap();
            let idx = *idx_guard;
            *idx_guard += 1;

            // 检查此索引是否应返回错误
            if let Some(msg) = self.error_at.get(&idx) {
                return Err(anyhow::anyhow!("{}", msg));
            }

            // 返回对应索引的响应，超出则返回最后一个
            let resp = if idx < self.ok_responses.len() {
                &self.ok_responses[idx]
            } else {
                self.ok_responses.last().unwrap_or_else(|| {
                    // 静态默认响应用于空队列场景
                    static DEFAULT: std::sync::LazyLock<LlmResponse> =
                        std::sync::LazyLock::new(|| LlmResponse {
                            thought: None,
                            content: Some("mock fallback".to_string()),
                            tool_calls: vec![],
                            usage: None,
                        });
                    &DEFAULT
                })
            };
            Ok(LlmResponse {
                thought: None,
                content: resp.content.clone(),
                tool_calls: resp.tool_calls.clone(),
                usage: None,
            })
        }
    }

    // ── Helper ──────────────────────────────────────────────────

    /// 创建用于测试的最小 ChatAgent（无配置文件，Mock LLM，空工具集，无 RAG/验证）。
    fn make_test_agent(llm: Arc<dyn LlmClient>) -> ChatAgent {
        let config = ChatAgentConfig {
            max_turns: 3, // 测试用短轮次
            preferences_path: "/nonexistent/test-preferences.md".to_string(),
            project_config_path: "/nonexistent/test-config.md".to_string(),
            ..Default::default()
        };
        ChatAgent {
            config,
            project_config_content: String::new(),
            user_preferences_content: String::new(),
            llm,
            tools: ToolRegistry::new(),
            document_index: None,
            embed_client: None,
            chunks: None,
        }
    }

    fn make_answer_user_response(answer: &str, confidence: f32) -> LlmResponse {
        LlmResponse {
            thought: None,
            content: Some("我来输出最终回答".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "answer_user".to_string(),
                arguments: serde_json::json!({
                    "answer": answer,
                    "confidence": confidence,
                    "references": [{"block_id": "b_3_7", "quote": "投标人须在东莞设有常驻服务机构"}],
                    "knowledge_refs": [
                        {"ref_type": "law", "title": "《政府采购法实施条例》第20条", "excerpt": "禁止以不合理的条件..."}
                    ],
                    "suggested_actions": ["查看 b_10_5 评分标准"]
                }),
            }],
            usage: None,
        }
    }

    // ── parse_answer_user 单元测试 ──────────────────────────────

    #[test]
    fn test_parse_answer_user_full_fields() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let args = serde_json::json!({
            "answer": "这条有合规风险。[b_3_7]原文片段有问题。",
            "confidence": 0.85,
            "references": [
                {"block_id": "b_3_7", "quote": "常驻服务机构"}
            ],
            "knowledge_refs": [
                {"ref_type": "law", "title": "《实施条例》第20条", "excerpt": "禁止..."}
            ],
            "suggested_actions": ["查看 b_10_5", "检查评分标准"]
        });

        let resp = agent.parse_answer_user(&args);

        assert!(resp.answer.contains("合规风险"));
        assert!(resp.answer.contains("[b_3_7]"));
        assert_eq!(resp.confidence, Some(0.85));
        assert_eq!(resp.references.len(), 1);
        assert_eq!(resp.references[0].block_id, "b_3_7");
        assert_eq!(resp.knowledge_refs.len(), 1);
        assert_eq!(resp.knowledge_refs[0].ref_type, "law");
        assert_eq!(resp.suggested_actions.len(), 2);
    }

    #[test]
    fn test_parse_answer_user_block_ref_extraction() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        // answer 中包含多个 [b_xxx] 引用
        let args = serde_json::json!({
            "answer": "[b_3_7]投标人须在东莞设有常驻服务机构。\n关联条款 [b_10_5] 也有问题。\n详见 [b_12_0]。",
            "confidence": 0.9
        });

        let resp = agent.parse_answer_user(&args);

        // 正则提取应产生 3 个 BlockRef（来自 answer 文本，不依赖 references 参数）
        assert_eq!(resp.references.len(), 3);
        assert_eq!(resp.references[0].block_id, "b_3_7");
        assert_eq!(resp.references[1].block_id, "b_10_5");
        assert_eq!(resp.references[2].block_id, "b_12_0");
    }

    #[test]
    fn test_parse_answer_user_empty_args() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let args = serde_json::json!({});

        let resp = agent.parse_answer_user(&args);

        assert!(resp.answer.is_empty());
        assert!(resp.references.is_empty());
        assert!(resp.knowledge_refs.is_empty());
        assert_eq!(resp.confidence, None);
        assert!(resp.suggested_actions.is_empty());
    }

    #[test]
    fn test_parse_answer_user_no_block_refs_in_text() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let args = serde_json::json!({
            "answer": "这条条款合规，没有问题。"
        });

        let resp = agent.parse_answer_user(&args);

        assert!(resp.references.is_empty());
    }

    // ── build_system_prompt 单元测试 ────────────────────────────

    #[test]
    fn test_build_system_prompt_base() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let prompt = agent.build_system_prompt(&None);

        assert!(prompt.contains("AI 招标文件合规审查助手"));
        assert!(prompt.contains("web_search"));
        assert!(prompt.contains("search_document"));
        assert!(prompt.contains("answer_user"));
        // 无配置注入时不应包含配置段
        assert!(!prompt.contains("review-config.md"));
        assert!(!prompt.contains("user-preferences.md"));
        // 无 selection 时不应包含页号
        assert!(!prompt.contains("用户当前在 PDF"));
    }

    #[test]
    fn test_build_system_prompt_with_config() {
        let mut agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        agent.project_config_content = "项目: 测试项目\n审查立场: 供应商视角".to_string();
        agent.user_preferences_content = "回答格式: 表格".to_string();

        let prompt = agent.build_system_prompt(&None);

        assert!(prompt.contains("review-config.md"));
        assert!(prompt.contains("测试项目"));
        assert!(prompt.contains("user-preferences.md"));
        assert!(prompt.contains("回答格式: 表格"));
    }

    #[test]
    fn test_build_system_prompt_with_selection() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let selection = TextSelection {
            text: "投标人须在东莞设有常驻服务机构".to_string(),
            block_ids: vec!["b_3_7".to_string()],
            page: 3,
            bbox: None,
        };

        let prompt = agent.build_system_prompt(&Some(selection));

        assert!(prompt.contains("用户当前在 PDF 第 4 页")); // page+1
        assert!(prompt.contains("投标人须在东莞设有常驻服务机构"));
        assert!(prompt.contains("b_3_7"));
    }

    // ── build_user_message 单元测试 ─────────────────────────────

    #[test]
    fn test_build_user_message_no_selection() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let msg = agent.build_user_message(&None, "这条有问题吗？");
        assert_eq!(msg, "这条有问题吗？");
    }

    #[test]
    fn test_build_user_message_with_selection() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let selection = TextSelection {
            text: "常驻服务机构".to_string(),
            block_ids: vec!["b_3_7".to_string()],
            page: 2,
            bbox: None,
        };

        let msg = agent.build_user_message(&Some(selection), "这条有问题吗？");

        assert!(msg.contains("PDF 第 3 页"));
        assert!(msg.contains("常驻服务机构"));
        assert!(msg.contains("b_3_7"));
        assert!(msg.contains("用户提问: 这条有问题吗？"));
    }

    #[test]
    fn test_build_user_message_selection_empty_input() {
        let agent = make_test_agent(Arc::new(MockLlmClient::new(vec![])));
        let selection = TextSelection {
            text: "常驻服务机构".to_string(),
            block_ids: vec!["b_3_7".to_string()],
            page: 0,
            bbox: None,
        };

        let msg = agent.build_user_message(&Some(selection), "");

        // 空输入 → 默认提示
        assert!(msg.contains("请分析这段文字的合规性"));
    }

    // ── chat() 集成测试（Mock LLM） ─────────────────────────────

    #[tokio::test]
    async fn test_chat_answer_user_termination() {
        // LLM 第一轮就调用 answer_user
        let mock = Arc::new(MockLlmClient::new(vec![make_answer_user_response(
            "这条条款有地域歧视风险。",
            0.9,
        )]));
        let agent = make_test_agent(mock);

        let resp = agent
            .chat(None, "b_3_7 合规吗？", None)
            .await
            .expect("chat 应成功");

        assert!(resp.answer.contains("地域歧视"));
        assert_eq!(resp.confidence, Some(0.9));
        assert_eq!(resp.references.len(), 1);
        assert_eq!(resp.references[0].block_id, "b_3_7");
        assert_eq!(resp.knowledge_refs.len(), 1);
        assert_eq!(resp.suggested_actions.len(), 1);
    }

    #[tokio::test]
    async fn test_chat_direct_text_response() {
        // LLM 直接输出文本（无 tool call）→ 视为最终回答
        let mock = Arc::new(MockLlmClient::new(vec![LlmResponse {
            thought: None,
            content: Some("根据我的分析，这条条款没有合规问题。".to_string()),
            tool_calls: vec![],
            usage: None,
        }]));
        let agent = make_test_agent(mock);

        let resp = agent
            .chat(None, "这条有问题吗？", None)
            .await
            .expect("chat 应成功");

        assert!(resp.answer.contains("没有合规问题"));
        // 直接文本 → 无结构化引用
        assert!(resp.references.is_empty());
        assert_eq!(resp.confidence, None);
    }

    #[tokio::test]
    async fn test_chat_max_turns_exhausted() {
        // LLM 每轮都返回 tool call（非 answer_user），不输出最终回答
        // max_turns=3 → 第3轮被强制 answer_user，但 mock 仍不调用
        let mock = Arc::new(MockLlmClient::new(vec![
            LlmResponse {
                thought: None,
                content: Some("我需要搜索一下法规".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "web_search".to_string(),
                    arguments: serde_json::json!({"question": "地域限制"}),
                }],
                usage: None,
            },
            LlmResponse {
                thought: None,
                content: Some("还需要再查一下".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_2".to_string(),
                    name: "web_search".to_string(),
                    arguments: serde_json::json!({"question": "本地业绩"}),
                }],
                usage: None,
            },
            LlmResponse {
                thought: None,
                content: Some("还需要再查一下".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_3".to_string(),
                    name: "web_search".to_string(),
                    arguments: serde_json::json!({"question": "更多"}),
                }],
                usage: None,
            },
        ]));
        // 工具集中注册一个简单工具供 execute_tool_calls 执行
        let mut agent = make_test_agent(mock);
        agent.tools.register(Box::new(AnswerUserTool));

        let resp = agent
            .chat(None, "审查整个标书", None)
            .await
            .expect("chat 应成功");

        // max_turns 耗尽 → 截断消息
        assert!(resp.answer.contains("分析上限"));
        assert!(
            resp.suggested_actions
                .iter()
                .any(|a| a.contains("拆分为更小的子问题"))
        );
    }

    #[tokio::test]
    async fn test_chat_llm_error() {
        // LLM 第0次调用返回错误
        let mock = Arc::new(MockLlmClient::with_error(
            vec![], // 空响应列表
            0,      // 第0次调用返回错误
            "API 连接超时",
        ));
        let agent = make_test_agent(mock);

        let resp = agent
            .chat(None, "测试", None)
            .await
            .expect("chat 不应 panic");

        assert!(resp.answer.contains("LLM 调用失败"));
        assert!(resp.answer.contains("连接超时"));
    }

    #[tokio::test]
    async fn test_chat_with_history() {
        // 验证历史消息被传递
        let mock = Arc::new(MockLlmClient::new(vec![make_answer_user_response(
            "好的，我来回答你的追问。",
            0.8,
        )]));
        let agent = make_test_agent(mock);

        let history = vec![
            ChatMessage::User {
                content: "b_3_7 合规吗？".to_string(),
            },
            ChatMessage::Assistant {
                content: Some("b_3_7 有地域歧视风险。".to_string()),
                tool_calls: None,
            },
        ];

        let resp = agent
            .chat(None, "那 b_10_5 呢？", Some(history))
            .await
            .expect("chat 应成功");

        assert!(resp.answer.contains("追问"));
    }

    #[tokio::test]
    async fn test_chat_with_selection() {
        // 用户划词 + 提问
        let mock = Arc::new(MockLlmClient::new(vec![make_answer_user_response(
            "选中的条款存在合规风险。",
            0.95,
        )]));
        let agent = make_test_agent(mock);

        let selection = TextSelection {
            text: "投标人须在东莞设有常驻服务机构".to_string(),
            block_ids: vec!["b_3_7".to_string()],
            page: 3,
            bbox: None,
        };

        let resp = agent
            .chat(Some(selection), "这条有问题吗？", None)
            .await
            .expect("chat 应成功");

        assert!(resp.answer.contains("合规风险"));
        assert_eq!(resp.confidence, Some(0.95));
    }

    #[tokio::test]
    async fn test_chat_final_turn_force_answer_user() {
        // 验证第 max_turns 轮传入了 ToolChoice::Specific { "answer_user" }
        // 通过 Mock 记录 tool_choice 来验证
        struct RecordingMock {
            tool_choices: Mutex<Vec<ToolChoice>>,
        }

        #[async_trait::async_trait]
        impl LlmClient for RecordingMock {
            async fn chat(
                &self,
                _messages: &[ChatMessage],
                _tools: &[serde_json::Value],
                tool_choice: &ToolChoice,
            ) -> Result<LlmResponse> {
                self.tool_choices.lock().unwrap().push(tool_choice.clone());
                // 返回一个非 answer_user 的响应 → 让循环继续
                Ok(LlmResponse {
                    thought: None,
                    content: Some("thinking...".to_string()),
                    tool_calls: vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "web_search".to_string(),
                        arguments: serde_json::json!({"question": "test"}),
                    }],
                    usage: None,
                })
            }
        }

        let mock = Arc::new(RecordingMock {
            tool_choices: Mutex::new(Vec::new()),
        });
        let mut agent = make_test_agent(mock.clone());
        agent.config.max_turns = 2; // 2轮 → 第2轮应为 Specific
        agent.tools.register(Box::new(AnswerUserTool));

        let _ = agent.chat(None, "测试", None).await;

        let choices = mock.tool_choices.lock().unwrap();
        assert_eq!(choices.len(), 2, "应调用 2 轮");
        // 第 1 轮: Required（P1 首轮工具强制）
        assert!(
            matches!(choices[0], ToolChoice::Required),
            "第1轮应强制工具调用(Required)，实际为: {:?}",
            choices[0]
        );
        // 第 2 轮 (最后一轮): Specific { "answer_user" }
        assert!(
            matches!(&choices[1], ToolChoice::Specific { name } if name == "answer_user"),
            "最后一轮应强制 answer_user，实际为: {:?}",
            choices[1]
        );
    }

    // ── diversify_hits 单元测试 ─────────────────────────────────

    use crate::domain::vector_index::SearchHit;

    fn make_hit(chunk_id: &str, title: &str, score: f32) -> SearchHit {
        SearchHit {
            chunk_id: chunk_id.to_string(),
            title: title.to_string(),
            score,
            snippet: format!("snippet of {}", chunk_id),
            page_start: 0,
        }
    }

    #[test]
    fn test_diversify_hits_empty() {
        let result = diversify_hits(&[], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn test_diversify_hits_fewer_than_max() {
        let hits = vec![
            make_hit("ch_001", "项目概况", 0.9),
            make_hit("ch_002", "项目概况", 0.8),
        ];
        let result = diversify_hits(&hits, 5);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_diversify_hits_groups_by_title() {
        // 同一章节的多个 chunk 挤在一起 → 每组只保留最高分 1 条优先，
        // 若组数不足 max_results，从候补中按分补齐（可能包含同组低分）
        let hits = vec![
            make_hit("ch_027", "资格条件承诺函", 0.85), // 同组最高分 → 优先入选
            make_hit("ch_028", "资格条件承诺函", 0.72), // 同组低分 → 候补
            make_hit("ch_029", "资格条件承诺函", 0.65), // 同组更低分
            make_hit("ch_138", "合同文本", 0.71),       // 合同组最高分 → 优先入选
            make_hit("ch_158", "合同文本", 0.68),       // 同组低分 → 候补
            make_hit("ch_200", "技术需求", 0.60),       // 技术组最高分 → 优先入选
        ];
        let result = diversify_hits(&hits, 5);

        // 3 组优先入选: ch_027, ch_138, ch_200
        // 剩余 2 个从候补补齐: ch_028 (0.72), ch_158 (0.68)
        let ids: Vec<&str> = result.iter().map(|h| h.chunk_id.as_str()).collect();
        assert_eq!(result.len(), 5, "应填满 5 条，实际: {:?}", ids);
        // 各组最高分必须在
        assert!(
            ids.contains(&"ch_027"),
            "ch_027 (资格组最高分) 应入选: {:?}",
            ids
        );
        assert!(
            ids.contains(&"ch_138"),
            "ch_138 (合同组最高分) 应入选: {:?}",
            ids
        );
        assert!(
            ids.contains(&"ch_200"),
            "ch_200 (技术组最高分) 应入选: {:?}",
            ids
        );
        // ch_029 (0.65) 是最低分，应该被挤掉
        assert!(
            !ids.contains(&"ch_029"),
            "ch_029 (最低分 0.65) 应被挤出: {:?}",
            ids
        );
        // 排序验证：前 3 条应按分降序（各组优胜者按分排序）
        assert_eq!(result[0].chunk_id, "ch_027", "最高分应排第一");
    }

    #[test]
    fn test_diversify_hits_all_same_group() {
        // 所有结果都在同一章节 → 返回 Top-N
        let hits = vec![
            make_hit("ch_001", "项目概况", 0.9),
            make_hit("ch_002", "项目概况", 0.8),
            make_hit("ch_003", "项目概况", 0.7),
            make_hit("ch_004", "项目概况", 0.6),
            make_hit("ch_005", "项目概况", 0.5),
        ];
        let result = diversify_hits(&hits, 3);
        // 同组只有 1 个（最高分 0.9），剩余 2 个从候补补齐
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].chunk_id, "ch_001"); // 最高分
    }

    // ── filter_emojis 单元测试 ──────────────────────────────

    #[test]
    fn test_filter_emojis_no_emoji() {
        // 纯文本无 emoji → 原样返回
        let text = "这是正常的回答文本，包含中文和English。";
        assert_eq!(ChatAgent::filter_emojis(text), text);
    }

    #[test]
    fn test_filter_emojis_single_emoji() {
        // 单个 emoji → 移除
        assert_eq!(ChatAgent::filter_emojis("✅ 通过"), " 通过");
        assert_eq!(ChatAgent::filter_emojis("⚠️ 警告"), " 警告");
        assert_eq!(ChatAgent::filter_emojis("❌ 不通过"), " 不通过");
    }

    #[test]
    fn test_filter_emojis_multiple_emojis() {
        // 多个 emoji → 全部移除
        assert_eq!(
            ChatAgent::filter_emojis("🔴 高风险 🟡 中风险 🟢 低风险"),
            " 高风险  中风险  低风险"
        );
    }

    #[test]
    fn test_filter_emojis_inline_emoji() {
        // emoji 嵌入在句子中 → 仅移除 emoji
        let filtered = ChatAgent::filter_emojis("供应商📌必须在东莞设有🔹常驻机构");
        assert!(!filtered.contains('📌'));
        assert!(!filtered.contains('🔹'));
        assert!(filtered.contains("供应商"));
        assert!(filtered.contains("必须在东莞设有"));
        assert!(filtered.contains("常驻机构"));
    }

    #[test]
    fn test_filter_emojis_all_emoji() {
        // 全部是 emoji → 返回空字符串
        let filtered = ChatAgent::filter_emojis("😀🎉✅⚠️");
        assert!(filtered.trim().is_empty());
    }

    #[test]
    fn test_filter_emojis_emoji_with_text_formatting() {
        // emoji 与 markdown 格式混用
        let filtered = ChatAgent::filter_emojis("**[重要]** ⚠️ 请核实资质文件。");
        assert!(!filtered.contains('⚠'));
        assert!(filtered.contains("**[重要]**"));
        assert!(filtered.contains("请核实资质文件。"));
    }

    #[test]
    fn test_filter_emojis_empty() {
        // 空字符串
        assert_eq!(ChatAgent::filter_emojis(""), "");
    }

    #[test]
    fn test_filter_emojis_table_with_emoji() {
        // 表格中带 emoji → 移除 emoji 保留表格结构
        let text = "| 状态 | 说明 |\n| ✅ | 合格 |\n| ❌ | 不合格 |";
        let filtered = ChatAgent::filter_emojis(text);
        assert!(!filtered.contains('✅'));
        assert!(!filtered.contains('❌'));
        assert!(filtered.contains("| 状态 |"));
        assert!(filtered.contains("| 合格 |"));
        assert!(filtered.contains("| 不合格 |"));
    }
}
