//! 候选排序诊断探针：一次看清「词库原始顺序 / 整句解码 / 最终候选」三层。
//!
//! 用法: cargo run --release --example probe_rank -- youxianji
//!
//! 存在的意义：用户报「优先级被三个非词压到第 4 位」时，需要分清是
//! 词库没这个词、还是整句解码插队、还是排序链把它降组了。

use std::path::Path;
use taishen_engine::Engine;

fn main() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "youxianji".to_string());

    println!("=== dictionary::query(\"{input}\") ===");
    for (i, w) in taishen_engine::dictionary::query(&input)
        .iter()
        .enumerate()
        .take(12)
    {
        println!(
            "  q[{i}] {w} common={}",
            taishen_engine::dictionary::is_common_word(w)
        );
    }

    println!("=== sentence::decode ===");
    for (i, w) in taishen_engine::sentence::decode(&input, None, 3)
        .iter()
        .enumerate()
    {
        println!("  s[{i}] {w}");
    }

    let mut e = Engine::new();
    e.set_candidate_limit(12);
    for ch in input.chars() {
        e.process_key(ch);
    }
    println!("=== engine candidates ({} 条) ===", e.candidate_count());
    for i in 0..e.candidate_count().min(12) {
        let word = e.candidate(i).unwrap_or("");
        let src = e.candidate_source_name(i).unwrap_or("?");
        let explain = match e.candidate_explain(i) {
            Some(m) => {
                let tags = taishen_engine::format_reasons(m.reasons);
                match m.score {
                    Some(sc) => format!("T{} score={sc} [{tags}]", m.tier),
                    None => format!("T{} [{tags}]", m.tier),
                }
            }
            None => String::new(),
        };
        println!("  c[{i}] {word} ({src}) {explain}");
    }
}
