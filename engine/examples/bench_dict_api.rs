//! 词库 API 级耗时归因：定位长串输入慢在哪个查询函数。
//!
//! 用法：cargo run --release --example bench_dict_api

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use taishen_engine::dictionary as dict;

fn bench(name: &str, mut f: impl FnMut() -> usize) {
    // 预热
    black_box(f());
    let mut samples = Vec::with_capacity(50);
    let mut hits = 0usize;
    for _ in 0..50 {
        let t = Instant::now();
        hits = f();
        samples.push(t.elapsed().as_micros() as u64);
        black_box(hits);
    }
    samples.sort_unstable();
    println!(
        "{:<22} {:>8} us  {:>8} us  hits={}",
        name,
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100],
        hits
    );
}

fn main() {
    let db = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    dict::init(Some(db.as_path()));
    for _ in 0..600 {
        if dict::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !dict::is_ready() {
        eprintln!("词库未就绪，退出");
        return;
    }

    for input in ["zhegeweomende", "xitongkongzhitai"] {
        println!("=== {input} ===");
        bench("query (全拼前缀)", || dict::query(input).len());
        bench("query_short (简拼)", || dict::query_short(input).len());
        bench("query_mixed (混合)", || dict::query_mixed(input).len());
        bench("query_abbrev_full", || dict::query_abbrev_full(input).len());
        bench("query_combo", || dict::query_combo(input).len());
        bench("combo_guess", || dict::combo_guess(input).len());
        bench("phrase_group_guess", || dict::phrase_group_guess(input).len());
    }
}
