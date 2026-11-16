//! 分段探针（诊断用，Completion 实现期临时工具）
//!
//! 用法: cargo run --release --example probe_split -- <输入串...>
//! 输出 dictionary 查询的「精确段 / 前缀扩展段」分界与两侧样本。

use std::path::Path;

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

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cases: Vec<String> = if args.is_empty() {
        vec!["lian".into(), "tong".into(), "xian".into(), "shi".into()]
    } else {
        args
    };

    for p in &cases {
        let (list, exact) = taishen_engine::dictionary::query_with_split(p);
        println!(
            "=== {} 总 {} | 精确 {} | 扩展 {} | is_complete={} ===",
            p,
            list.len(),
            exact,
            list.len().saturating_sub(exact),
            taishen_engine::pinyin::is_complete_pinyin(p)
        );
        let ex: Vec<&str> = list[..exact.min(10)].iter().map(String::as_str).collect();
        println!("  精确前10: {:?}", ex);
        let from = exact.min(list.len());
        let to = (exact + 15).min(list.len());
        let ext: Vec<&str> = list[from..to].iter().map(String::as_str).collect();
        println!("  扩展前15: {:?}", ext);
        let comp = taishen_engine::dictionary::completion_candidates(p, 8);
        println!("  completion: {:?}", comp);
    }
}
