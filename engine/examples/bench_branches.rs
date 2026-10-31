//! 分支耗时归因：逐项关闭纠错 / 模糊 / 混输，观察长串输入耗时变化。
//!
//! 用法：cargo run --release --example bench_branches

use std::path::Path;
use std::time::Instant;

use taishen_engine::Engine;

fn measure(input: &str, correction: bool, fuzzy: bool, mix: bool) -> (u64, u64) {
    let mut engine = Engine::new();
    engine.set_correction_enabled(correction);
    engine.set_fuzzy_enabled(fuzzy);
    engine.set_mix_mode(mix);
    // 预热
    for ch in "warmup".chars() {
        engine.process_key(ch);
    }
    let mut samples = Vec::with_capacity(50);
    for _ in 0..50 {
        engine.reset();
        let t = Instant::now();
        for ch in input.chars() {
            engine.process_key(ch);
        }
        samples.push(t.elapsed().as_micros() as u64);
    }
    samples.sort_unstable();
    (samples[samples.len() / 2], samples[samples.len() * 95 / 100])
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

    let groups: &[(&str, bool, bool, bool)] = &[
        ("全开", true, true, true),
        ("关纠错", false, true, true),
        ("关模糊", true, false, true),
        ("关混输", true, true, false),
        ("全关", false, false, false),
    ];

    println!("{:<18} {:>14} {:>14}", "配置", "zhegeweomende", "xitongkongzhitai");
    for &(name, c, f, m) in groups {
        let a = measure("zhegeweomende", c, f, m);
        let b = measure("xitongkongzhitai", c, f, m);
        println!(
            "{:<18} {:>6}us/{:<5}us {:>6}us/{:<5}us",
            name, a.0, a.1, b.0, b.1
        );
    }
}
