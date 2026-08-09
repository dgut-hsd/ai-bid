//! Group A Real Agent→Tool Smoke Baseline（一次性运行）。
//!
//! 运行: cargo run --bin eval_smoke
//!
//! 链路（全部复用生产组件）：
//! EvalCase → 真实 Agent system prompt → 真实 Agent tool list
//! → DashScopeNativeClient（生产 provider）→ tool_calls
//! → 真实 Tool.execute → tool result 回传 → final response。
//!
//! 配置：DASHSCOPE_API_KEY（.env 或环境变量），DASHSCOPE_MODEL（默认 qwen-plus）。
//! 产出：eval_results/<run_id>/run_summary.json + case_results.jsonl。

use ai_bid::services::llm_client::DashScopeNativeClient;
use ai_bid::agents::tools::eval_harness::{
    RunConfig, build_eval_registry, production_smoke_cases, run_eval, save_results,
};
use anyhow::{Context, Result};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // ── 加载 .env（与 main.rs 一致：cwd → data_dir；另外加载项目根 .env）──
    dotenv::dotenv().ok();
    let data_env = ai_bid::paths::data_dir().join(".env");
    if data_env.exists() {
        dotenv::from_path(data_env).ok();
    }
    // CARGO_MANIFEST_DIR 的父目录 = 项目根（.env 实际位置）
    let root_env = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join(".env"));
    if let Some(env_path) = root_env {
        if env_path.exists() {
            dotenv::from_path(env_path).ok();
        }
    }

    let api_key = std::env::var("DASHSCOPE_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .context("❌ DASHSCOPE_API_KEY 未设置（检查项目根 .env）——REAL BASELINE BLOCKED")?;
    let model = std::env::var("DASHSCOPE_MODEL").unwrap_or_else(|_| "qwen-plus".to_string());

    println!("[eval_smoke] provider=dashscope_native model={} key_len={}", model, api_key.len());

    let client = DashScopeNativeClient::new(&api_key, &model);

    let mut cfg = RunConfig::production_default();
    cfg.model = model.clone();
    cfg.provider = "dashscope_native".to_string();
    cfg.prompt_variant = "CurrentProductionPrompt".to_string();
    cfg.repetitions = 1;

    let cases = production_smoke_cases();
    println!("[eval_smoke] cases={} repetitions=1 tool_choice=auto web_search=disabled", cases.len());

    let registry = build_eval_registry();
    let (summary, results) = run_eval(&cfg, &client, &cases, &registry).await?;

    // ── 保存 artifacts ──
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval_results").join(&summary.run_id);
    save_results(&out_dir, &summary, &results)?;
    println!("[eval_smoke] artifacts -> {}", out_dir.display());

    // ── 摘要 ──
    println!();
    println!("=== Smoke Summary ===");
    println!("run_id:          {}", summary.run_id);
    println!("provider:        {} model: {}", summary.provider, summary.model);
    println!("prompt_variant:  {} (hash {})", summary.prompt_variant, summary.prompt_hash);
    println!("web_search:      {}", summary.web_search);
    println!("git_revision:    {}", summary.git_revision);
    println!("case_count:      {} completed", summary.case_count);
    println!();
    let m = |x: &ai_bid::agents::tools::eval_harness::Metric| {
        format!("{}/{} = {:.1}%", x.numerator, x.denominator, x.rate() * 100.0)
    };
    println!("Required Tool Recall:          {}", m(&summary.required_recall));
    println!("Preferred Tool Use Rate:       {}", m(&summary.preferred_use_rate));
    println!("Tool Precision:                {}", m(&summary.tool_precision));
    println!("Wrong Tool Rate:               {}", m(&summary.wrong_tool_rate));
    println!("False Call Rate:               {}", m(&summary.false_call_rate));
    println!("Argument JSON Valid Rate:      {}", m(&summary.argument_json_valid_rate));
    println!("Required-Field Schema Valid:   {}", m(&summary.argument_deserialize_success_rate));
    println!("Expected Key Arg Recall:       {}", m(&summary.expected_key_arg_recall));
    println!("Tool Execution Success Rate:   {}", m(&summary.tool_execution_success_rate));
    println!("Result Adoption Rate:          {} (SMOKE HEURISTIC)", m(&summary.result_adoption_rate));
    println!("Final Tool-Consistency Rate:   {}", m(&summary.final_tool_consistency_rate));

    // ── Case-Level 失败 ──
    let failures: Vec<_> = results
        .iter()
        .filter(|r| {
            !r.selected_expected_tool
                || (r.expectation == ai_bid::agents::tools::eval_harness::Expectation::Negative
                    && r.false_call)
                || !r.execution_success
                || r.tool_result_consistency == "contradicted"
        })
        .collect();
    println!();
    println!("=== Case-Level Failures ({}) ===", failures.len());
    for r in &failures {
        let calls: Vec<String> = r
            .tool_calls
            .iter()
            .map(|tc| {
                format!(
                    "{}[{}]({})",
                    tc.tool_name,
                    tc.execution_status,
                    if tc.error.is_some() { "err" } else { "ok" }
                )
            })
            .collect();
        println!(
            "case={} agent={} expectation={:?} expected={:?} actual_calls=[{}] args_presence={:.2} consistency={} err={:?}",
            r.case_id,
            r.agent_id,
            r.expectation,
            r.expected_tool,
            calls.join(", "),
            r.argument_presence_score,
            r.tool_result_consistency,
            r.error
        );
    }

    println!();
    println!("[eval_smoke] done. artifacts: {}", out_dir.display());
    Ok(())
}
