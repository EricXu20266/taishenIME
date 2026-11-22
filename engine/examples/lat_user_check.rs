//! 用户词库加载前后的每键延迟对照探针（诊断用，不入库）。
//!
//! 用法：
//!   cargo run --release --example lat_user_check            # 干净引擎
//!   cargo run --release --example lat_user_check -- --user  # 加载真实用户词库
//!
//! 对比目的：本次排序改动在 reduce_candidates 增加了一轮只读遍历与
//! user_short_index / common_full_index 查询，需确认用户词库加载后
//! 单键延迟没有明显上升。

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dict = root.join("resources/system_dict.db");
    let use_user = std::env::args().any(|a| a == "--user");
    let user_db = std::env::var("APPDATA")
        .ok()
        .map(|a| PathBuf::from(a).join("taishen-ime").join("user_dict.db"))
        .filter(|p| p.exists());

    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if use_user {
        match &user_db {
            Some(u) => taishen_engine::dictionary::set_user_dict_path(Some(u.as_path())),
            None => println!("[WARN] 未找到用户词库"),
        }
    }
    println!("[env] user={use_user} db={:?}", user_db.as_ref().map(|p| p.display().to_string()));

    // 预热，排除首查建缓存
    for input in ["nihao", "shurushi", "zhongguorenmin"] {
        let mut e = taishen_engine::Engine::new();
        for ch in input.chars() {
            e.process_key(ch);
        }
    }

    let inputs = [
        "ces", "ceshi", "nihao", "shi", "ji", "haiy", "fax", "keyima", "shurushi",
        "womenyiqi", "jintiantianqi", "zhongguorenmin",
    ];
    for input in inputs {
        for limit in [5usize, 9] {
            // 每个输入跑 5 轮取中位数，降低调度抖动
            let mut totals: Vec<u128> = Vec::new();
            let mut peaks: Vec<u128> = Vec::new();
            for _ in 0..5 {
                let mut e = taishen_engine::Engine::new();
                e.set_candidate_limit(limit);
                let mut per_key_us: Vec<u128> = Vec::new();
                let t0 = Instant::now();
                for ch in input.chars() {
                    let k0 = Instant::now();
                    e.process_key(ch);
                    per_key_us.push(k0.elapsed().as_micros());
                }
                totals.push(t0.elapsed().as_micros());
                peaks.push(per_key_us.iter().copied().max().unwrap_or(0));
            }
            totals.sort_unstable();
            peaks.sort_unstable();
            println!(
                "{input:>18} limit={limit} total_med={:>7}us peak_med={:>6}us",
                totals[2], peaks[2]
            );
        }
    }
}
