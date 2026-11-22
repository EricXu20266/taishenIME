//! 延迟拆测：把整句解码（`decode_meta`）从完整按键路径里分离出来。
//!
//! 用法: cargo run --release --example lat_split
//!
//! 诊断用临时探针：回答「单键 90ms 里，多少是整句解码，多少是其余环节」。

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

    // 预热
    let mut e = taishen_engine::Engine::new();
    for ch in "nihao".chars() {
        e.process_key(ch);
    }

    for input in [
        "nihao",
        "keyima",
        "shurushi",
        "womenyiqi",
        "yigezuci",
        "jintiantianqi",
        "zhongguorenmin",
    ] {
        // 纯解码耗时（同一输入连测 3 次取最小，排除首次建缓存）
        let mut best_decode = u128::MAX;
        let mut n = 0usize;
        for _ in 0..3 {
            let t0 = Instant::now();
            let r = taishen_engine::sentence::decode_meta(input, None, 9);
            let us = t0.elapsed().as_micros();
            n = r.len();
            if us < best_decode {
                best_decode = us;
            }
        }

        // 完整按键路径逐键峰值 + 末键耗时
        let mut eng = taishen_engine::Engine::new();
        eng.set_candidate_limit(9);
        let mut peak = 0u128;
        let mut last = 0u128;
        for ch in input.chars() {
            let k0 = Instant::now();
            eng.process_key(ch);
            let us = k0.elapsed().as_micros();
            last = us;
            if us > peak {
                peak = us;
            }
        }

        println!(
            "{input:>18} decode={:>7}us  full_peak={:>7}us  full_last={:>7}us  解码占比={:>5.1}%  cands={n}",
            best_decode,
            peak,
            last,
            if peak > 0 {
                best_decode as f64 / peak as f64 * 100.0
            } else {
                0.0
            }
        );
    }
}
