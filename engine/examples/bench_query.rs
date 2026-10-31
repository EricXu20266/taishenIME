//! 候选生成耗时基线（审计报告 §6.3 附录 A 的 1、3 项）。
//!
//! 用法：cargo run --release --example bench_query
//!
//! 输出每个输入串的字符数、P50 / P95 耗时（微秒）、首屏候选数。
//! 用于对照 A1（去重集合）/ A2（原地分区）前后是否真的省下了时间。

use std::path::Path;
use std::time::Instant;

use taishen_engine::Engine;

const CASES: &[&str] = &[
    "women",           // 2 音节，候选多
    "zhongguo",        // 2 音节，常用词
    "zhegeweomende",   // 5 音节，触发多个纠错分支
    "dianjishiy",      // 4-5 音节，纠错 + 锚定组合
    "xinashi",         // 纠错（相邻键）
    "womenceshi",      // 长串，组合联想
    "zhegewumende",    // 长串，锚定拆分
    "xitongkongzhitai", // 6 音节，长串压力
    "nihaoshijie",     // 4 音节，长句过滤路径
    "hello",           // 英文混输
];

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
    if !taishen_engine::dictionary::is_ready() {
        eprintln!("词库未就绪，退出");
        return;
    }

    let mut engine = Engine::new();
    // 预热：第一次查询会触发索引页加载
    for ch in "warmup".chars() {
        engine.process_key(ch);
    }
    engine.reset();

    println!("{:<20} {:>4} {:>10} {:>10} {:>8}", "input", "len", "p50(us)", "p95(us)", "pages");
    for input in CASES {
        let mut samples: Vec<u64> = Vec::with_capacity(100);
        let mut pages = 0usize;
        for _ in 0..100 {
            engine.reset();
            let t = Instant::now();
            for ch in input.chars() {
                engine.process_key(ch);
            }
            samples.push(t.elapsed().as_micros() as u64);
            pages = engine.total_pages();
        }
        samples.sort_unstable();
        let p50 = samples[samples.len() / 2];
        let p95 = samples[samples.len() * 95 / 100];
        println!(
            "{:<20} {:>4} {:>10} {:>10} {:>8}",
            input,
            input.chars().count(),
            p50,
            p95,
            pages
        );
    }
}
