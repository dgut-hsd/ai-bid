//! 千问 LLM 连接测试 — 独立可执行脚本。
//!
//! 运行: cargo run --bin test_llm

use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // 加载 .env：依次尝试当前目录 → 上级目录（开发时 .env 在项目根）
    dotenv::dotenv().ok();
    if let Some(parent) = std::env::current_dir()
        .ok()
        .and_then(|d| d.parent().map(|p| p.join(".env")))
    {
        if parent.exists() {
            dotenv::from_path(&parent).ok();
        }
    }
    println!("=== 千问 LLM 连接测试 ===\n");

    let api_key =
        std::env::var("OPENAI_API_KEY").context("❌ OPENAI_API_KEY 未设置（检查 .env 文件）")?;
    let api_base = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string());
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "qwen-max".to_string());

    let key_preview = if api_key.len() > 12 {
        format!(
            "{}...{}",
            &api_key[..8],
            &api_key[api_key.len().saturating_sub(4)..]
        )
    } else {
        "***".to_string()
    };
    println!("  端点: {}", api_base);
    println!("  模型: {}", model);
    println!("  Key:  {}", key_preview);

    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    println!("\n  正在连接 {} ...", url);

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "你是一个测试助手。"},
            {"role": "user", "content": "请回复：连通测试成功"}
        ],
        "max_tokens": 50,
        "temperature": 0.0,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("❌ 网络请求失败")?;

    let status = response.status();
    let response_body: serde_json::Value = response.json().await.context("❌ 解析响应失败")?;

    if !status.is_success() {
        println!("\n❌ API 返回错误 {}:", status);
        println!(
            "{}",
            serde_json::to_string_pretty(&response_body).unwrap_or_default()
        );
        anyhow::bail!("API 调用失败");
    }

    let content = response_body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(无文本回复)");
    let usage = &response_body["usage"];

    println!();
    println!("✅ 连接成功！");
    println!("   回复: {}", content);
    println!(
        "   tokens: prompt={}, completion={}, total={}",
        usage["prompt_tokens"].as_u64().unwrap_or(0),
        usage["completion_tokens"].as_u64().unwrap_or(0),
        usage["total_tokens"].as_u64().unwrap_or(0),
    );

    Ok(())
}
