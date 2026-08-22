//! ReviewEventBus — Coordinator 到前端的 SSE 实时推送通道。
//!
//! 设计文档 §17.1 实现。
//!
//! ## 与 AgentBus 的区别
//!
//! | 通道 | 方向 | 消费方 | 事件粒度 | 用途 |
//! |------|------|--------|---------|------|
//! | AgentBus | Agent ↔ Agent | 其他 Agent（ReActLoop 内） | 仅 High 风险 | 跨 Agent 实时协作 |
//! | ReviewEventBus | Coordinator → 外部 | SSE 客户端（Java/前端） | 全部事件 | 前端双层展示（L1+L2） |
//!
//! ## 事件类型
//!
//! - `phase` — 管线阶段切换（ROUTE/EXECUTE/MERGE/…）
//! - `agent_progress` — 单个 Agent 的审查进度
//! - `trace` — ReAct 循环内的细粒度事件（思考/工具调用/结果）
//! - `finding_added` — 稳定后的 risk finding（通过 MERGE+VERIFY）
//! - `finding_updated` — finding 字段变更（降级/辩论裁决）
//! - `finding_removed` — finding 被消除（去重合并）
//! - `stats` — 阶段性统计快照
//! - `done` — 审查完成
//! - `partial_done` — 审查部分完成，携带失败 Agent/条款明细
//! - `error` — 审查执行失败

use serde::Serialize;
use tokio::sync::broadcast;

/// 管线阶段。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelinePhase {
    Scout,
    Route,
    Execute,
    Merge,
    LegalVerify,
    BlindSpot,
    Debate,
    Triage,
}

/// Finding 生命周期状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingLifecycle {
    /// 通过法条验证
    Verified,
    /// 盲点扫描发现
    BlindSpot,
    /// 辩论后维持
    Debated,
}

/// Finding 字段变更记录。
#[derive(Debug, Clone, Serialize)]
pub struct FindingChange {
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_value: Option<String>,
}

/// 审查事件 —— Coordinator 到客户端的单向推送。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum ReviewEvent {
    /// 管线阶段切换
    #[serde(rename = "phase")]
    Phase {
        phase: PipelinePhase,
        phase_index: u8,
        total_phases: u8,
        message: String,
    },

    /// 单个 Agent 的审查进度
    #[serde(rename = "agent_progress")]
    AgentProgress {
        agent_id: String,
        agent_label: String,
        clauses_done: usize,
        clauses_total: usize,
        raw_findings: usize,
        status: String, // "pending" | "running" | "completed" | "failed"
    },

    /// ReAct 循环细粒度事件
    #[serde(rename = "trace")]
    Trace {
        event_type: String,
        agent_name: String,
        turn: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        clause_id: Option<String>,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<serde_json::Value>,
    },

    /// Finding 进入稳定状态（L1 主视图可用）
    #[serde(rename = "finding_added")]
    FindingAdded {
        risk_id: String,
        severity: String,
        is_critical: bool,
        critical_reason: String,
        risk_type: String,
        agent: String,
        confidence: f64,
        clause_ids: Vec<String>,
        source_quote: String,
        legal_basis: Vec<String>,
        reason: String,
        suggestion: String,
        lifecycle: FindingLifecycle,
        #[serde(skip_serializing_if = "Option::is_none")]
        page_number: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        section_path: Option<Vec<String>>,
    },

    /// Finding 被更新
    #[serde(rename = "finding_updated")]
    FindingUpdated {
        risk_id: String,
        changes: Vec<FindingChange>,
        reason: String,
    },

    /// Finding 被移除
    #[serde(rename = "finding_removed")]
    FindingRemoved {
        risk_id: String,
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        merged_into: Option<String>,
    },

    /// 阶段性统计快照
    #[serde(rename = "stats")]
    Stats {
        phase: PipelinePhase,
        total_raw: usize,
        total_merged: usize,
        total_verified: usize,
        high: usize,
        medium: usize,
        low: usize,
        info: usize,
    },

    /// 审查完成
    #[serde(rename = "done")]
    Done {
        total_findings: usize,
        high_risk: usize,
        session_id: String,
        duration_secs: f64,
    },

    /// 审查仅部分完成，保留成功结果并携带失败明细。
    #[serde(rename = "partial_done")]
    PartialDone {
        total_findings: usize,
        high_risk: usize,
        session_id: String,
        duration_secs: f64,
        failed_agents: Vec<crate::agents::types::AgentExecutionFailure>,
        failed_clauses: Vec<crate::agents::types::ClauseExecutionFailure>,
        failed_stages: Vec<crate::agents::types::StageExecutionFailure>,
        #[serde(skip_serializing_if = "Option::is_none")]
        budget: Option<crate::agents::execution_control::BudgetUsage>,
    },

    /// 审查执行失败
    #[serde(rename = "error")]
    Error { message: String, session_id: String },
}

/// ReviewEventBus — 审查事件广播通道。
///
/// 内部使用 `tokio::sync::broadcast` 实现，每个 review session
/// 拥有独立的 channel（按 doc_id 隔离在 AppState 中）。
pub struct ReviewEventBus {
    sender: broadcast::Sender<String>,
}

impl ReviewEventBus {
    /// 创建新的 ReviewEventBus。
    ///
    /// * `capacity` — 通道缓冲容量。超出时最旧的事件被丢弃并
    ///   在 receiver 端表现为 `Lagged` 错误。
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// 获取一个新的 Receiver，用于 SSE 流消费。
    ///
    /// 每个 SSE 客户端应调用一次 `subscribe()` 获取独立的 Receiver。
    /// 注意：`tokio::sync::broadcast` 不保证订阅前的历史消息可消费，
    /// 因此 SSE 客户端应先 `subscribe()` 再触发审查。
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }

    /// 发射一个审查事件。
    ///
    /// 事件被序列化为 JSON 字符串后发送。如果 channel 已满（无活跃
    /// receiver 或所有 receiver 落后），消息被静默丢弃。
    pub fn emit(&self, event: &ReviewEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            let _ = self.sender.send(json);
        }
    }

    /// 发射事件并附加 SSE event 类型前缀。
    ///
    /// 发送格式: `event:{event_type}\n{json}`。
    /// 接收端可按 SSE 协议解析。
    #[allow(dead_code)]
    pub(crate) fn emit_sse(&self, event_type: &str, json: &str) {
        let payload = format!("event:{}\n{}", event_type, json);
        let _ = self.sender.send(payload);
    }
}

// ─── 测试 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_and_receive() {
        let bus = ReviewEventBus::new(32);
        let mut rx = bus.subscribe();

        bus.emit(&ReviewEvent::Phase {
            phase: PipelinePhase::Execute,
            phase_index: 2,
            total_phases: 7,
            message: "7 个 Agent 并行审查中...".to_string(),
        });

        let msg = rx.try_recv().expect("应收到消息");
        assert!(msg.contains("\"phase\""), "应包含 phase 事件类型");
        assert!(msg.contains("execute"), "应包含 execute phase");
    }

    #[test]
    fn test_agent_progress_event() {
        let bus = ReviewEventBus::new(32);
        let mut rx = bus.subscribe();

        bus.emit(&ReviewEvent::AgentProgress {
            agent_id: "fiscal_compliance".to_string(),
            agent_label: "财政合规Agent".to_string(),
            clauses_done: 23,
            clauses_total: 45,
            raw_findings: 3,
            status: "running".to_string(),
        });

        let msg = rx.try_recv().expect("应收到消息");
        assert!(msg.contains("agent_progress"));
        assert!(msg.contains("fiscal_compliance"));
        assert!(msg.contains("23"));
    }

    #[test]
    fn test_partial_done_event_contains_failure_details() {
        let event = ReviewEvent::PartialDone {
            total_findings: 2,
            high_risk: 1,
            session_id: "doc-partial".to_string(),
            duration_secs: 1.5,
            failed_agents: vec![crate::agents::types::AgentExecutionFailure {
                agent_id: "missing-agent".to_string(),
                message: "Agent 定义未找到".to_string(),
            }],
            failed_clauses: vec![crate::agents::types::ClauseExecutionFailure {
                agent_id: "missing-agent".to_string(),
                clause_id: "ch_001".to_string(),
                message: "Agent 定义未找到".to_string(),
            }],
            failed_stages: vec![crate::agents::types::StageExecutionFailure {
                stage: "batch_search".to_string(),
                message: "BatchSearch 阶段超时".to_string(),
            }],
            budget: Some(crate::agents::execution_control::BudgetUsage {
                limits: crate::agents::execution_control::BudgetLimits::for_workload(1, 1),
                llm_calls: 2,
                tool_calls: 3,
                web_search_calls: 1,
                total_tokens: 1_000,
                exhausted: false,
                exhausted_reason: None,
            }),
        };

        let json = serde_json::to_value(event).expect("partial_done 应可序列化");
        assert_eq!(json["event"], "partial_done");
        assert_eq!(
            json["data"]["failed_agents"][0]["agent_id"],
            "missing-agent"
        );
        assert_eq!(json["data"]["failed_clauses"][0]["clause_id"], "ch_001");
        assert_eq!(json["data"]["failed_stages"][0]["stage"], "batch_search");
        assert_eq!(json["data"]["budget"]["limits"]["llm_calls"], 30);
    }

    #[test]
    fn test_trace_event() {
        let bus = ReviewEventBus::new(32);
        let mut rx = bus.subscribe();

        bus.emit(&ReviewEvent::Trace {
            event_type: "agent_thought".to_string(),
            agent_name: "财政合规Agent".to_string(),
            turn: 1,
            clause_id: Some("ch_042".to_string()),
            summary: "该条款构成变相指定品牌".to_string(),
            payload: None,
        });

        let msg = rx.try_recv().expect("应收到消息");
        assert!(msg.contains("agent_thought"));
        assert!(msg.contains("ch_042"));
    }

    #[test]
    fn test_finding_added_event() {
        let bus = ReviewEventBus::new(32);
        let mut rx = bus.subscribe();

        bus.emit(&ReviewEvent::FindingAdded {
            risk_id: "R_001".to_string(),
            severity: "high".to_string(),
            is_critical: true,
            critical_reason: "唯一品牌且拒绝同等产品".to_string(),
            risk_type: "品牌指定".to_string(),
            agent: "财政合规Agent".to_string(),
            confidence: 0.91,
            clause_ids: vec!["ch_042".to_string()],
            source_quote: "投标人须选用华为...".to_string(),
            legal_basis: vec!["政府采购法第20条".to_string()],
            reason: "构成变相指定品牌".to_string(),
            suggestion: "修改为功能参数要求".to_string(),
            lifecycle: FindingLifecycle::Verified,
            page_number: Some(3),
            section_path: Some(vec!["技术要求".to_string()]),
        });

        let msg = rx.try_recv().expect("应收到消息");
        assert!(msg.contains("finding_added"));
        assert!(msg.contains("R_001"));
        assert!(msg.contains("high"));
    }

    #[test]
    fn test_finding_updated_event() {
        let bus = ReviewEventBus::new(32);
        let mut rx = bus.subscribe();

        bus.emit(&ReviewEvent::FindingUpdated {
            risk_id: "R_003".to_string(),
            changes: vec![FindingChange {
                field: "severity".to_string(),
                old_value: Some("high".to_string()),
                new_value: Some("info".to_string()),
            }],
            reason: "法条引用验证未通过".to_string(),
        });

        let msg = rx.try_recv().expect("应收到消息");
        assert!(msg.contains("finding_updated"));
        assert!(msg.contains("severity"));
    }

    #[test]
    fn test_finding_removed_event() {
        let bus = ReviewEventBus::new(32);
        let mut rx = bus.subscribe();

        bus.emit(&ReviewEvent::FindingRemoved {
            risk_id: "R_002".to_string(),
            reason: "去重合并".to_string(),
            merged_into: Some("R_001".to_string()),
        });

        let msg = rx.try_recv().expect("应收到消息");
        assert!(msg.contains("finding_removed"));
        assert!(msg.contains("R_001"));
    }

    #[test]
    fn test_done_event() {
        let bus = ReviewEventBus::new(32);
        let mut rx = bus.subscribe();

        bus.emit(&ReviewEvent::Done {
            total_findings: 8,
            high_risk: 3,
            session_id: "doc_abc".to_string(),
            duration_secs: 187.5,
        });

        let msg = rx.try_recv().expect("应收到消息");
        assert!(msg.contains("done"));
        assert!(msg.contains("8"));
        assert!(msg.contains("3"));
    }

    #[test]
    fn test_multi_subscriber() {
        let bus = ReviewEventBus::new(32);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.emit(&ReviewEvent::Stats {
            phase: PipelinePhase::Execute,
            total_raw: 12,
            total_merged: 8,
            total_verified: 5,
            high: 3,
            medium: 2,
            low: 2,
            info: 1,
        });

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn test_late_subscriber_may_miss() {
        let bus = ReviewEventBus::new(32);

        // 先发一条消息
        bus.emit(&ReviewEvent::Phase {
            phase: PipelinePhase::Route,
            phase_index: 1,
            total_phases: 7,
            message: "路由中...".to_string(),
        });

        // 后订阅
        let mut late_rx = bus.subscribe();

        // 再发一条
        bus.emit(&ReviewEvent::Phase {
            phase: PipelinePhase::Execute,
            phase_index: 2,
            total_phases: 7,
            message: "审查中...".to_string(),
        });

        // 后订阅者应能收到订阅后的消息
        let msg = late_rx.try_recv().expect("应收到订阅后的消息");
        assert!(msg.contains("execute"));
    }
}
