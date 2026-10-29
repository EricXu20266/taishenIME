//! 分段耗时探针：把一次长串查询拆到 dictionary 各子查询上，定位热点。
//!
//! 用法: cargo run --release --example probe_stage -- zhegeweomende

use std::path::Path;
use std::time::Instant;
use taishen_engine::dictionary as d;

fn main() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    d::init(Some(dict.as_path()));
    for _ in 0..600 {
        if d::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let inputs: Vec<String> = {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            vec!["zhegeweomende".into(), "womenceshi".into()]
        } else {
            args
        }
    };

    for input in &inputs {
        // 预热
        let _ = d::query(input);
        println!("=== {input} ===");
        let t = Instant::now();
        let n = d::query(input).len();
        println!("  query              {:>8.2} ms  ({n} 条)", ms(t));
        let t = Instant::now();
        let n = d::query_short(input).len();
        println!("  query_short        {:>8.2} ms  ({n} 条)", ms(t));
        let t = Instant::now();
        let n = d::query_mixed(input).len();
        println!("  query_mixed        {:>8.2} ms  ({n} 条)", ms(t));
        let t = Instant::now();
        let n = d::query_abbrev_full(input).len();
        println!("  query_abbrev_full  {:>8.2} ms  ({n} 条)", ms(t));
        let t = Instant::now();
        let n = d::query_combo(input).len();
        println!("  query_combo        {:>8.2} ms  ({n} 条)", ms(t));
        let t = Instant::now();
        let n = d::combo_guess(input).len();
        println!("  combo_guess        {:>8.2} ms  ({n} 条)", ms(t));
        let t = Instant::now();
        let n = d::phrase_group_guess(input).len();
        println!("  phrase_group_guess {:>8.2} ms  ({n} 条)", ms(t));
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}
