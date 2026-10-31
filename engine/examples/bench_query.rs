//! 候选生成耗时基线（审计报告 §6.3 附录 A）。
//!
//! 用法：cargo run --release --example bench_query
//!
//! 两个指标要分开看：
//! - total：从空输入打到完整串的总耗时（= N 次 query_all 累加）
//! - last ：打最后一个键的单次耗时（= 用户真正感知的按键延迟）
//! 把 total 当成单键延迟会高估一个数量级。

use std::path::Path;
use std::time::Instant;

use taishen_engine::Engine;

const CASES: &[&str] = &[
    "women",
    "zhongguo",
    "nihao",
    "nihaoshijie",
    "zhegeweomende",
    "dianjishiy",
    "xinashi",
    "womenceshi",
    "zhegewumende",
    "xitongkongzhitai",
    "hello",
];

fn p50_p95(mut samples: Vec<u64>) -> (u64, u64) {
    samples.sort_unstable();
    (
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100],
    )
}

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
    for ch in "warmup".chars() {
        engine.process_key(ch);
    }
    engine.reset();

    println!(
        "{:<18} {:>4} {:>14} {:>14} {:>12} {:>12}",
        "input", "len", "total p50", "total p95", "lastkey p50", "lastkey p95"
    );
    for input in CASES {
        let chars: Vec<char> = input.chars().collect();
        let mut totals = Vec::with_capacity(40);
        for _ in 0..40 {
            engine.reset();
            let t = Instant::now();
            for &c in &chars {
                engine.process_key(c);
            }
            totals.push(t.elapsed().as_micros() as u64);
        }
        let mut lasts = Vec::with_capacity(40);
        if chars.len() > 1 {
            for _ in 0..40 {
                engine.reset();
                for &c in &chars[..chars.len() - 1] {
                    engine.process_key(c);
                }
                let t = Instant::now();
                engine.process_key(chars[chars.len() - 1]);
                lasts.push(t.elapsed().as_micros() as u64);
            }
        } else {
            lasts.push(0);
        }
        let (t50, t95) = p50_p95(totals);
        let (l50, l95) = p50_p95(lasts);
        println!(
            "{:<18} {:>4} {:>12}us {:>12}us {:>10}us {:>10}us",
            input,
            chars.len(),
            t50,
            t95,
            l50,
            l95
        );
    }
}
