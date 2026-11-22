//! 混拼查询成本探针：`char_graph` 在 `clean=false` 时会对短段做混拼查询，
//! 这里量冷/热两种情况下每次查询的成本。
//!
//! 用法: cargo run --release --example lat_abbrev

use std::path::Path;
use std::time::Instant;

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

    // zhongg 的全部 1..=4 字符子串（char_graph 会实际查的那些段）
    let input = "zhongg";
    let n = input.len();
    let mut segs: Vec<String> = Vec::new();
    for start in 0..n {
        for len in 1..=4usize {
            if start + len <= n {
                segs.push(input[start..start + len].to_string());
            }
        }
    }
    segs.sort();
    segs.dedup();

    println!("段数 = {} : {:?}\n", segs.len(), segs);

    // 冷：首次查询（缓存未命中）
    let t = Instant::now();
    let mut cold_words = 0usize;
    for s in &segs {
        cold_words += taishen_engine::dictionary::abbrev_span_candidates(s, 8).len();
    }
    let cold = t.elapsed().as_micros();

    // 热：再次查询（全部命中全局缓存）
    let t = Instant::now();
    let mut hot_words = 0usize;
    for s in &segs {
        hot_words += taishen_engine::dictionary::abbrev_span_candidates(s, 8).len();
    }
    let hot = t.elapsed().as_micros();

    println!("冷查询: {cold}us  命中词 {cold_words}");
    println!("热查询: {hot}us  命中词 {hot_words}");
    println!(
        "平均单段: 冷 {:.0}us / 热 {:.0}us",
        cold as f64 / segs.len() as f64,
        hot as f64 / segs.len() as f64
    );
}
