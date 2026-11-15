//! 整句格子查询探针：打印若干拼音键在 span_candidates 里的返回。
//!
//! 用法: cargo run --release --example probe_span -- [拼音...]

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let keys: Vec<String> = if args.is_empty() {
        ["beijing", "zheli", "tianqi", "nimen", "jige", "women", "chifan", "zai"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        args
    };
    for key in keys {
        let words = dictionary::span_candidates(&key, 8);
        println!("{key:<10} -> {words:?}");
        // 对照：普通 query（含前缀扩展）
        let q = dictionary::query(&key);
        println!("{:<10}    query 前 5: {:?}", "", &q[..q.len().min(5)]);
    }
}
