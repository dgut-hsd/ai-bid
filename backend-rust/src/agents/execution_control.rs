//! 审核管线的全局并发、文档公平额度、超时和预算控制。

use crate::agents::react_loop::{ChatMessage, LlmClient, LlmResponse, ToolChoice};
use crate::agents::types::StageExecutionFailure;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub const DEFAULT_GLOBAL_CONCURRENCY: usize = 12;
pub const DEFAULT_DOCUMENT_CONCURRENCY: usize = 3;
/// 主分析（Execute / LegalVerify / Debate）共享的 Token 预算上限（输入+输出）。
///
/// 实测：约 85 条款标书消耗 ~2.03M token、120 条款标书在 20 分钟 Execute 硬墙内消耗
/// ~2.75M token，成本仅 ~¥1.9（DashScope qwen-turbo 约 ¥0.7/M）。故该预算并非成本约束，
/// 而是「到点交卷」的优雅降级阀：撞上限时各 Agent 快速失败并返回已完成的发现，避免被
/// Execute 超时 abort 后把已完成结果一并丢弃。2M 偏低（85 条款标书差 3 条即被截断），
/// 2.5M 是「覆盖正常大标书 + 在 20 分钟硬墙前优雅交卷」的甜点。
/// 证据核验（EvidenceVerify）不占此预算（其调用使用裸 LLM 客户端）。
pub const DEFAULT_BUDGET_TOTAL_TOKENS: u64 = 2_500_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStage {
    Pipeline,
    BatchSearch,
    Execute,
    LegalVerify,
    Debate,
    BlindSpot,
    EvidenceVerify,
}

impl ExecutionStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pipeline => "pipeline",
            Self::BatchSearch => "batch_search",
            Self::Execute => "execute",
            Self::LegalVerify => "legal_verify",
            Self::Debate => "debate",
            Self::BlindSpot => "blind_spot",
            Self::EvidenceVerify => "evidence_verify",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionLimits {
    pub global_concurrency: usize,
    pub document_concurrency: usize,
    pub call_timeout: Duration,
    pub clause_timeout: Duration,
    pub batch_search_timeout: Duration,
    pub execute_timeout: Duration,
    pub legal_verify_timeout: Duration,
    pub debate_timeout: Duration,
    pub pipeline_timeout: Duration,
    pub evidence_verify_concurrency: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            document_concurrency: DEFAULT_DOCUMENT_CONCURRENCY,
            call_timeout: Duration::from_secs(60),
            clause_timeout: Duration::from_secs(180),
            batch_search_timeout: Duration::from_secs(120),
            execute_timeout: Duration::from_secs(20 * 60),
            legal_verify_timeout: Duration::from_secs(5 * 60),
            debate_timeout: Duration::from_secs(5 * 60),
            pipeline_timeout: Duration::from_secs(60 * 60),
            evidence_verify_concurrency: 6,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct BudgetLimits {
    pub llm_calls: usize,
    pub tool_calls: usize,
    pub web_search_calls: usize,
    pub total_tokens: u64,
}

impl BudgetLimits {
    /// 按工作负载动态计算预算。Token 上限默认 2.5M（`AIBID_BUDGET_TOTAL_TOKENS` 可配，
    /// 区间 100k~100M）。
    ///
    /// Token 预算的角色是「到点交卷」的优雅降级阀，而非成本约束：主分析（Execute /
    /// LegalVerify / Debate）共享该预算，撞上限时各 Agent 快速失败并返回已完成的发现；
    /// 证据核验（EvidenceVerify）使用裸 LLM 客户端、不占此预算，因此不会被主分析打满
    /// 预算后前置拦截、导致误报无法降级。
    pub fn for_workload(effective_tasks: usize, source_clauses: usize) -> Self {
        let token_cap = std::env::var("AIBID_BUDGET_TOTAL_TOKENS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_BUDGET_TOTAL_TOKENS)
            .clamp(100_000, 100_000_000);
        Self {
            llm_calls: (effective_tasks.saturating_mul(6)).clamp(30, 600),
            tool_calls: (effective_tasks.saturating_mul(10)).clamp(60, 1_000),
            web_search_calls: (source_clauses.saturating_mul(2)).clamp(10, 120),
            total_tokens: (effective_tasks as u64)
                .saturating_mul(28_000)
                .clamp(100_000, token_cap),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct BudgetUsage {
    pub limits: BudgetLimits,
    pub llm_calls: usize,
    pub tool_calls: usize,
    pub web_search_calls: usize,
    pub total_tokens: u64,
    pub exhausted: bool,
    pub exhausted_reason: Option<String>,
}

pub struct GlobalExecutionLimiter {
    semaphore: Arc<Semaphore>,
    limits: ExecutionLimits,
}

impl GlobalExecutionLimiter {
    pub fn new(limits: ExecutionLimits) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(limits.global_concurrency.max(1))),
            limits,
        }
    }

    pub fn from_env() -> Self {
        let mut limits = ExecutionLimits::default();
        limits.global_concurrency = env_usize(
            "AIBID_GLOBAL_CONCURRENCY",
            DEFAULT_GLOBAL_CONCURRENCY,
            1,
            128,
        );
        limits.document_concurrency = env_usize(
            "AIBID_DOCUMENT_CONCURRENCY",
            DEFAULT_DOCUMENT_CONCURRENCY,
            1,
            limits.global_concurrency,
        );
        // 审核管线总超时（分钟），默认 60；大标书（如 92/145 页）需更长时间完成
        // 证据核验(EvidenceVerify)。此前硬编码 30 分钟，导致大标书在 30 分钟红线处
        // 跳过证据核验、把未验证的发现直接落库（详见 docs/审核稳定性修复计划.md）。
        limits.pipeline_timeout = Duration::from_secs(
            (env_usize("AIBID_PIPELINE_TIMEOUT_MINUTES", 60, 5, 1440) as u64) * 60,
        );
        // 证据核验的多组并行度：串行时 79 组 × ~14s ≈ 18 分钟，并行后按该上限分批。
        limits.evidence_verify_concurrency = env_usize("AIBID_EVIDENCE_VERIFY_CONCURRENCY", 6, 1, 32);
        Self::new(limits)
    }

    pub fn start_review(
        self: &Arc<Self>,
        effective_tasks: usize,
        source_clauses: usize,
    ) -> Arc<ReviewExecutionControl> {
        Arc::new(ReviewExecutionControl {
            global: self.semaphore.clone(),
            document: Arc::new(Semaphore::new(self.limits.document_concurrency.max(1))),
            limits: self.limits.clone(),
            budget_limits: BudgetLimits::for_workload(effective_tasks, source_clauses),
            llm_calls: AtomicUsize::new(0),
            tool_calls: AtomicUsize::new(0),
            web_search_calls: AtomicUsize::new(0),
            total_tokens: AtomicU64::new(0),
            exhausted_reason: std::sync::Mutex::new(None),
            failed_stages: std::sync::Mutex::new(Vec::new()),
            started_at: Instant::now(),
        })
    }
}

pub struct ReviewExecutionControl {
    global: Arc<Semaphore>,
    document: Arc<Semaphore>,
    limits: ExecutionLimits,
    budget_limits: BudgetLimits,
    llm_calls: AtomicUsize,
    tool_calls: AtomicUsize,
    web_search_calls: AtomicUsize,
    total_tokens: AtomicU64,
    exhausted_reason: std::sync::Mutex<Option<String>>,
    failed_stages: std::sync::Mutex<Vec<StageExecutionFailure>>,
    started_at: Instant,
}

pub struct ExecutionPermit {
    _document: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

/// 调用成功后提交预算；未提交即离开作用域时自动回滚预留。
pub struct BudgetReservation<'a> {
    counter: &'a AtomicUsize,
    committed: bool,
}

impl BudgetReservation<'_> {
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for BudgetReservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.counter.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl ReviewExecutionControl {
    pub async fn acquire(self: &Arc<Self>) -> Result<ExecutionPermit> {
        let document = self
            .document
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("文档并发控制器已关闭"))?;
        let global = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("全局并发控制器已关闭"))?;
        Ok(ExecutionPermit {
            _document: document,
            _global: global,
        })
    }

    pub fn limits(&self) -> &ExecutionLimits {
        &self.limits
    }

    pub fn pipeline_remaining(&self) -> Option<Duration> {
        self.limits
            .pipeline_timeout
            .checked_sub(self.started_at.elapsed())
    }

    pub fn pipeline_expired(&self) -> bool {
        self.pipeline_remaining().is_none()
    }

    pub fn reserve_llm_call(&self) -> Result<BudgetReservation<'_>> {
        if self.total_tokens.load(Ordering::SeqCst) >= self.budget_limits.total_tokens {
            return Err(anyhow!("Token 预算已耗尽"));
        }
        reserve_counter(
            &self.llm_calls,
            self.budget_limits.llm_calls,
            "LLM 调用预算已耗尽",
            &self.exhausted_reason,
        )
    }

    pub fn reserve_tool_call(&self, tool_name: &str) -> Result<BudgetReservation<'_>> {
        if tool_name == "web_search" {
            reserve_counter(
                &self.web_search_calls,
                self.budget_limits.web_search_calls,
                "联网搜索预算已耗尽",
                &self.exhausted_reason,
            )
        } else {
            reserve_counter(
                &self.tool_calls,
                self.budget_limits.tool_calls,
                "工具调用预算已耗尽",
                &self.exhausted_reason,
            )
        }
    }

    pub fn record_tokens(&self, tokens: u64) {
        let total = self.total_tokens.fetch_add(tokens, Ordering::SeqCst) + tokens;
        if total >= self.budget_limits.total_tokens {
            record_exhaustion(&self.exhausted_reason, "Token 预算已耗尽");
        }
    }

    pub fn budget_usage(&self) -> BudgetUsage {
        let llm_calls = self.llm_calls.load(Ordering::SeqCst);
        let tool_calls = self.tool_calls.load(Ordering::SeqCst);
        let web_search_calls = self.web_search_calls.load(Ordering::SeqCst);
        let total_tokens = self.total_tokens.load(Ordering::SeqCst);
        let exhausted_reason = self
            .exhausted_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone());
        BudgetUsage {
            limits: self.budget_limits.clone(),
            llm_calls,
            tool_calls,
            web_search_calls,
            total_tokens,
            exhausted: exhausted_reason.is_some(),
            exhausted_reason,
        }
    }

    pub fn record_stage_failure(&self, stage: ExecutionStage, message: impl Into<String>) {
        if let Ok(mut failures) = self.failed_stages.lock() {
            let stage_name = stage.as_str();
            if failures.iter().all(|failure| failure.stage != stage_name) {
                failures.push(StageExecutionFailure {
                    stage: stage_name.to_string(),
                    message: message.into(),
                });
            }
        }
    }

    pub fn record_pipeline_timeout_if_expired(&self) {
        if self.pipeline_expired() {
            let minutes = self.limits.pipeline_timeout.as_secs() / 60;
            self.record_stage_failure(
                ExecutionStage::Pipeline,
                format!("审核管线超过 {minutes} 分钟硬上限"),
            );
        }
    }

    pub fn failed_stages(&self) -> Vec<StageExecutionFailure> {
        self.failed_stages
            .lock()
            .map(|failures| failures.clone())
            .unwrap_or_default()
    }
}

fn reserve_counter<'a>(
    counter: &'a AtomicUsize,
    limit: usize,
    reason: &str,
    exhausted_reason: &std::sync::Mutex<Option<String>>,
) -> Result<BudgetReservation<'a>> {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            (current < limit).then_some(current + 1)
        })
        .map(|_| BudgetReservation {
            counter,
            committed: false,
        })
        .map_err(|_| {
            record_exhaustion(exhausted_reason, reason);
            anyhow!(reason.to_string())
        })
}

fn record_exhaustion(exhausted_reason: &std::sync::Mutex<Option<String>>, reason: &str) {
    if let Ok(mut current) = exhausted_reason.lock()
        && current.is_none()
    {
        *current = Some(reason.to_string());
    }
}

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

pub struct ControlledLlmClient {
    inner: Box<dyn LlmClient>,
    control: Arc<ReviewExecutionControl>,
}

impl ControlledLlmClient {
    pub fn wrap(
        inner: Box<dyn LlmClient>,
        control: Arc<ReviewExecutionControl>,
    ) -> Box<dyn LlmClient> {
        Box::new(Self { inner, control })
    }
}

#[async_trait::async_trait]
impl LlmClient for ControlledLlmClient {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        tool_choice: &ToolChoice,
    ) -> Result<LlmResponse> {
        let reservation = self.control.reserve_llm_call()?;
        let response = tokio::time::timeout(
            self.control.limits.call_timeout,
            self.inner.chat(messages, tools, tool_choice),
        )
        .await
        .map_err(|_| anyhow!("单次 LLM 调用超过 60 秒"))??;
        reservation.commit();
        if let Some(usage) = &response.usage {
            self.control.record_tokens(usage.total_tokens as u64);
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysFailLlm;

    #[async_trait::async_trait]
    impl LlmClient for AlwaysFailLlm {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[serde_json::Value],
            _tool_choice: &ToolChoice,
        ) -> Result<LlmResponse> {
            Err(anyhow!("模拟调用失败"))
        }
    }

    #[test]
    fn budget_limits_match_workload_formula() {
        let limits = BudgetLimits::for_workload(40, 20);
        assert_eq!(limits.llm_calls, 240, "40 任务 × 6 = 240（clamp 30..600 内）");
        assert_eq!(limits.tool_calls, 400, "40 任务 × 10 = 400（clamp 60..1000 内）");
        assert_eq!(limits.web_search_calls, 40, "20 条款 × 2 = 40（clamp 10..120 内）");
        assert_eq!(
            limits.total_tokens,
            1_120_000,
            "40 任务 × 28000 = 1.12M（未触 2.5M 上限也未低于 100k 下限）"
        );
    }

    #[test]
    fn llm_call_budget_is_enforced() {
        let limiter = Arc::new(GlobalExecutionLimiter::new(ExecutionLimits::default()));
        let control = limiter.start_review(1, 1);

        // start_review(1,1) → llm_calls = clamp(1*6, 30, 600) = 30
        for _ in 0..30 {
            control
                .reserve_llm_call()
                .expect("预算内调用应被允许")
                .commit();
        }
        assert!(
            control.reserve_llm_call().is_err(),
            "超过 llm_calls 上限后应被拒绝"
        );
        let usage = control.budget_usage();
        assert_eq!(usage.llm_calls, 30);
        assert!(usage.exhausted, "预算耗尽后应标记 exhausted");
        assert_eq!(
            usage.exhausted_reason.as_deref(),
            Some("LLM 调用预算已耗尽")
        );
    }

    #[test]
    fn dropped_reservation_releases_budget() {
        let limiter = Arc::new(GlobalExecutionLimiter::new(ExecutionLimits::default()));
        let control = limiter.start_review(1, 1);

        let reservation = control.reserve_llm_call().expect("预算内调用应被允许");
        drop(reservation);

        assert_eq!(
            control.budget_usage().llm_calls,
            0,
            "未完成调用不得消耗预算"
        );
    }

    #[test]
    fn tool_budget_is_independent_of_llm_budget() {
        let limiter = Arc::new(GlobalExecutionLimiter::new(ExecutionLimits::default()));
        let control = limiter.start_review(1, 1);

        // start_review(1,1) → tool_calls = clamp(1*10, 60, 1000) = 60
        for _ in 0..60 {
            control
                .reserve_tool_call("read_section")
                .expect("工具预算内应被允许")
                .commit();
        }
        assert!(control.reserve_tool_call("read_section").is_err());
        assert!(
            control.reserve_llm_call().is_ok(),
            "工具预算耗尽不得影响 LLM 调用预算"
        );
    }

    #[tokio::test]
    async fn failed_llm_call_releases_reserved_budget() {
        let limiter = Arc::new(GlobalExecutionLimiter::new(ExecutionLimits::default()));
        let control = limiter.start_review(1, 1);
        let client = ControlledLlmClient::wrap(Box::new(AlwaysFailLlm), control.clone());

        let result = client.chat(&[], &[], &ToolChoice::Auto).await;

        assert!(result.is_err());
        assert_eq!(control.budget_usage().llm_calls, 0);
    }

    #[test]
    fn web_search_uses_independent_budget() {
        let limiter = Arc::new(GlobalExecutionLimiter::new(ExecutionLimits::default()));
        let control = limiter.start_review(1, 1);

        control
            .reserve_tool_call("web_search")
            .expect("联网搜索预算内调用应被允许")
            .commit();
        let usage = control.budget_usage();
        assert_eq!(usage.web_search_calls, 1);
        assert_eq!(usage.tool_calls, 0, "联网搜索不得挤占核心工具预算");
    }

    #[test]
    fn stage_failure_is_recorded_once() {
        let limiter = Arc::new(GlobalExecutionLimiter::new(ExecutionLimits::default()));
        let control = limiter.start_review(1, 1);
        control.record_stage_failure(ExecutionStage::Debate, "第一次超时");
        control.record_stage_failure(ExecutionStage::Debate, "重复超时");

        assert_eq!(control.failed_stages().len(), 1);
        assert_eq!(control.failed_stages()[0].message, "第一次超时");
    }

    #[tokio::test]
    async fn document_limit_does_not_consume_unused_global_capacity() {
        let limiter = Arc::new(GlobalExecutionLimiter::new(ExecutionLimits {
            global_concurrency: 2,
            document_concurrency: 1,
            ..ExecutionLimits::default()
        }));
        let document_a = limiter.start_review(1, 1);
        let document_b = limiter.start_review(1, 1);
        let _a = document_a.acquire().await.expect("文档 A 应取得第一个名额");

        assert!(
            tokio::time::timeout(Duration::from_millis(20), document_a.acquire())
                .await
                .is_err(),
            "同一文档不得超过自身并发上限"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(20), document_b.acquire())
                .await
                .is_ok(),
            "其他文档应能使用剩余全局名额"
        );
    }
}
