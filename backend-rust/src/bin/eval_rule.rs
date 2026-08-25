//! 规则引擎验证小工具 —— 直接调用 `candidate_categories()`。
//!
//! 用途：喂一句招标条款原文，看规则引擎认不认得出「这是哪种违规」。
//! 纯函数，不碰 LLM / Agent / 网络 / 主审核链路。
//!
//! 用法：
//!
//!   cargo run --bin eval_rule -- "投标人须在本市注册成立三年以上"
//!
//!   # 也支持不带引号（多个参数会自动拼成一句）：
//!   cargo run --bin eval_rule -- 投标人 须在 本市 注册
//!
//! 输出示例：
//!
//!   条款：投标人须在本市注册成立三年以上，且在本市设有分支机构。
//!     → 命中 LOCAL_REGISTRATION（地域注册限制，责任 Agent = SemanticRiskAgent）
//!
//! 说明：本工具只验证「分类命中」；Critical 红线判定（哪些是红线）
//! 见 tests/rule_engine_verify.rs 里的测试 4。

use ai_bid::rules::catalog::{display_name, owner_agent};
use ai_bid::rules::engine::candidate_categories;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("用法：cargo run --bin eval_rule -- \"<条款原文>\"");
        eprintln!("示例：cargo run --bin eval_rule -- \"投标人须在本市注册成立三年以上\"");
        std::process::exit(2);
    }
    let text = args.join(" ");

    let hits = candidate_categories(&text);

    println!("条款：{text}");
    if hits.is_empty() {
        println!("  → 未命中任何风险（合规）");
    } else {
        for code in &hits {
            let name = display_name(code).unwrap_or(code);
            let agent = owner_agent(code);
            println!("  → 命中 {code}（{name}，责任 Agent = {agent}）");
        }
    }
}