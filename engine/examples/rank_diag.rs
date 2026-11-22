//! 排序诊断探针：打印每个候选的 tier / match_level / reasons / source。
//!
//! 用法：
//!   cargo run --release --example rank_diag              # 不加载用户词库
//!   cargo run --release --example rank_diag -- --user    # 加载真实用户词库
//!
//! 用于定性「同一输入在两种环境下候选顺序不同」的成因。

use std::path::PathBuf;
use taishen_engine::{Engine, dictionary};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let db = root.join("resources/system_dict.db");
    let use_user = std::env::args().any(|a| a == "--user");

    let user_db = std::env::var("APPDATA")
        .ok()
        .map(|a| PathBuf::from(a).join("taishen-ime").join("user_dict.db"))
        .filter(|p| p.exists());

    dictionary::init(Some(db.as_path()));
    for _ in 0..600 {
        if dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // 用户词库必须在 init + 就绪之后设置（Dictionary 实例已存在）
    if use_user {
        match &user_db {
            Some(u) => dictionary::set_user_dict_path(Some(u.as_path())),
            None => println!("[WARN] 未找到用户词库"),
        }
    }

    println!(
        "[env] user_dict={} ({:?})",
        use_user,
        user_db.as_ref().map(|p| p.display().to_string())
    );

    for input in ["ces", "fax", "haiy"] {
        let mut e = Engine::new();
        e.set_candidate_limit(9);
        for ch in input.chars() {
            e.process_key(ch);
        }
        println!("=== {input} ===");
        for i in 0..9 {
            let Some(text) = e.candidate_display(i) else {
                break;
            };
            let src = e.candidate_source_name(i).unwrap_or("?");
            let m = e.candidate_explain(i).unwrap_or_default();
            let ml = m
                .match_level
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string());
            println!(
                "  #{:<2} {:<10} src={:<12} tier={} match={} score={:?} reasons={}",
                i + 1,
                text,
                src,
                m.tier,
                ml,
                m.score,
                taishen_engine::format_reasons(m.reasons)
            );
        }
    }
}
