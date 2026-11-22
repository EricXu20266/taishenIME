//! 复现实机环境：加载真实用户词库 + 上下文搭配，对比与纯 CLI 的差异。
//!
//! 用法: cargo run --release --example live_ctx_check -- ces fax haiy

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dict = root.join("resources/system_dict.db");
    let user_db = std::env::var("APPDATA")
        .map(|a| Path::new(&a).join("taishen-ime").join("user_dict.db"))
        .unwrap_or_default();

    taishen_engine::dictionary::init(Some(dict.as_path()));
    if user_db.exists() {
        taishen_engine::dictionary::set_user_dict_path(Some(user_db.as_path()));
        println!("用户词库: {}", user_db.display());
    }
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let pairs = taishen_engine::dictionary::load_context_pairs(2, 8);
    println!("学习型上下文搭配: {} 条", pairs.len());

    let args: Vec<String> = std::env::args().skip(1).collect();
    let inputs: Vec<&str> = if args.is_empty() {
        vec!["ces", "fax", "haiy"]
    } else {
        args.iter().map(String::as_str).collect()
    };

    // 四种组合：是否加载上下文 × 是否开上下文
    for (label, use_ctx, ctx_on) in [
        ("纯 CLI（无上下文、上下文关）", false, false),
        ("加载搭配但不启用", true, false),
        ("加载搭配 + 启用", true, true),
    ] {
        println!("\n=== {label} ===");
        for input in &inputs {
            let mut e = taishen_engine::Engine::new();
            e.set_candidate_limit(9);
            if use_ctx {
                e.load_learned_contexts(pairs.clone());
            }
            e.set_context_enabled(ctx_on);
            for ch in input.chars() {
                e.process_key(ch);
            }
            let top: Vec<String> = (0..9)
                .filter_map(|i| e.candidate_display(i).map(|t| t.to_string()))
                .collect();
            println!("  {input:>6} → {}", top.join(" | "));
        }
    }
}
