//! 拆组搭配打分对比：同一输入在有/无 bigram 表时的候选顺序。
//!
//! 用法: cargo run --release --example probe_bigram -- [bigram.db] [输入...]
//! 不传 bigram.db 时只跑无搭配表基线。

use std::path::Path;
use std::time::Instant;
use taishen_engine::dictionary;

const DEFAULT_INPUTS: &[&str] = &[
    "dianjishiyong",
    "dianjishiy",
    "zhegeshiyong",
    "womenyong",
    "zhegeweomende",
    "womenceshi",
    "tashiyong",
    "womendedianhua",
];

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
    let mut inputs: Vec<String> = Vec::new();
    let mut bigram_path: Option<String> = None;
    if let Some(first) = args.first() {
        if first.ends_with(".db") {
            bigram_path = Some(first.clone());
            inputs.extend(args.iter().skip(1).cloned());
        } else {
            inputs.extend(args.iter().cloned());
        }
    }
    if inputs.is_empty() {
        inputs.extend(DEFAULT_INPUTS.iter().map(|s| s.to_string()));
    }

    if let Some(p) = &bigram_path {
        let t = Instant::now();
        let pairs = dictionary::set_bigram(Path::new(p), 1, usize::MAX);
        println!("bigram 已加载: {pairs} 对，{} ms", t.elapsed().as_millis());
        for (a, b) in [("他", "使用"), ("踏", "实用"), ("我们", "的")] {
            println!("  score({a},{b}) = {}", dictionary::bigram_score(a, b));
        }
    } else {
        println!("bigram: 未加载（基线）");
    }

    for input in &inputs {
        let t = Instant::now();
        let out = dictionary::phrase_group_guess(input);
        println!(
            "{input:<16} {:>6.2}ms  {}",
            t.elapsed().as_secs_f64() * 1000.0,
            out.join(" / ")
        );
    }
}
