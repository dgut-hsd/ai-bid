//! API Key 连通性测试 — 独立可执行脚本。
//!
//! 运行: cargo run --bin test_api_key
//!
//! 自动从 .env 加载 DASHSCOPE_API_KEY，使用与主程序相同的 DashScope 原生协议。

use anyhow::{Context, Result};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<()> {
    // 加载 .env：依次尝试当前目录 → data_dir → 上级目录（开发时 .env 在项目根）
    dotenv::dotenv().ok();
    let data_env = ai_bid::paths::data_dir().join(".env");
    if data_env.exists() {
        dotenv::from_path(&data_env).ok();
    }
    if let Some(parent) = std::env::current_dir()
        .ok()
        .and_then(|d| d.parent().map(|p| p.join(".env")))
    {
        if parent.exists() {
            dotenv::from_path(&parent).ok();
        }
    }

    println!("══════════════════════════════════════════════");
    println!("  API Key 连通性测试");
    println!("══════════════════════════════════════════════");
    println!();

    // ── 读取环境变量 ──
    let api_key = std::env::var("DASHSCOPE_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .context("❌ DASHSCOPE_API_KEY 或 OPENAI_API_KEY 未设置（检查 .env）")?;

    let model = std::env::var("DASHSCOPE_MODEL").unwrap_or_else(|_| "qwen-plus".to_string());

    let endpoint = "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation";

    // 安全预览 Key（不输出完整密钥）
    let key_preview = if api_key.len() > 12 {
        format!(
            "{}...{}",
            &api_key[..8],
            &api_key[api_key.len().saturating_sub(4)..]
        )
    } else {
        "***".to_string()
    };

    println!("  协议:   DashScope 原生");
    println!("  端点:   {}", endpoint);
    println!("  模型:   {}", model);
    println!("  Key:    {}", key_preview);
    println!();

    // ── 构建请求体（与 DashScopeNativeClient::chat 完全一致）──
    let body = serde_json::json!({
        "model": model,
        "input": {
            "messages": [
                {"role": "system", "content": "你是一个测试助手。"},
                {"role": "user", "content": "请回复：API连通测试成功"}
            ]
        },
        "parameters": {
            "result_format": "message",
            "max_tokens": 50,
            "temperature": 0.0,
        }
    });

    println!("  正在发送请求...");

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("❌ 创建 HTTP 客户端失败")?;

    let response = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("❌ 网络请求失败（检查网络 / API 端点可达性）")?;

    let status = response.status();
    let response_body: Value = response.json().await.context("❌ 解析响应 JSON 失败")?;

    // ── 错误处理 ──
    if !status.is_success() {
        println!();
        println!("❌ API 返回错误 HTTP {}:", status);
        println!(
            "{}",
            serde_json::to_string_pretty(&response_body).unwrap_or_default()
        );
        anyhow::bail!("API 调用失败 — 请检查 API Key 是否有效");
    }

    // ── 解析 DashScope 原生响应 ──
    let choice = response_body["output"]["choices"]
        .as_array()
        .and_then(|arr| arr.first())
        .context("❌ DashScope 返回空 output.choices")?;

    let content = choice["message"]["content"]
        .as_str()
        .unwrap_or("(无文本回复)");

    // 兼容两种 usage 位置：output.usage（原生）/ 顶层 usage
    let usage = response_body
        .get("output")
        .and_then(|o| o.get("usage"))
        .or_else(|| response_body.get("usage"));

    let input_tokens = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .and_then(|u| u.get("total_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    println!();
    println!("✅ 连通测试成功！");
    println!("   回复:    {}", content);
    println!(
        "   Tokens:  prompt={}, completion={}, total={}",
        input_tokens, output_tokens, total_tokens,
    );
    println!();
    println!("══════════════════════════════════════════════");
    println!("  API Key 有效，可以正常使用。");
    println!("══════════════════════════════════════════════");

    Ok(())
}
