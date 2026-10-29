//! 候选生成性能探针：测 process_key 到候选就绪的单次耗时
//!
//! 用法: cargo run --release --example probe_perf -- [轮数]

use std::path::Path;
use std::time::Instant;
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

    let mut args = std::env::args().skip(1);
    let rounds: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

    // 覆盖不同分支：完整拼音命中 / 组合兜底 / 锚定插队 / 英文
    let defaults = [
        "zhongguo",
        "beijing",
        "anxia",
        "dianjishiyong",
        "dianjishiy",
        "nihao",
        "hello",
        "women",
    ];
    let cases: Vec<String> = if args.len() > 0 {
        args.collect()
    } else {
        defaults.iter().map(|s| s.to_string()).collect()
    };
    let cases: Vec<&str> = cases.iter().map(|s| s.as_str()).collect();

    let mut engine = Engine::new();
    for input in cases {
        // 预热
        engine.reset();
        for ch in input.chars() {
            engine.process_key(ch);
        }
        let _ = engine.candidate_count();

        let t = Instant::now();
        for _ in 0..rounds {
            engine.reset();
            for ch in input.chars() {
                engine.process_key(ch);
            }
            let n = engine.candidate_count();
            let _ = engine.candidate(0);
            debug_assert!(n > 0 || input == "zzzz");
        }
        let us = t.elapsed().as_micros() as f64 / rounds as f64;
        println!("{input:<16} {us:>8.1} µs/次");
    }
}
