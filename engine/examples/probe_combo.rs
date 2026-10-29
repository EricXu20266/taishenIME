//! 组合候选探针（诊断用）：对比 query / query_combo / combo_guess / phrase_group_guess
//!
//! 用法: cargo run --release --example probe_combo -- anxia dianjishiyong

use std::path::Path;
use taishen_engine::dictionary;

fn main() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    for a in std::env::args().skip(1) {
        println!("\n=== {a} ===");
        let q: Vec<String> = dictionary::query(&a).into_iter().take(10).collect();
        println!("  query            : {q:?}");
        let qs: Vec<String> = dictionary::query_short(&a).into_iter().take(10).collect();
        println!("  query_short      : {qs:?}");
        let qc: Vec<String> = dictionary::query_combo(&a).into_iter().take(12).collect();
        println!("  query_combo      : {qc:?}");
        let cg: Vec<String> = dictionary::combo_guess(&a).into_iter().take(12).collect();
        println!("  combo_guess      : {cg:?}");
        let pg: Vec<String> = dictionary::phrase_group_guess(&a)
            .into_iter()
            .take(12)
            .collect();
        println!("  phrase_group     : {pg:?}");
        println!(
            "  is_complete      : {} / prefix {}",
            taishen_engine::pinyin::is_complete_pinyin(&a),
            taishen_engine::pinyin::is_valid_pinyin_prefix(&a)
        );
        println!(
            "  common?          : 北京={} 点击={} 按下={}",
            dictionary::is_common_word("\u{5317}\u{4eac}"),
            dictionary::is_common_word("\u{70b9}\u{51fb}"),
            dictionary::is_common_word("\u{6309}\u{4e0b}")
        );
    }
}
