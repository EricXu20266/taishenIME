//! 每键延迟探针：定位长输入（≥3 音节）的卡顿。
//!
//! 用法: cargo run --release --example lat_probe
//!
//! 关注单键峰值（keys 数组），它是用户能感知的卡顿；总耗时包含首次建缓存。
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
    // 预热：排除首查建缓存的影响
    for input in ["nihao", "shurushi"] {
        let mut e = taishen_engine::Engine::new();
        for ch in input.chars() {
            e.process_key(ch);
        }
    }
    let inputs = [
        "nihao",          // 2 音节，精确命中
        "keyima",         // 3 音节
        "shurushi",       // 3 音节，无精确命中
        "womenyiqi",      // 4 音节
        "yigezuci",       // 4 音节
        "jintiantianqi",  // 5 音节（曾单键 395ms）
        "zhongguorenmin", // 5 音节
    ];
    for input in inputs {
        for limit in [5usize, 9] {
            let mut e = taishen_engine::Engine::new();
            e.set_candidate_limit(limit);
            let mut per_key_us: Vec<u128> = Vec::new();
            let t0 = Instant::now();
            for ch in input.chars() {
                let k0 = Instant::now();
                e.process_key(ch);
                per_key_us.push(k0.elapsed().as_micros());
            }
            println!(
                "{input:>18} limit={limit} total={:>7}us peak={:>6}us keys={:?}",
                t0.elapsed().as_micros(),
                per_key_us.iter().copied().max().unwrap_or(0),
                per_key_us
            );
        }
    }
}
