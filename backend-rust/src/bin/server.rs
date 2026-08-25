//! HTTP 微服务入口。
//!
//! 启动 axum 服务器，暴露 REST API 供 Java 业务后端调用。
//!
//! ## 运行方式
//!
//! ```powershell
//! cargo run --bin server
//! ```
//!
//! 或设置环境变量后运行：
//!
//! ```powershell
//! $env:AIBID_DATA_DIR = ".."
//! cargo run --bin server
//! ```

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 加载 .env：依次尝试当前目录 → data_dir → 上级目录（开发时 .env 在项目根）
    dotenv::dotenv().ok();
    let data_env = ai_bid::paths::data_dir().join(".env");
    if data_env.exists() {
        dotenv::from_path(&data_env).ok();
    }
    if let Some(parent) = std::env::current_dir().ok().and_then(|d| d.parent().map(|p| p.join(".env"))) {
        if parent.exists() {
            dotenv::from_path(&parent).ok();
        }
    }

    println!("╔══════════════════════════════════════╗");
    println!("║  智能标书审核引擎 — HTTP 微服务     ║");
    println!("╚══════════════════════════════════════╝");

    let embed_engine = std::env::var("EMBED_ENGINE").unwrap_or_else(|_| "local".to_string());
    println!("  嵌入引擎: {}", embed_engine);

    // 初始化共享状态（加载 BGE-M3 模型等）
    println!("  正在加载模型...");
    let state = ai_bid::api::handlers::AppState::init().await?;
    println!("  模型加载完成");

    let router = ai_bid::api::router::build(state);
    let bind_addr = std::env::var("AIBID_RUST_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3001".to_string());

    println!("  监听地址: http://{}", bind_addr);
    println!("    POST /api/v1/documents        上传并处理文档");
    println!("    GET  /api/v1/documents/:id    查询文档状态");
    println!("    POST /api/v1/documents/:id/review  运行审核");
    println!("    POST /api/v1/documents/:id/chat    对话问答");
    println!("    POST /api/v1/documents/:id/search  语义搜索");
    println!("    GET  /health                   健康检查");
    println!();

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
